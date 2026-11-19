use super::*;
use crate::api::schema::AgentStatus;
use ratatui::{
    text::Line,
    widgets::{Paragraph, Widget},
};

fn workspace_selection_background(palette: &Palette) -> ratatui::style::Color {
    if palette.selection_bg == ratatui::style::Color::Reset {
        palette.active_row_bg
    } else {
        palette.selection_bg
    }
}

pub(in crate::client::shell) fn workspace_active_background(
    palette: &Palette,
    navigating: bool,
) -> ratatui::style::Color {
    // The fallback cursor shares the active-row color; only fill the cursor while navigating.
    if navigating && palette.selection_bg == ratatui::style::Color::Reset {
        palette.sidebar_bg
    } else {
        palette.active_row_bg
    }
}

pub(in crate::client::shell) fn collapsed_sidebar_sections(
    area: Rect,
) -> (Rect, Option<u16>, Rect) {
    let content = Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height);
    if content.is_empty() {
        return (Rect::default(), None, Rect::default());
    }
    if content.height < 7 {
        return (content, None, Rect::default());
    }
    let workspace_height = content.height.div_ceil(2);
    let divider_y = content.y + workspace_height;
    let detail_height = content.height.saturating_sub(workspace_height + 1);
    (
        Rect::new(content.x, content.y, content.width, workspace_height),
        Some(divider_y),
        Rect::new(content.x, divider_y + 1, content.width, detail_height),
    )
}

pub(crate) fn render_collapsed_sidebar(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    selected_workspace_id: Option<&str>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    let selection_background = workspace_selection_background(palette);
    let active_background = workspace_active_background(palette, selected_workspace_id.is_some());
    render_sidebar_background(buffer, area, palette);
    let (workspace_area, divider_y, detail_area) = collapsed_sidebar_sections(area);
    for (index, workspace) in snapshot
        .workspaces
        .iter()
        .take(workspace_area.height as usize)
        .enumerate()
    {
        let rect = Rect::new(
            workspace_area.x,
            workspace_area.y + index as u16,
            workspace_area.width,
            1,
        );
        let selected = selected_workspace_id == Some(workspace.workspace_id.as_str());
        if selected {
            buffer.set_style(rect, Style::default().bg(selection_background));
        } else if workspace.focused {
            buffer.set_style(rect, Style::default().bg(active_background));
        }
        let number_style = if selected {
            Style::default()
                .fg(palette.overlay1)
                .bg(selection_background)
        } else if workspace.focused {
            Style::default().fg(palette.text).bg(active_background)
        } else {
            Style::default().fg(palette.overlay0)
        };
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width.min(2),
            &format!("{:<2}", index + 1),
            number_style,
        );
        let status = workspace.agent_status;
        put_text(
            buffer,
            rect.x.saturating_add(2),
            rect.y,
            rect.width.saturating_sub(2),
            status_icon(status, config.status_indicators),
            Style::default().fg(status_color(status, palette)),
        );
        hits.workspaces.push(WorkspaceHit {
            rect,
            endpoint_id: ClientEndpointId::Local,
            workspace_id: workspace.workspace_id.clone(),
            indented: false,
            group_toggle: None,
        });
    }

    if let Some(divider_y) = divider_y {
        put_text(
            buffer,
            workspace_area.x,
            divider_y,
            workspace_area.width,
            &"─".repeat(workspace_area.width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }

    let detail_content = Rect::new(
        detail_area.x,
        detail_area.y,
        detail_area.width,
        detail_area.height.saturating_sub(1),
    );
    for (index, pane_id) in super::ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .take(detail_content.height as usize)
        .enumerate()
    {
        let Some(agent) = snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == pane_id)
        else {
            continue;
        };
        let rect = Rect::new(
            detail_content.x,
            detail_content.y + index as u16,
            detail_content.width,
            1,
        );
        if agent.focused {
            buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
        }
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width.min(2),
            &format!("{:<2}", index + 1),
            Style::default().fg(if agent.focused {
                palette.text
            } else {
                palette.overlay0
            }),
        );
        put_text(
            buffer,
            rect.x.saturating_add(2),
            rect.y,
            rect.width.saturating_sub(2),
            status_icon(agent.agent_status, config.status_indicators),
            Style::default().fg(status_color(agent.agent_status, palette)),
        );
        hits.agents.push((rect, pane_id));
    }
    hits.sidebar_toggle = if area.is_empty() || workspace_area.width == 0 {
        Rect::default()
    } else {
        Rect::new(
            workspace_area.x + workspace_area.width / 2,
            area.bottom().saturating_sub(1),
            1,
            1,
        )
    };
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "»",
        if super::super::global_menu::global_menu_attention(snapshot) {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.overlay0)
        },
    );
}

