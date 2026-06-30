use crate::app::Focus;
use crate::parser::{ParsedLine, Segment, TaskState};
use crate::source::GraphSource;
use crate::view_model::ViewModel;
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
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

pub fn draw<S: GraphSource>(f: &mut Frame, app: &mut crate::app::App<S>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
        .split(chunks[0]);

    let browser_visible_height = main_chunks[0].height as usize;
    let content_visible_height = main_chunks[1].height as usize;

    let vm =
        crate::view_model::build_view_model(app, browser_visible_height, content_visible_height);

    draw_browser(f, &vm, main_chunks[0]);
    draw_content(f, &vm, main_chunks[1]);
    draw_statusbar(f, &vm, chunks[1]);
}

fn draw_browser(f: &mut Frame, vm: &ViewModel, area: Rect) {
    let focused = vm.browser.focused;
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

    let items: Vec<ListItem> = vm
        .browser
        .visible_rows
        .iter()
        .map(|row| {
            let indent = "  ".repeat(row.depth);
            let icon = if row.is_dir {
                if row.is_expanded {
                    "▼ "
                } else {
                    "▶ "
                }
            } else {
                "  "
            };
            let label = format!("{}{}{}", indent, icon, row.name);

            let style = if row.is_selected {
                if focused {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::Black).bg(Color::DarkGray)
                }
            } else if row.is_dir {
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

fn draw_content(f: &mut Frame, vm: &ViewModel, area: Rect) {
    let focused = vm.content.focused;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let block = Block::default()
        .title(Span::styled(
            vm.content.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    if vm.content.no_file_loaded {
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

    let lines: Vec<Line> = vm.content.visible_lines.iter().map(render_line).collect();

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);

    // Scrollbar
    if let Some(scrollbar_info) = &vm.content.scrollbar {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        let mut scrollbar_state =
            ScrollbarState::new(scrollbar_info.total).position(scrollbar_info.position);
        f.render_stateful_widget(
            scrollbar,
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

fn draw_statusbar(f: &mut Frame, vm: &ViewModel, area: Rect) {
    let hints = match vm.focus {
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
