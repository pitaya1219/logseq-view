use crate::app::Focus;
use crate::parser::{FragmentKind, ParsedLine, TaskState};
use crate::source::GraphSource;
use crate::view_model::{LineHighlight, ViewModel};
use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    prelude::Stylize,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    Frame,
};

/// Columns `draw_content` reserves in front of the text for the cursor-block
/// gutter bar (see the `gutter` span built in `draw_content`). Subtracted
/// from the pane's inner width before it's used as `wrap_width`, so wrapping
/// never assumes the gutter column is available for text.
const CONTENT_GUTTER_COLS: usize = 1;

/// Word-wraps a line's styled spans to `width` columns, splitting spans at
/// row boundaries while preserving each fragment's original style. Backed by
/// `wrap::wrap_row_ranges` -- see the comment at its call site in
/// `draw_content` for why this exists instead of `Paragraph::wrap()`.
fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let mut full_text = String::new();
    let mut span_ranges: Vec<(std::ops::Range<usize>, Style)> = Vec::with_capacity(spans.len());
    for span in &spans {
        let start = full_text.len();
        full_text.push_str(&span.content);
        span_ranges.push((start..full_text.len(), span.style));
    }

    crate::wrap::wrap_row_ranges(&full_text, width)
        .into_iter()
        .map(|row_range| {
            span_ranges
                .iter()
                .filter_map(|(span_range, style)| {
                    let start = span_range.start.max(row_range.start);
                    let end = span_range.end.min(row_range.end);
                    if start < end {
                        Some(Span::styled(full_text[start..end].to_string(), *style))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .collect()
}

/// The style for one `DisplayFragment`, given whether the line as a whole is
/// a completed/cancelled task (which dims its `Plain` text -- a line-level
/// fact, not a per-fragment one, so it's threaded in separately rather than
/// carried on `FragmentKind` itself).
fn style_for_fragment(kind: &FragmentKind, dim_plain_text: bool) -> Style {
    match kind {
        FragmentKind::Indent => Style::default(),
        FragmentKind::Bullet => Style::default().fg(Color::DarkGray),
        FragmentKind::TaskLabel(state) => match state {
            TaskState::Todo => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            TaskState::Done => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::DIM),
            TaskState::Later => Style::default().fg(Color::Blue),
            TaskState::Now => Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            TaskState::Waiting => Style::default().fg(Color::Cyan),
            TaskState::Cancelled => Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::CROSSED_OUT),
        },
        FragmentKind::Plain => {
            if dim_plain_text {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM)
            } else {
                Style::default()
            }
        }
        FragmentKind::PageLink => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
        FragmentKind::Tag => Style::default().fg(Color::Green),
        FragmentKind::Bold => Style::default().add_modifier(Modifier::BOLD),
        FragmentKind::Italic => Style::default().add_modifier(Modifier::ITALIC),
        FragmentKind::Code => Style::default().fg(Color::Yellow).bg(Color::DarkGray),
        FragmentKind::BlockRef => Style::default().fg(Color::Magenta),
        FragmentKind::PropertyKey => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
        FragmentKind::PropertySeparator => Style::default().fg(Color::DarkGray),
        FragmentKind::PropertyValue => Style::default(),
    }
}

/// Renders a `ParsedLine` by styling `parser::line_display_fragments`'
/// output -- the ONLY place that line's text is built. This function decides
/// how each fragment looks; it never reconstructs what the fragment's text
/// is, so it can't drift from `line_display_text`/`line_row_count` (used for
/// wrap-row math) the way two independent text-building implementations
/// could. See the doc comment on `line_display_fragments` for why that
/// matters (#71).
fn render_line(parsed: &ParsedLine) -> Line<'static> {
    let dim_plain_text = matches!(
        parsed.task,
        Some(TaskState::Done) | Some(TaskState::Cancelled)
    );

    let spans: Vec<Span<'static>> = crate::parser::line_display_fragments(parsed)
        .into_iter()
        .map(|fragment| {
            let style = style_for_fragment(&fragment.kind, dim_plain_text);
            Span::styled(fragment.text, style)
        })
        .collect();

    Line::from(spans)
}

