use std::collections::HashMap;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::*;

pub(super) struct AgentRow {
    pub(super) pane_id: String,
    pub(super) status: crate::api::schema::AgentStatus,
    pub(super) focused: bool,
    pub(super) rows: Vec<Vec<crate::ui::ResolvedToken>>,
    unified: bool,
}

pub(super) fn ordered_agent_pane_ids(
    snapshot: &ClientShellSnapshot,
    sort: crate::config::AgentPanelSortConfig,
) -> Vec<String> {
    if snapshot.agent_view_label.is_some() {
        return snapshot
            .agent_order
            .iter()
            .filter(|pane_id| {
                snapshot
                    .agents
                    .iter()
                    .any(|agent| agent.pane_id == pane_id.as_str())
            })
            .cloned()
            .collect();
    }
    let mut agents = snapshot.agents.iter().collect::<Vec<_>>();
    if sort == crate::config::AgentPanelSortConfig::Priority {
        agents.sort_by_key(|agent| {
            (
                std::cmp::Reverse(status_priority(agent.agent_status)),
                std::cmp::Reverse(agent.state_change_seq),
            )
        });
    }
    agents
        .into_iter()
        .map(|agent| agent.pane_id.clone())
        .collect()
}

pub(super) fn render_agent_panel(
    buffer: &mut Buffer,
    area: Rect,
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
) {
    if !render_agent_panel_header(
        buffer,
        area,
        snapshot.agent_view_label.as_deref(),
        config,
        hits,
    ) {
        return;
    }

    let rows = agent_rows(snapshot, config, None);
    render_agent_list(
        buffer,
        area,
        &rows,
        snapshot
            .agent_view_label
            .as_ref()
            .map(|_| " no matching agents"),
        config,
        agent_scroll,
        hits,
        |row| row.rows.len(),
        |buffer, rect, row, hits| {
            hits.agents.push((rect, row.pane_id.clone()));
            render_agent_row(buffer, rect, row, config);
        },
    );
}

pub(super) fn render_agent_panel_header(
    buffer: &mut Buffer,
    area: Rect,
    agent_view_label: Option<&str>,
    config: &ClientShellConfig,
    hits: &mut ShellHitMap,
) -> bool {
    if area.height == 0 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        &"─".repeat(area.width as usize),
        Style::default().fg(config.palette.surface_dim),
    );
    if area.height < 2 {
        return false;
    }
    put_text(
        buffer,
        area.x,
        area.y + 1,
        area.width,
        " agents",
        Style::default()
            .fg(config.palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    let sort_label = agent_view_label.unwrap_or(match config.agent_panel_sort {
        crate::config::AgentPanelSortConfig::Spaces => "grouped",
        crate::config::AgentPanelSortConfig::Priority => "priority",
    });
    let sort_width = display_width(sort_label).min(area.width as usize) as u16;
    let sort_rect = Rect::new(
        area.right().saturating_sub(sort_width),
        area.y + 1,
        sort_width,
        1,
    );
    hits.agent_sort_toggle = if config.mouse_capture && agent_view_label.is_none() {
        sort_rect
    } else {
        Rect::default()
    };
    put_text(
        buffer,
        sort_rect.x,
        sort_rect.y,
        sort_rect.width,
        sort_label,
        Style::default()
            .fg(if agent_view_label.is_some() {
                config.palette.accent
            } else {
                config.palette.overlay0
            })
            .add_modifier(Modifier::BOLD),
    );
    true
}

pub(super) fn render_agent_list<T>(
    buffer: &mut Buffer,
    area: Rect,
    rows: &[T],
    empty_message: Option<&str>,
    config: &ClientShellConfig,
    agent_scroll: &mut usize,
    hits: &mut ShellHitMap,
    row_lines: impl Fn(&T) -> usize,
    mut render_row: impl FnMut(&mut Buffer, Rect, &T, &mut ShellHitMap),
) {
    let body = Rect::new(
        area.x,
        area.y.saturating_add(3),
        area.width,
        area.height.saturating_sub(3),
    );
    hits.agent_body = body;
    if body.is_empty() || rows.is_empty() {
        *agent_scroll = 0;
        if let Some(message) = empty_message.filter(|_| !body.is_empty()) {
            put_text(
                buffer,
                body.x,
                body.y,
                body.width,
                message,
                Style::default()
                    .fg(config.palette.overlay0)
                    .add_modifier(Modifier::DIM),
            );
        }
        return;
    }

    let row_heights = rows
        .iter()
        .map(|row| row_lines(row).max(1).min(u16::MAX as usize) as u16)
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            }
        })
        .collect::<Vec<_>>();
    let metrics =
        super::scroll::list_scroll_metrics(&row_heights, &gaps, body.height, *agent_scroll);
    hits.agent_max_scroll = metrics.max_offset_from_bottom;
    hits.agent_scroll_metrics = Some(metrics);
    *agent_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (index, row) in rows.iter().enumerate().skip(*agent_scroll) {
        let height = row_heights[index].min(body.height);
        if y.saturating_add(height) > body.bottom() {
            break;
        }
        let rect = Rect::new(body.x, y, content_width, height);
        render_row(buffer, rect, row, hits);
        y = y
            .saturating_add(height)
            .saturating_add(if index + 1 < rows.len() {
                config.agents.row_gap
            } else {
                0
            });
    }

    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.agent_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, &config.palette);
    }
}