pub(in crate::client::shell) enum SpaceChild<'a> {
    Agent(super::super::agent_sidebar::AgentRow),
    Pane(&'a crate::protocol::ClientShellPane),
    Tab(&'a ClientShellTab),
}

const FOLDED_STATUSES: [AgentStatus; 3] = [
    AgentStatus::Blocked,
    AgentStatus::Working,
    AgentStatus::Done,
];

#[derive(Default)]
struct FoldedSummary {
    panes: usize,
    statuses: [usize; FOLDED_STATUSES.len()],
}

fn render_pane_highlight(buffer: &mut Buffer, rect: Rect, indent: u16, palette: &Palette) {
    if indent == 0 {
        return;
    }
    buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
    for line in rect.y..rect.bottom() {
        put_text(
            buffer,
            rect.x + indent - 1,
            line,
            1,
            "▌",
            Style::default().fg(palette.accent),
        );
    }
}

pub(in crate::client::shell) fn space_rows<'a>(
    snapshot: &'a ClientShellSnapshot,
    workspaces: Vec<WorkspaceEntry>,
    config: &ClientShellConfig,
    folded_spaces: &HashSet<String>,
) -> Vec<(WorkspaceEntry, Option<SpaceChild<'a>>)> {
    let mut children = HashMap::<&str, Vec<SpaceChild>>::new();
    if config.spaces.unified_tree {
        let mut children_by_tab = HashMap::<&str, Vec<SpaceChild>>::new();
        let mut agent_panes = HashSet::new();
        for agent in &snapshot.agents {
            if folded_spaces.contains(&agent.workspace_id) {
                continue;
            }
            if let Some(row) =
                super::super::agent_sidebar::unified_agent_row(snapshot, agent, config)
            {
                agent_panes.insert(agent.pane_id.as_str());
                children_by_tab
                    .entry(&agent.tab_id)
                    .or_default()
                    .push(SpaceChild::Agent(row));
            }
        }
        if config.spaces.show_other_panes {
            for pane in &snapshot.panes {
                if !folded_spaces.contains(&pane.workspace_id)
                    && !agent_panes.contains(pane.pane_id.as_str())
                {
                    children_by_tab
                        .entry(&pane.tab_id)
                        .or_default()
                        .push(SpaceChild::Pane(pane));
                }
            }
        }
        for tab in &snapshot.tabs {
            if folded_spaces.contains(&tab.workspace_id) {
                continue;
            }
            let rows = children.entry(&tab.workspace_id).or_default();
            if let Some(children) = children_by_tab.remove(tab.tab_id.as_str()) {
                rows.extend(children);
            } else {
                rows.push(SpaceChild::Tab(tab));
            }
        }
    }
    let mut rows = Vec::with_capacity(workspaces.len());
    for entry in workspaces {
        rows.push((entry, None));
        if let Some(children) =
            children.remove(snapshot.workspaces[entry.index].workspace_id.as_str())
        {
            rows.extend(children.into_iter().map(|child| (entry, Some(child))));
        }
    }
    rows
}

