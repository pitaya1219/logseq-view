use crate::app::{url_decode, App, Focus};
use crate::parser::{ParsedLine, Segment, TaskState};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Wrap,
    },
    Frame,
};

fn render_line(parsed: &ParsedLine) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    if parsed.indent > 0 {
        spans.push(Span::raw("  ".repeat(parsed.indent)));
    }

    if parsed.is_bullet {
        let bullet_char = match parsed.task {
            Some(TaskState::Done) | Some(TaskState::Cancelled) => "✓ ",
            Some(_) => "○ ",
            None => "• ",
        };
        let style = Style::default().fg(Color::DarkGray);
        spans.push(Span::styled(bullet_char.to_string(), style));
    }

    if let Some(ref state) = parsed.task {
        let (label, style) = match state {
            TaskState::Todo => (
                "TODO",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            TaskState::Done => (
                "DONE",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::DIM),
            ),
            TaskState::Later => ("LATER", Style::default().fg(Color::Blue)),
            TaskState::Now => (
                "NOW",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            TaskState::Waiting => ("WAITING", Style::default().fg(Color::Cyan)),
            TaskState::Cancelled => (
                "CANCELLED",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
        };
        spans.push(Span::styled(label.to_string(), style));
        spans.push(Span::raw(" "));
    }

    for seg in &parsed.segments {
        match seg {
            Segment::Plain(s) => {
                let style = if matches!(
                    parsed.task,
                    Some(TaskState::Done) | Some(TaskState::Cancelled)
                ) {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(s.clone(), style));
            }
            Segment::PageLink(s) => {
                spans.push(Span::styled(
                    format!("[[{}]]", s),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            }
            Segment::Tag(s) => {
                spans.push(Span::styled(
                    format!("#{}", s),
                    Style::default().fg(Color::Green),
                ));
            }
            Segment::Bold(s) => {
                spans.push(Span::styled(
                    s.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
            }
            Segment::Italic(s) => {
                spans.push(Span::styled(
                    s.clone(),
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
            }
            Segment::Code(s) => {
                spans.push(Span::styled(
                    s.clone(),
                    Style::default().fg(Color::Yellow).bg(Color::DarkGray),
                ));
            }
            Segment::BlockRef(s) => {
                let preview: String = s.chars().take(8).collect();
                spans.push(Span::styled(
                    format!("(({}…))", preview),
                    Style::default().fg(Color::Magenta),
                ));
            }
            Segment::Property(key, val) => {
                spans.push(Span::styled(
                    key.clone(),
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(":: ", Style::default().fg(Color::DarkGray)));
                spans.push(Span::raw(val.clone()));
            }
        }
    }

    Line::from(spans)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(chunks[0]);

    draw_browser(f, app, main_chunks[0]);
    draw_content(f, app, main_chunks[1]);
    draw_statusbar(f, app, chunks[1]);
}

fn draw_browser(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Browser;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(Span::styled(
            " Files ",
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;

    app.clamp_browser_scroll(visible_height);

    let items: Vec<ListItem> = app
        .file_items
        .iter()
        .skip(app.browser_offset)
        .take(visible_height)
        .enumerate()
        .map(|(i, item)| {
            let abs_idx = i + app.browser_offset;
            let indent = "  ".repeat(item.depth);
            let icon = if item.is_dir {
                if item.is_expanded {
                    "▼ "
                } else {
                    "▶ "
                }
            } else {
                "  "
            };
            let label = format!("{}{}{}", indent, icon, item.name);

            let style = if abs_idx == app.browser_selected {
                if focused {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Black).bg(Color::DarkGray)
                }
            } else if item.is_dir {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Span::styled(label, style))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn draw_content(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Content;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = app
        .current_file
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| format!(" {} ", url_decode(&s.to_string_lossy())))
        .unwrap_or_else(|| " (no file) ".to_string());

    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.current_file.is_none() {
        let hint = Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "  Select a file in the left panel and press Enter",
                Style::default().fg(Color::DarkGray),
            )]),
        ]);
        f.render_widget(hint, inner);
        return;
    }

    let visible_height = inner.height as usize;
    let total = app.content_lines.len();

    app.clamp_content_scroll(visible_height);

    let lines: Vec<Line> = app
        .content_lines
        .iter()
        .skip(app.content_scroll)
        .take(visible_height)
        .map(render_line)
        .collect();

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);

    // Scrollbar
    if total > visible_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        let mut scrollbar_state =
            ScrollbarState::new(total.saturating_sub(visible_height)).position(app.content_scroll);
        f.render_stateful_widget(
            scrollbar,
            area.inner(ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn draw_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.focus {
        Focus::Browser => {
            vec![
                Span::styled(
                    " BROWSER ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled("↑↓/jk", Style::default().fg(Color::Yellow)),
                Span::raw(" navigate  "),
                Span::styled("Enter", Style::default().fg(Color::Yellow)),
                Span::raw(" open  "),
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw(" switch pane  "),
                Span::styled("q", Style::default().fg(Color::Red)),
                Span::raw(" quit"),
            ]
        }
        Focus::Content => {
            vec![
                Span::styled(
                    " CONTENT ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled("↑↓/jk", Style::default().fg(Color::Yellow)),
                Span::raw(" scroll  "),
                Span::styled("gg/G", Style::default().fg(Color::Yellow)),
                Span::raw(" top/bottom  "),
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw(" switch pane  "),
                Span::styled("q", Style::default().fg(Color::Red)),
                Span::raw(" quit"),
            ]
        }
    };

    let bar = Paragraph::new(Line::from(hints)).style(Style::default().bg(Color::Reset));
    f.render_widget(bar, area);
}