pub(super) fn agent_rows(
    snapshot: &ClientShellSnapshot,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Vec<AgentRow> {
    ordered_agent_pane_ids(snapshot, config.agent_panel_sort)
        .into_iter()
        .filter_map(|pane_id| agent_row(snapshot, &pane_id, config, machine))
        .collect()
}

pub(super) fn agent_row(
    snapshot: &ClientShellSnapshot,
    pane_id: &str,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let agent = snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == pane_id)?;
    agent_row_for(snapshot, agent, config, machine)
}

pub(super) fn unified_agent_row(
    snapshot: &ClientShellSnapshot,
    agent: &crate::protocol::ClientShellAgent,
    config: &ClientShellConfig,
) -> Option<AgentRow> {
    let mut row = agent_row_for(snapshot, agent, config, None)?;
    row.unified = true;
    for tokens in &mut row.rows {
        tokens.retain(|token| !matches!(token.kind, crate::ui::ResolvedTokenKind::Workspace(_)));
    }
    row.rows.retain(|tokens| !tokens.is_empty());
    if row.rows.is_empty() {
        row.rows.push(vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]);
    }
    let first = &mut row.rows[0];
    if first.iter().all(|t| {
        matches!(
            t.kind,
            crate::ui::ResolvedTokenKind::StateIcon | crate::ui::ResolvedTokenKind::StateText(_)
        )
    }) {
        let label = [
            agent.terminal_title_stripped.as_deref(),
            agent.title.as_deref(),
            agent.display_agent.as_deref(),
            agent.name.as_deref(),
            agent.agent.as_deref(),
        ]
        .into_iter()
        .flatten()
        .find(|label| !label.trim().is_empty())
        .unwrap_or(&agent.pane_id);
        first.push(crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::TerminalTitle(label.to_owned()),
            style: Default::default(),
        });
    }
    Some(row)
}

fn agent_row_for(
    snapshot: &ClientShellSnapshot,
    agent: &crate::protocol::ClientShellAgent,
    config: &ClientShellConfig,
    machine: Option<&str>,
) -> Option<AgentRow> {
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == agent.workspace_id)?;
    let tab = snapshot.tabs.iter().find(|tab| tab.tab_id == agent.tab_id);
    let pane = snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == agent.pane_id);
    let tab_count = snapshot
        .tabs
        .iter()
        .filter(|candidate| candidate.workspace_id == agent.workspace_id)
        .count();
    let tab_label = tab
        .filter(|tab| tab_count > 1 || tab.custom_label)
        .map(|tab| tab.label.as_str());
    let agent_label = agent
        .display_agent
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.agent.as_deref())
        .or(agent.title.as_deref());
    let labels = agent
        .state_labels
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    let tokens = agent.tokens.iter().cloned().collect::<HashMap<_, _>>();
    let state_text = labels
        .get(status_text(agent.agent_status))
        .map(String::as_str)
        .unwrap_or_else(|| sidebar_status_text(agent.agent_status));
    let canonical_agent = agent
        .agent
        .as_deref()
        .and_then(crate::detect::parse_agent_label);
    let rows = crate::ui::sidebar_agent_rows(
        &config.agents,
        crate::ui::AgentTokenContext {
            machine,
            workspace: &workspace.label,
            tab: tab_label,
            pane: agent
                .title
                .as_deref()
                .or_else(|| pane.and_then(|pane| pane.label.as_deref())),
            agent_label,
            terminal_title: agent.terminal_title.as_deref(),
            terminal_title_stripped: agent.terminal_title_stripped.as_deref(),
            canonical_agent,
            tokens: &tokens,
        },
        state_text,
    );
    Some(AgentRow {
        pane_id: agent.pane_id.clone(),
        status: agent.agent_status,
        focused: agent.focused,
        rows,
        unified: false,
    })
}