pub(crate) fn render_sidebar(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    let (workspace_area, detail_area) = if config.spaces.unified_tree {
        (
            Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height),
            Rect::default(),
        )
    } else {
        hits.sidebar_section_divider =
            crate::ui::sidebar_section_divider_rect(area, state.sidebar_section_split);
        crate::ui::expanded_sidebar_sections(area, state.sidebar_section_split)
    };
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        " spaces",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );

    let entries = space_rows(
        snapshot,
        workspace_entries(snapshot, state.collapsed_groups),
        config,
        state.folded_spaces,
    );
    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1),
    );
    hits.workspace_body = body;
    hits.unified_spaces = config.spaces.unified_tree;
    let row_heights = entries
        .iter()
        .map(|(entry, tab)| {
            match tab {
                Some(SpaceChild::Agent(row)) => {
                    return row.rows.len().clamp(1, u16::MAX as usize) as u16
                }
                Some(SpaceChild::Pane(_) | SpaceChild::Tab(_)) => return 1,
                None if config.spaces.unified_tree => return 1,
                None => {}
            }
            snapshot
                .workspaces
                .get(entry.index)
                .map(|workspace| {
                    workspace_rows(
                        workspace,
                        displayed_workspace_status(snapshot, workspace, state.collapsed_groups),
                        entry.indented,
                        &config.spaces,
                    )
                    .len()
                    .max(1)
                    .min(u16::MAX as usize) as u16
                })
                .unwrap_or(1)
        })
        .collect::<Vec<_>>();
    let gaps = entries
        .iter()
        .enumerate()
        .map(|(index, _)| {
            entries.get(index + 1).map_or(0, |(next, tab)| match tab {
                None if !next.indented => config.spaces.row_gap,
                Some(SpaceChild::Agent(_))
                    if matches!(entries[index].1, Some(SpaceChild::Agent(_))) =>
                {
                    config.agents.row_gap
                }
                _ => 0,
            })
        })
        .collect::<Vec<_>>();
    let mut metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    let reveal_navigation = config.spaces.unified_tree
        && !body.is_empty()
        && std::mem::take(state.reveal_navigation_workspace);
    let reveal_focus = !body.is_empty() && std::mem::take(state.reveal_focused_workspace);
    if reveal_focus || reveal_navigation {
        let selected = state
            .selected_workspace_id
            .filter(|_| reveal_navigation)
            .and_then(|target| {
                entries.iter().position(|(entry, tab)| {
                    tab.is_none()
                        && target.matches(
                            state.active_endpoint_id,
                            &snapshot.workspaces[entry.index].workspace_id,
                        )
                })
            });
        let target = selected.or_else(|| {
            if reveal_focus {
                entries
                    .iter()
                    .position(|(_, child)| match child {
                        Some(SpaceChild::Agent(row)) => row.focused,
                        Some(SpaceChild::Pane(pane)) => pane.focused,
                        Some(SpaceChild::Tab(tab)) => tab.focused,
                        None => false,
                    })
                    .or_else(|| {
                        entries.iter().position(|(entry, child)| {
                            child.is_none() && snapshot.workspaces[entry.index].focused
                        })
                    })
            } else {
                None
            }
        });
        if let Some(target) = target {
            *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
                target,
            );
            metrics = super::scroll::list_scroll_metrics(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
            );
        }
    }
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut folded_summaries = HashMap::<&str, FoldedSummary>::new();
    if config.spaces.unified_tree && !state.folded_spaces.is_empty() {
        for pane in &snapshot.panes {
            if state.folded_spaces.contains(&pane.workspace_id) {
                folded_summaries
                    .entry(&pane.workspace_id)
                    .or_default()
                    .panes += 1;
            }
        }
        for agent in &snapshot.agents {
            if state.folded_spaces.contains(&agent.workspace_id) {
                if let Some(index) = FOLDED_STATUSES
                    .iter()
                    .position(|status| *status == agent.agent_status)
                {
                    folded_summaries
                        .entry(&agent.workspace_id)
                        .or_default()
                        .statuses[index] += 1;
                }
            }
        }
    }
    let mut y = body.y;
    for (entry_position, (entry, tab)) in entries.iter().enumerate().skip(*state.workspace_scroll) {
        if y >= body.bottom() {
            break;
        }
        if let Some(SpaceChild::Agent(row)) = tab {
            let height = row_heights[entry_position].min(body.height);
            if y.saturating_add(height) > body.bottom() {
                break;
            }
            let rect = Rect::new(body.x, y, content_width, height);
            let indent = content_width.min(if entry.indented { 3 } else { 1 });
            let nested = Rect::new(rect.x + indent, y, content_width - indent, height);
            if row.focused {
                render_pane_highlight(buffer, rect, indent, palette);
            }
            super::super::agent_sidebar::render_agent_row(buffer, nested, row, config);
            hits.agents.push((rect, row.pane_id.clone()));
            y = y
                .saturating_add(height)
                .saturating_add(gaps[entry_position]);
            continue;
        }
        if let Some(child) = tab {
            let rect = Rect::new(body.x, y, content_width, 1);
            let (label, focused) = match child {
                SpaceChild::Pane(pane) => {
                    hits.agents.push((rect, pane.pane_id.clone()));
                    (ordinary_pane_label(pane), pane.focused)
                }
                SpaceChild::Tab(tab) => {
                    hits.space_tabs.push((rect, tab.tab_id.clone()));
                    (tab.label.as_str(), tab.focused)
                }
                SpaceChild::Agent(_) => unreachable!("agent rows render above"),
            };
            if focused && snapshot.workspaces[entry.index].focused {
                let indent = content_width.min(if entry.indented { 3 } else { 1 });
                render_pane_highlight(buffer, rect, indent, palette);
            }
            let x = rect.x.saturating_add(if entry.indented { 6 } else { 4 });
            put_segment(
                buffer,
                x,
                y,
                rect.right(),
                label,
                Style::default().fg(if focused {
                    palette.text
                } else {
                    palette.subtext0
                }),
            );
            y = y.saturating_add(1).saturating_add(gaps[entry_position]);
            continue;
        }
        let Some(workspace) = snapshot.workspaces.get(entry.index) else {
            continue;
        };
        let row_height = row_heights[entry_position].min(body.height);
        if y.saturating_add(row_height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, row_height);
        let selected = state.selected_workspace_id.is_some_and(|target| {
            target.matches(state.active_endpoint_id, &workspace.workspace_id)
        });
        let dragged = state.dragged_workspace_id == Some(workspace.workspace_id.as_str());
        let folded =
            config.spaces.unified_tree && state.folded_spaces.contains(&workspace.workspace_id);
        if selected {
            buffer.set_style(
                rect,
                Style::default().bg(workspace_selection_background(palette)),
            );
        } else if dragged {
            buffer.set_style(rect, Style::default().bg(palette.surface1));
        } else if workspace.focused && !config.spaces.unified_tree {
            buffer.set_style(
                rect,
                Style::default().bg(workspace_active_background(
                    palette,
                    state.selected_workspace_id.is_some(),
                )),
            );
        }
        let group_toggle = if config.spaces.unified_tree {
            let empty = FoldedSummary::default();
            let summary = folded.then(|| {
                folded_summaries
                    .get(workspace.workspace_id.as_str())
                    .unwrap_or(&empty)
            });
            render_unified_heading(buffer, rect, workspace, entry.indented, summary, config);
            // Reorder uses the group key. Group collapse is available in the context menu.
            parent_group_key(snapshot, entry.index).map(|key| (Rect::default(), key))
        } else {
            let status = displayed_workspace_status(snapshot, workspace, state.collapsed_groups);
            let rows = workspace_rows(workspace, status, entry.indented, &config.spaces);
            render_workspace_rows(
                buffer,
                rect,
                status,
                config.status_indicators,
                entry,
                rows,
                workspace.focused,
                selected,
                state.selected_workspace_id.is_some(),
                dragged,
                palette,
            );
            render_parent_group_toggle(
                buffer,
                rect,
                snapshot,
                entry.index,
                state.collapsed_groups,
                palette,
            )
        };
        hits.workspaces.push(WorkspaceHit {
            rect,
            endpoint_id: ClientEndpointId::Local,
            workspace_id: workspace.workspace_id.clone(),
            indented: entry.indented,
            group_toggle,
        });
        y = y
            .saturating_add(row_height)
            .saturating_add(gaps[entry_position]);
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }

    if let Some(row) = state.workspace_drop_indicator_row.filter(|row| {
        *row >= workspace_area.y.saturating_add(1)
            && *row < workspace_area.bottom().saturating_sub(1)
    }) {
        put_text(
            buffer,
            body.x,
            row,
            body.width,
            &"─".repeat(body.width as usize),
            Style::default().fg(palette.accent),
        );
    }

    let footer_area = Rect {
        width: workspace_area
            .width
            .saturating_sub(u16::from(config.spaces.unified_tree)),
        ..workspace_area
    };
    let footer_y = footer_area.bottom().saturating_sub(1);
    if config.mouse_capture {
        hits.new_workspace = Rect::new(
            footer_area.x,
            footer_y,
            5.min(footer_area.width),
            u16::from(footer_area.height > 0),
        );
        put_text(
            buffer,
            footer_area.x,
            footer_y,
            footer_area.width,
            " new",
            Style::default().fg(palette.overlay0),
        );
        let attention = super::super::global_menu::global_menu_attention(snapshot);
        let launcher_width = if attention { 8 } else { 6 }.min(footer_area.width);
        hits.global_launcher = Rect::new(
            footer_area.right().saturating_sub(launcher_width),
            footer_y,
            launcher_width,
            1,
        );
        if attention {
            let start_x = footer_area.right().saturating_sub(6);
            put_text(
                buffer,
                start_x,
                footer_y,
                2,
                "● ",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            );
            put_text(
                buffer,
                start_x.saturating_add(2),
                footer_y,
                4,
                "menu",
                Style::default().fg(palette.overlay0),
            );
        } else {
            put_right_text(
                buffer,
                footer_area,
                footer_y,
                "menu",
                Style::default().fg(palette.overlay0),
            );
        }
    }

    super::render_agent_panel(
        buffer,
        detail_area,
        snapshot,
        config,
        state.agent_scroll,
        hits,
    );

    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.bottom().saturating_sub(1),
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