pub fn draw<S: GraphSource>(f: &mut Frame, app: &mut crate::app::App<S>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let browser_collapsed = app.browser_collapsed;
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(if browser_collapsed {
            [Constraint::Length(0), Constraint::Percentage(100)]
        } else {
            [Constraint::Percentage(25), Constraint::Percentage(75)]
        })
        .split(chunks[0]);

    // Both panes are drawn inside a `Borders::ALL` block, so the actual number of
    // visible rows is the pane height minus the top and bottom border (2 rows).
    // Clamping must use this inner height, otherwise the selection/scroll can sit
    // up to 2 rows below the visible area (selection clipped at the bottom).
    const BORDER_ROWS: usize = 2;
    // Left + right borders (2 cols) plus the one-column cursor-block gutter
    // `draw_content` always reserves in front of the text (see `CONTENT_GUTTER_COLS`).
    const BORDER_COLS: usize = 2;
    let browser_visible_height = (main_chunks[0].height as usize).saturating_sub(BORDER_ROWS);
    let content_visible_height = (main_chunks[1].height as usize).saturating_sub(BORDER_ROWS);
    let content_visible_width = (main_chunks[1].width as usize)
        .saturating_sub(BORDER_COLS)
        .saturating_sub(CONTENT_GUTTER_COLS);

    let vm = crate::view_model::build_view_model(
        app,
        browser_visible_height,
        content_visible_height,
        content_visible_width,
    );

    if !browser_collapsed {
        draw_browser(f, &vm, main_chunks[0]);
    }
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

    // Build styled rows: a left-column gutter bar marks the current block,
    // and the text carries the (search-only) highlight. The two are separate
    // channels, so a line can show both at once. Each logical line can wrap
    // into more than one terminal row (`wrap_spans`, at `vm.content.wrap_width`
    // -- the SAME width `view_model::build_content_view` used to decide which
    // lines fit, per the invariant documented on `ContentView::wrap_width`);
    // the gutter/highlight styling is repeated on every wrapped row of a
    // line so a highlighted or cursor-block line reads as one continuous
    // block rather than just its first row.
    let lines: Vec<Line> = vm
        .content
        .visible_lines
        .iter()
        .zip(vm.content.line_highlights.iter())
        .zip(vm.content.cursor_block.iter())
        .flat_map(|((line, highlight), in_cursor_block)| {
            let base_line = render_line(line);
            let text_spans: Vec<Span> = match highlight {
                LineHighlight::Current => base_line
                    .spans
                    .into_iter()
                    .map(|span| span.bg(Color::Yellow).fg(Color::Black))
                    .collect(),
                LineHighlight::Match => base_line
                    .spans
                    .into_iter()
                    .map(|span| span.bg(Color::DarkGray))
                    .collect(),
                LineHighlight::None => base_line.spans,
            };

            // Every row gets a one-column gutter so the text stays aligned;
            // only cursor-block rows show the bar (in the otherwise empty
            // leading column — no character or bullet there).
            let gutter = if *in_cursor_block {
                Span::styled("▎", Style::default().fg(Color::Cyan))
            } else {
                Span::raw(" ")
            };

            wrap_spans(text_spans, vm.content.wrap_width)
                .into_iter()
                .map(move |row_spans| {
                    let mut spans = Vec::with_capacity(row_spans.len() + 1);
                    spans.push(gutter.clone());
                    spans.extend(row_spans);
                    Line::from(spans)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // ratatui's own `Paragraph::wrap()` isn't used: the scroll/cursor model
    // (`content_scroll`, `clamp_content_cursor_scroll`, the scrollbar) needs
    // to know exactly how many rows a line will take BEFORE rendering, and a
    // widget's internal wrap algorithm isn't something callers can predict
    // from the outside. `wrap_spans` (backed by `wrap::wrap_row_ranges`) is
    // the single algorithm both this function and `view_model` use, so the
    // two can never disagree about row counts the way truncation was papering
    // over after #71.
    let paragraph = Paragraph::new(lines);
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
    let toggle_browser_hint = if vm.browser_collapsed {
        "show files"
    } else {
        "hide files"
    };

    let mut hints = match vm.focus {
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
                Span::styled("/", Style::default().fg(Color::Yellow)),
                Span::raw(" filter  "),
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw(" switch pane  "),
                Span::styled("^B", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {toggle_browser_hint}  ")),
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
                Span::raw(" move  "),
                Span::styled("gg/G", Style::default().fg(Color::Yellow)),
                Span::raw(" top/bottom  "),
                Span::styled("e", Style::default().fg(Color::Yellow)),
                Span::raw(" edit  "),
                Span::styled("E", Style::default().fg(Color::Yellow)),
                Span::raw(" edit block  "),
                Span::styled("Tab", Style::default().fg(Color::Yellow)),
                Span::raw(" switch pane  "),
                Span::styled("^B", Style::default().fg(Color::Yellow)),
                Span::raw(format!(" {toggle_browser_hint}  ")),
                Span::styled("q", Style::default().fg(Color::Red)),
                Span::raw(" quit"),
            ]
        }
    };

    // Browser filter prompt
    if vm.focus == Focus::Browser
        && (vm.browser_filter_active || !vm.browser_filter_query.is_empty())
    {
        let filter_span = if vm.browser_filter_active {
            let display_text = if vm.browser_filter_query.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", vm.browser_filter_query)
            };
            Span::styled(
                display_text,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!("/{}", vm.browser_filter_query),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        };
        hints.insert(0, filter_span);
        hints.insert(1, Span::raw(" "));
    }

    // Content search prompt with match counter
    if vm.focus == Focus::Content
        && (vm.content_search_active || !vm.content_search_query.is_empty())
    {
        let counter = if !vm.content_search_query.is_empty() {
            if vm.content.match_count > 0 {
                if let Some(current) = vm.content.current_match {
                    format!(" [{}/{}]", current, vm.content.match_count)
                } else {
                    format!(" [-/{}]", vm.content.match_count)
                }
            } else {
                " [no matches]".to_string()
            }
        } else {
            String::new()
        };

        let search_span = if vm.content_search_active {
            let display_text = if vm.content_search_query.is_empty() {
                "/".to_string()
            } else {
                format!("/{}", vm.content_search_query)
            };
            Span::styled(
                display_text,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                format!("/{}", vm.content_search_query),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        };

        hints.insert(0, search_span);
        if !counter.is_empty() {
            hints.insert(1, Span::raw(counter));
        }
    }

    let bar = Paragraph::new(Line::from(hints)).style(Style::default().bg(Color::Reset));
    f.render_widget(bar, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, FileItem};
    use crate::source::FakeGraphSource;
    use ratatui::{backend::TestBackend, Terminal};
    use std::path::PathBuf;

    fn rendered_text(width: u16, height: u16, app: &mut App<FakeGraphSource>) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn browser_selection_at_bottom_is_visible() {
        // Regression: clamping used the pane's outer height instead of the height
        // inside the border, so the selected row could be clipped below the view
        // when scrolled to the bottom. The last item must remain rendered.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items = (0..30)
            .map(|i| FileItem {
                path: PathBuf::from(format!("item{i}")),
                name: format!("item{i}"),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            })
            .collect();
        app.focus = Focus::Browser;
        app.browser_selected = 29;
        app.browser_offset = 0;

        let text = rendered_text(50, 22, &mut app);
        assert!(
            text.contains("item29"),
            "selected bottom row should be visible, but it was clipped.\nBuffer:\n{text}"
        );
    }

    /// `render_line` styles `parser::line_display_fragments`' output without
    /// altering the fragment text itself (see `render_line`'s doc comment),
    /// so its rendered text is always exactly `line_display_text` -- no
    /// separate cross-check needed; this just documents the invariant with a
    /// couple of representative lines.
    #[test]
    fn render_line_text_matches_line_display_text() {
        use crate::parser::{Segment, TaskState};

        fn rendered_plain_text(line: &crate::parser::ParsedLine) -> String {
            render_line(line)
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect()
        }

        let cases = vec![
            crate::parser::ParsedLine {
                indent: 2,
                is_bullet: true,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("plain text".to_string())],
                ..Default::default()
            },
            crate::parser::ParsedLine {
                indent: 1,
                is_bullet: true,
                task: None,
                segments: vec![
                    Segment::Plain("see ".to_string()),
                    Segment::PageLink("Other Page".to_string()),
                    Segment::Tag("logseq".to_string()),
                    Segment::BlockRef("0123456789abcdef".to_string()),
                ],
                ..Default::default()
            },
            crate::parser::ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Property(
                    "status".to_string(),
                    "active".to_string(),
                )],
                ..Default::default()
            },
        ];

        for line in &cases {
            assert_eq!(
                crate::parser::line_display_text(line),
                rendered_plain_text(line),
            );
        }
    }

    fn plain_line(text: &str) -> crate::parser::ParsedLine {
        crate::parser::ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![crate::parser::Segment::Plain(text.to_string())],
            ..Default::default()
        }
    }

    #[test]
    fn long_line_is_wrapped_not_truncated() {
        // The point of wrapping instead of truncating (see #71, and the
        // proper fix here): a long line's full text must actually be
        // readable, not cut off at the pane's width. A marker placed at the
        // very end of a long line only shows up in the rendered buffer if
        // the line wrapped onto further rows instead of being clipped.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = vec![plain_line(&format!("{}TAILMARKER", "x".repeat(60)))];
        app.focus = Focus::Content;

        let text = rendered_text(40, 20, &mut app);
        assert!(
            text.contains("TAILMARKER"),
            "the end of a long line must be visible once wrapped, not truncated.\nBuffer:\n{text}"
        );
    }

    #[test]
    fn later_lines_stay_visible_alongside_a_wrapped_line_when_room_allows() {
        // A moderately long line pushes later lines down by however many
        // extra rows it needs (row-aware scroll math), but doesn't have to
        // evict them entirely as long as the viewport has room for both.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        let mut lines: Vec<crate::parser::ParsedLine> =
            (0..6).map(|i| plain_line(&format!("line{i}"))).collect();
        lines[2] = plain_line(&"x".repeat(60)); // wraps to a handful of rows
        app.content_lines = lines;
        app.focus = Focus::Content;

        let text = rendered_text(40, 20, &mut app);
        assert!(
            text.contains("line4") && text.contains("line5"),
            "later lines should remain visible when the viewport has room for the wrapped line too.\nBuffer:\n{text}"
        );
    }

    #[test]
    fn single_line_taller_than_viewport_renders_without_panicking() {
        // Pathological case: one line alone needs more rows than the whole
        // pane. There's no valid layout that shows it in full alongside
        // anything else -- the important thing is this doesn't panic and
        // still shows the start of the line, rather than the old
        // truncate-to-one-row behavior or a blank pane.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = vec![plain_line(&"y".repeat(500)), plain_line("after")];
        app.focus = Focus::Content;

        let text = rendered_text(30, 10, &mut app);
        assert!(
            text.contains("yyyyy"),
            "the start of an oversized line should still render.\nBuffer:\n{text}"
        );
    }

    #[test]
    fn cjk_text_wraps_by_display_width_not_char_count() {
        // Regression guard for Unicode-width-unaware wrapping: a run of
        // wide (2-column) CJK characters must wrap well before hitting the
        // pane's raw character-count width, and every character must still
        // be present (not silently dropped by a wrap that gets the column
        // math wrong).
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = vec![plain_line(&"あ".repeat(40))];
        app.focus = Focus::Content;

        let text = rendered_text(30, 20, &mut app);
        assert_eq!(
            text.matches('あ').count(),
            40,
            "every character of the wrapped CJK line should still be rendered.\nBuffer:\n{text}"
        );
    }

    #[test]
    fn collapsed_browser_pane_is_not_rendered() {
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items = vec![FileItem {
            path: PathBuf::from("visible-file.md"),
            name: "visible-file.md".to_string(),
            depth: 0,
            is_dir: false,
            is_expanded: false,
        }];
        app.browser_collapsed = true;

        let text = rendered_text(50, 10, &mut app);
        assert!(
            !text.contains("Files") && !text.contains("visible-file"),
            "a collapsed browser pane must not be drawn.\nBuffer:\n{text}"
        );
    }

    #[test]
    fn collapsed_browser_gives_content_the_full_width() {
        // A 40-column line doesn't fit the content pane's usual ~34-column
        // share of a 50-column terminal (would wrap to 2 rows -- 2 gutter
        // bars), but does fit the ~47 columns content gets once the browser
        // pane stops claiming its 25% share (1 row -- 1 gutter bar). Counting
        // the cursor-block gutter bar (see `cursor_gutter_bar_repeats_on_every_wrapped_row`
        // above) is a robust way to observe the row count without depending
        // on exactly where ratatui's buffer wraps the text.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = vec![plain_line(&"x".repeat(40))];
        app.focus = Focus::Content;
        app.content_cursor = 0;
        app.browser_collapsed = true;

        let text = rendered_text(50, 10, &mut app);
        assert_eq!(
            text.matches('▎').count(),
            1,
            "content should use the full pane width once the browser is collapsed, fitting the line on one row.\nBuffer:\n{text}"
        );
    }

    #[test]
    fn fenced_code_block_lines_render_on_separate_rows() {
        // Regression guard: `parser::parse_file` folds a whole fenced code
        // block into one `ParsedLine` whose `Segment::Code` text embeds real
        // '\n' bytes (fence + code + fence, joined with "\n"). Before
        // `wrap::wrap_row_ranges` treated '\n' as a hard break, it had zero
        // display width, so it never triggered a row break on its own -- a
        // code block narrow enough to fit `wrap_width` rendered as ONE row
        // with every original source line run together, instead of one row
        // per line like the rest of the file's content.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = vec![crate::parser::ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![crate::parser::Segment::Code(
                "```rust\nfn main() {\n    println!(\"hi\");\n}\n```".to_string(),
            )],
            ..Default::default()
        }];
        app.focus = Focus::Content;

        let width = 60;
        let backend = TestBackend::new(width, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        let row_text = |y: u16| -> String { (0..width).map(|x| buffer[(x, y)].symbol()).collect() };
        let rows: Vec<String> = (0..buffer.area.height).map(row_text).collect();

        let fn_main_row = rows.iter().position(|r| r.contains("fn main"));
        let println_row = rows.iter().position(|r| r.contains("println"));

        assert!(
            fn_main_row.is_some() && println_row.is_some(),
            "expected both code lines to render somewhere.\nBuffer:\n{}",
            rows.join("\n")
        );
        assert_ne!(
            fn_main_row,
            println_row,
            "the code block's separate source lines were merged onto the \
             same rendered row -- the embedded '\\n' failed to force a row \
             break.\nBuffer:\n{}",
            rows.join("\n")
        );
    }

    #[test]
    fn tab_indented_code_block_never_renders_a_raw_tab_cell() {
        // Regression for the corruption reported in the wild: Logseq
        // indents a block's continuation lines with tabs to match its own
        // outline nesting, so a fenced code block's raw source lines carry
        // that prefix (e.g. "\t\t\t  fn main() {}" under a block opened by
        // "\t\t\t- ```"). `ratatui::widgets::Paragraph` writes
        // `StyledGrapheme`s straight into buffer cells with no
        // control-character filtering, so a raw tab reaching a rendered
        // cell would reach the real terminal too -- where it jumps to the
        // next tab stop instead of advancing one column, desyncing the
        // terminal's actual cursor from what the app assumes.
        let content = "\t\t\t- ```rust\n\t\t\t  fn main() {}\n\t\t\t  ```";
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = crate::parser::parse_file(content);
        app.focus = Focus::Content;

        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let has_control_char = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.symbol().chars().any(|ch| ch.is_control()));

        assert!(
            !has_control_char,
            "a rendered cell contained a raw control character (e.g. a tab) \
             -- this reaches the real terminal as a cursor-moving control \
             code, not visible text"
        );
    }

    #[test]
    fn cursor_gutter_bar_repeats_on_every_wrapped_row() {
        // The cursor-block gutter bar is a per-row visual, not per-line: a
        // wrapped line inside the cursor block should show the bar on every
        // row it occupies, not just its first, so the block reads as one
        // continuous highlighted region.
        let mut app = App::new(PathBuf::new(), FakeGraphSource::new()).unwrap();
        app.file_items.clear();
        app.current_file = Some(PathBuf::from("test.md"));
        app.content_lines = vec![plain_line(&"z".repeat(60))];
        app.focus = Focus::Content;
        app.content_cursor = 0;

        let text = rendered_text(40, 20, &mut app);
        assert!(
            text.matches('▎').count() >= 2,
            "the gutter bar should repeat across the wrapped line's rows.\nBuffer:\n{text}"
        );
    }
}