pub(super) fn render_agent_row(
    buffer: &mut Buffer,
    rect: Rect,
    row: &AgentRow,
    config: &ClientShellConfig,
) {
    let palette = &config.palette;
    let row_style = if row.focused {
        Style::default().bg(palette.active_row_bg)
    } else {
        Style::default()
    };
    let name_style = if row.focused {
        Style::default()
            .fg(palette.text)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(palette.subtext0)
            .add_modifier(Modifier::BOLD)
    };
    let status_style = Style::default().fg(status_color(row.status, palette));
    let secondary = Style::default().fg(palette.overlay0);
    let icon = (
        status_icon(row.status, config.status_indicators),
        Style::default().fg(status_color(row.status, palette)),
    );
    let rows = if row.rows.is_empty() {
        vec![vec![crate::ui::ResolvedToken {
            kind: crate::ui::ResolvedTokenKind::StateIcon,
            style: Default::default(),
        }]]
    } else {
        row.rows.clone()
    };
    for (index, tokens) in rows.iter().take(rect.height as usize).enumerate() {
        let indent = if index == 0 { 1 } else { 3 };
        let width = rect.width.saturating_sub(indent as u16) as usize;
        let secondary = if row.unified && index > 0 {
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::DIM)
        } else {
            secondary
        };
        let custom_style = if row.unified && index == 0 {
            Style::default().fg(palette.text)
        } else {
            secondary
        };
        let render_tokens = |tokens: &[crate::ui::ResolvedToken], width| {
            crate::ui::resolved_token_spans(
                tokens,
                icon,
                status_style,
                name_style,
                secondary,
                custom_style,
                palette,
                width,
            )
        };
        let mut spans = vec![Span::raw(" ".repeat(indent))];
        if row.unified && index > 0 {
            spans.extend(aligned_metadata(tokens, width, render_tokens));
        } else {
            spans.extend(render_tokens(tokens, width));
        }
        Paragraph::new(Line::from(spans)).style(row_style).render(
            Rect::new(rect.x, rect.y + index as u16, rect.width, 1),
            buffer,
        );
    }
}

fn aligned_metadata(
    tokens: &[crate::ui::ResolvedToken],
    width: usize,
    render: impl Fn(&[crate::ui::ResolvedToken], usize) -> Vec<Span<'static>>,
) -> Vec<Span<'static>> {
    let (left, launcher) = match tokens.split_last() {
        Some((token, left)) if matches!(token.kind, crate::ui::ResolvedTokenKind::Agent(_)) => {
            (left, render(std::slice::from_ref(token), width))
        }
        _ => (tokens, Vec::new()),
    };
    let launcher_width = launcher.iter().map(Span::width).sum::<usize>();
    let left_limit = width.saturating_sub(launcher_width + 2 * usize::from(launcher_width > 0));
    let mut spans = Vec::new();
    let mut used = 0;
    for token in left {
        let gap = usize::from(used > 0);
        let budget = left_limit.saturating_sub(used + gap);
        if budget == 0 {
            break;
        }
        let rendered = render(std::slice::from_ref(token), budget);
        let rendered_width = rendered.iter().map(Span::width).sum::<usize>();
        if rendered_width > 0 && rendered_width <= budget {
            if gap > 0 {
                spans.push(Span::raw(" "));
            }
            spans.extend(rendered);
            used += gap + rendered_width;
        }
    }
    if launcher_width > 0 {
        spans.push(Span::raw(
            " ".repeat(width.saturating_sub(used + launcher_width)),
        ));
        spans.extend(launcher);
    }
    spans
}

fn put_text(buffer: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    for (offset, character) in text.chars().take(width as usize).enumerate() {
        if let Some(cell) = buffer.cell_mut((x + offset as u16, y)) {
            cell.set_char(character).set_style(style);
        }
    }
}

fn display_width(text: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(text)
}

fn sidebar_status_text(status: crate::api::schema::AgentStatus) -> &'static str {
    use crate::api::schema::AgentStatus;
    match status {
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Working => "working",
        AgentStatus::Idle | AgentStatus::Unknown => "idle",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{ResolvedToken, ResolvedTokenKind};

    #[test]
    fn unified_metadata_reserves_the_right_edge_for_the_launcher() {
        let config = ClientShellConfig::from_config(&crate::config::Config::default());
        let token = |kind| ResolvedToken {
            kind,
            style: Default::default(),
        };
        let mut row = AgentRow {
            pane_id: "pane_1".into(),
            status: crate::api::schema::AgentStatus::Idle,
            focused: true,
            rows: vec![
                vec![
                    token(ResolvedTokenKind::StateIcon),
                    token(ResolvedTokenKind::TerminalTitle("Review".into())),
                ],
                vec![
                    token(ResolvedTokenKind::Branch("feature/very-long-branch".into())),
                    token(ResolvedTokenKind::Agent("podex".into())),
                ],
            ],
            unified: true,
        };
        for (width, launcher) in [(6, "po…"), (9, "podex"), (18, "podex")] {
            for focused in [true, false] {
                row.focused = focused;
                let rect = Rect::new(0, 0, width, 2);
                let mut buffer = Buffer::empty(rect);
                render_agent_row(&mut buffer, rect, &row, &config);
                let metadata = (0..width)
                    .map(|x| buffer[(x, 1)].symbol())
                    .collect::<String>();
                assert_eq!(metadata.starts_with("   f"), width == 18, "{metadata}");
                assert!(!metadata.contains('·'), "{metadata}");
                assert!(metadata.ends_with(launcher), "{metadata}");
                assert_eq!(buffer[(3, 0)].modifier & Modifier::BOLD, Modifier::empty());
                assert!(buffer[(width - 1, 1)].modifier.contains(Modifier::DIM));
            }
        }
    }
}