pub(crate) fn workspace_entries(
    snapshot: &ClientShellSnapshot,
    collapsed_groups: &HashSet<String>,
) -> Vec<WorkspaceEntry> {
    let mut members = HashMap::<&str, Vec<usize>>::new();
    for (index, workspace) in snapshot.workspaces.iter().enumerate() {
        if let Some(worktree) = &workspace.worktree {
            members.entry(&worktree.key).or_default().push(index);
        }
    }
    let grouped = members
        .iter()
        .filter(|(_, indices)| {
            indices.iter().any(|index| {
                snapshot.workspaces[*index]
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| worktree.is_linked_worktree)
            }) && indices.iter().any(|index| {
                snapshot.workspaces[*index]
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| !worktree.is_linked_worktree)
            })
        })
        .map(|(key, _)| *key)
        .collect::<HashSet<_>>();
    let mut emitted = HashSet::<&str>::new();
    let mut entries = Vec::new();
    for (index, workspace) in snapshot.workspaces.iter().enumerate() {
        let Some(worktree) = workspace
            .worktree
            .as_ref()
            .filter(|worktree| grouped.contains(worktree.key.as_str()))
        else {
            entries.push(WorkspaceEntry {
                index,
                indented: false,
                last_child: false,
            });
            continue;
        };
        if !emitted.insert(&worktree.key) {
            continue;
        }
        let Some(group_members) = members.get(worktree.key.as_str()) else {
            continue;
        };
        for parent in group_members.iter().copied().filter(|member| {
            snapshot.workspaces[*member]
                .worktree
                .as_ref()
                .is_some_and(|worktree| !worktree.is_linked_worktree)
        }) {
            entries.push(WorkspaceEntry {
                index: parent,
                indented: false,
                last_child: false,
            });
        }
        if collapsed_groups.contains(&worktree.key) {
            if let Some(active) = group_members.iter().copied().find(|member| {
                let workspace = &snapshot.workspaces[*member];
                workspace.focused
                    && workspace
                        .worktree
                        .as_ref()
                        .is_some_and(|worktree| worktree.is_linked_worktree)
            }) {
                entries.push(WorkspaceEntry {
                    index: active,
                    indented: true,
                    last_child: true,
                });
            }
            continue;
        }
        let children = group_members
            .iter()
            .copied()
            .filter(|member| {
                snapshot.workspaces[*member]
                    .worktree
                    .as_ref()
                    .is_some_and(|worktree| worktree.is_linked_worktree)
            })
            .collect::<Vec<_>>();
        for (child_index, child) in children.iter().enumerate() {
            entries.push(WorkspaceEntry {
                index: *child,
                indented: true,
                last_child: child_index + 1 == children.len(),
            });
        }
    }
    entries
}

