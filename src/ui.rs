use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};
use crate::app::{url_decode, App, Focus};
use crate::parser::render_line;

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
        .title(Span::styled(" Files ", Style::default().add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(border_style);

    let inner = block.inner(area);
    f.render_widget(block, area);

    let visible_height = inner.height as usize;

    // Compute scroll offset to keep selected item visible
    if app.browser_selected < app.browser_offset {
        app.browser_offset = app.browser_selected;
    } else if app.browser_selected >= app.browser_offset + visible_height {
        app.browser_offset = app.browser_selected + 1 - visible_height;
    }

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
                if item.is_expanded { "▼ " } else { "▶ " }
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
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
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
        .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)))
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

    // Clamp scroll
    if app.content_scroll + visible_height > total && total > visible_height {
        app.content_scroll = total - visible_height;
    }

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
        let mut scrollbar_state = ScrollbarState::new(total.saturating_sub(visible_height))
            .position(app.content_scroll);
        f.render_stateful_widget(
            scrollbar,
            area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
            &mut scrollbar_state,
        );
    }
}

fn draw_statusbar(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.focus {
        Focus::Browser => {
            vec![
                Span::styled(" BROWSER ", Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)),
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
                Span::styled(" CONTENT ", Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD)),
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

    let bar = Paragraph::new(Line::from(hints))
        .style(Style::default().bg(Color::Reset));
    f.render_widget(bar, area);
}