fn parent_group_key(snapshot: &ClientShellSnapshot, index: usize) -> Option<String> {
    let workspace = snapshot.workspaces.get(index)?;
    let worktree = workspace.worktree.as_ref()?;
    if worktree.is_linked_worktree {
        return None;
    }
    snapshot
        .workspaces
        .iter()
        .any(|candidate| {
            candidate.worktree.as_ref().is_some_and(|candidate| {
                candidate.key == worktree.key && candidate.is_linked_worktree
            })
        })
        .then(|| worktree.key.clone())
}

pub(in crate::client::shell) fn workspace_close_is_group(
    snapshot: &ClientShellSnapshot,
    workspace: &ClientShellWorkspace,
) -> bool {
    let Some(worktree) = workspace
        .worktree
        .as_ref()
        .filter(|worktree| !worktree.is_linked_worktree)
    else {
        return false;
    };
    let mut has_child = false;
    for member in &snapshot.workspaces {
        if member.workspace_id == workspace.workspace_id {
            continue;
        }
        if let Some(candidate) = member
            .worktree
            .as_ref()
            .filter(|candidate| candidate.key == worktree.key)
        {
            if !candidate.is_linked_worktree {
                return false;
            }
            has_child = true;
        }
    }
    has_child
}

pub(in crate::client::shell) fn render_parent_group_toggle(
    buffer: &mut Buffer,
    workspace_rect: Rect,
    snapshot: &ClientShellSnapshot,
    workspace_index: usize,
    collapsed_groups: &HashSet<String>,
    palette: &Palette,
) -> Option<(Rect, String)> {
    let key = parent_group_key(snapshot, workspace_index)?;
    let toggle = Rect::new(
        workspace_rect.right().saturating_sub(1),
        workspace_rect.y,
        1,
        1,
    );
    put_text(
        buffer,
        toggle.x,
        toggle.y,
        toggle.width,
        if collapsed_groups.contains(&key) {
            "▸"
        } else {
            "▾"
        },
        Style::default().fg(palette.accent),
    );
    Some((toggle, key))
}

pub(in crate::client::shell) fn displayed_workspace_status(
    snapshot: &ClientShellSnapshot,
    workspace: &ClientShellWorkspace,
    collapsed_groups: &HashSet<String>,
) -> crate::api::schema::AgentStatus {
    let Some(worktree) = workspace
        .worktree
        .as_ref()
        .filter(|worktree| !worktree.is_linked_worktree)
    else {
        return workspace.agent_status;
    };
    if !collapsed_groups.contains(&worktree.key) {
        return workspace.agent_status;
    }
    snapshot
        .workspaces
        .iter()
        .filter(|candidate| {
            candidate.worktree.as_ref().is_some_and(|member| {
                member.key == worktree.key
                    && (member.is_linked_worktree
                        || candidate.workspace_id == workspace.workspace_id)
            })
        })
        .map(|candidate| candidate.agent_status)
        .max_by_key(|status| status_priority(*status))
        .unwrap_or(workspace.agent_status)
}

fn workspace_label(workspace: &ClientShellWorkspace, indented: bool) -> &str {
    if indented && !workspace.custom_label {
        workspace
            .branch
            .as_deref()
            .and_then(|branch| branch.strip_prefix("worktree/").or(Some(branch)))
            .unwrap_or(&workspace.label)
    } else {
        &workspace.label
    }
}

fn ordinary_pane_label(pane: &crate::protocol::ClientShellPane) -> &str {
    pane.label
        .as_deref()
        .filter(|label| !label.is_empty())
        .or_else(|| {
            let cwd = pane.foreground_cwd.as_deref().or(pane.cwd.as_deref())?;
            let name = cwd
                .trim_end_matches(['/', '\\'])
                .rsplit(['/', '\\'])
                .next()?;
            Some(if name.is_empty() { cwd } else { name })
        })
        .unwrap_or(&pane.pane_id)
}

fn render_unified_heading(
    buffer: &mut Buffer,
    area: Rect,
    workspace: &ClientShellWorkspace,
    indented: bool,
    summary: Option<&FoldedSummary>,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let label = workspace_label(workspace, indented);
    let style = Style::default().fg(palette.accent);
    let style = config
        .spaces
        .rows
        .iter()
        .flatten()
        .find(|token| matches!(token.parts().0, crate::config::SpaceSidebarToken::Workspace))
        .and_then(|token| token.style_for_value(label))
        .map_or(style, |patch| crate::ui::sidebar_token_style(style, patch));
    let indent = area.width.min(if indented { 2 } else { 0 });
    let mut width = area.width - indent;
    if let Some(summary) = summary {
        let suffix = format!("[{}]", summary.panes);
        let suffix_width = display_width(&suffix);
        if width > suffix_width + 1 {
            put_right_text(
                buffer,
                area,
                area.y,
                &suffix,
                Style::default().fg(palette.overlay0),
            );
            width -= suffix_width + 1;
        }
    }
    let mut statuses = Vec::new();
    if let Some(summary) = summary {
        for (status, count) in FOLDED_STATUSES.into_iter().zip(summary.statuses) {
            if count > 0 {
                let segment = format!(" {} {count}", status_icon(status, config.status_indicators));
                let segment_width = display_width(&segment);
                if width.saturating_sub(segment_width) >= display_width(label).min(4) {
                    width -= segment_width;
                    statuses.push((segment, status_color(status, palette)));
                }
            }
        }
    }
    let label = crate::ui::truncate_end(label, usize::from(width));
    put_text(buffer, area.x + indent, area.y, width, &label, style);
    let mut x = area.x + indent + display_width(&label);
    for (segment, color) in statuses {
        x = put_segment(
            buffer,
            x,
            area.y,
            area.right(),
            &segment,
            Style::default().fg(color),
        );
    }
}

pub(in crate::client::shell) fn workspace_rows(
    workspace: &ClientShellWorkspace,
    status: crate::api::schema::AgentStatus,
    indented: bool,
    config: &SpacesSidebarConfig,
) -> Vec<Vec<crate::ui::ResolvedToken>> {
    let label = workspace_label(workspace, indented);
    let token_values = workspace.tokens.iter().cloned().collect::<HashMap<_, _>>();
    crate::ui::sidebar_space_rows(
        config,
        crate::ui::SpaceTokenContext {
            workspace: label,
            branch: workspace.branch.as_deref(),
            state_text: status_text(status),
            ahead_behind: workspace.git_ahead_behind,
            tokens: &token_values,
            suppress_git_details: indented,
        },
    )
}

pub(in crate::client::shell) fn render_workspace_rows(
    buffer: &mut Buffer,
    area: Rect,
    status: crate::api::schema::AgentStatus,
    indicators: crate::config::StatusIndicatorStyle,
    entry: &WorkspaceEntry,
    rows: Vec<Vec<crate::ui::ResolvedToken>>,
    focused: bool,
    selected: bool,
    navigating: bool,
    dragged: bool,
    palette: &Palette,
) {
    for (row_index, row) in rows.iter().enumerate() {
        let y = area.y + row_index as u16;
        if y >= area.bottom() {
            break;
        }
        let mut x = area.x;
        if entry.indented {
            let prefix = if row_index == 0 {
                if entry.last_child {
                    "   └─ "
                } else {
                    "   ├─ "
                }
            } else if entry.last_child {
                "        "
            } else {
                "   │    "
            };
            x = put_segment(
                buffer,
                x,
                y,
                area.right(),
                prefix,
                Style::default().fg(palette.overlay0),
            );
        } else if row_index == 0 {
            x = x.saturating_add(1);
        } else {
            x = x.saturating_add(3);
        }
        let highlighted = focused || dragged;
        let workspace_style = Style::default()
            .fg(if highlighted {
                palette.text
            } else {
                palette.subtext0
            })
            .add_modifier(if highlighted {
                Modifier::BOLD
            } else {
                Modifier::empty()
            });
        let secondary_style = Style::default().fg(if focused {
            palette.mauve
        } else {
            palette.overlay0
        });
        let spans = crate::ui::resolved_token_spans(
            row,
            (
                status_icon(status, indicators),
                Style::default().fg(status_color(status, palette)),
            ),
            Style::default().fg(status_color(status, palette)),
            workspace_style,
            secondary_style,
            Style::default().fg(palette.overlay1),
            palette,
            area.right().saturating_sub(2).saturating_sub(x) as usize,
        );
        Paragraph::new(Line::from(spans)).render(
            Rect::new(x, y, area.right().saturating_sub(2).saturating_sub(x), 1),
            buffer,
        );
    }

    let background = if selected {
        Some(workspace_selection_background(palette))
    } else if dragged {
        Some(palette.surface1)
    } else if focused {
        Some(workspace_active_background(palette, navigating))
    } else {
        None
    };
    if let Some(background) = background {
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buffer[(x, y)].set_bg(background);
            }
        }
    }
}
