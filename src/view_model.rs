use crate::app::{App, Focus};
use crate::parser::ParsedLine;
use crate::source::{url_decode, GraphSource};

/// A single visible browser row for rendering
#[derive(Debug, Clone)]
pub struct BrowserRow {
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
    pub name: String,
    pub is_selected: bool,
}

/// Scrollbar state for content area
#[derive(Debug, Clone)]
pub struct ScrollbarInfo {
    pub total: usize,
    pub position: usize,
}

/// Highlight state for a visible line: search results and/or the block
/// cursor.
///
/// Priority when more than one would apply to the same line (highest
/// first): `Current` (the active search match) > `Cursor` (the block the
/// content cursor is on) > `Match` (other search matches) > `None`. Search's
/// "you are here" wins over the cursor block since it answers a more
/// specific question ("where's my match"), but the cursor block still shows
/// through on any of its lines that are merely one of several matches.
#[derive(Debug, Clone, PartialEq)]
pub enum LineHighlight {
    None,
    Match,
    Cursor,
    Current,
}

/// ViewModel for the content area
#[derive(Debug, Clone)]
pub struct ContentView {
    pub title: String,
    pub visible_lines: Vec<ParsedLine>,
    pub focused: bool,
    pub scrollbar: Option<ScrollbarInfo>,
    pub no_file_loaded: bool,
    pub match_count: usize,
    pub current_match: Option<usize>,
    pub line_highlights: Vec<LineHighlight>,
}

/// ViewModel for the browser area
#[derive(Debug, Clone)]
pub struct BrowserView {
    pub visible_rows: Vec<BrowserRow>,
    pub focused: bool,
}

/// Complete ViewModel holding all data needed for rendering
#[derive(Debug, Clone)]
pub struct ViewModel {
    pub browser: BrowserView,
    pub content: ContentView,
    pub focus: Focus,
    // Content search state (for status bar and highlight)
    pub content_search_active: bool,
    pub content_search_query: String,
    // Browser search state (for status bar)
    pub browser_search_active: bool,
    pub browser_search_query: String,
}

/// Build a ViewModel from the App state and visible heights.
pub fn build_view_model<S: GraphSource>(
    app: &mut App<S>,
    browser_visible_height: usize,
    content_visible_height: usize,
) -> ViewModel {
    let browser_view = build_browser_view(app, browser_visible_height);
    let content_view = build_content_view(app, content_visible_height);

    ViewModel {
        browser: browser_view,
        content: content_view,
        focus: app.focus,
        content_search_active: app.content_search_active,
        content_search_query: app.content_search_query.clone(),
        browser_search_active: app.browser_search_active,
        browser_search_query: app.browser_search_query.clone(),
    }
}

fn build_browser_view<S: GraphSource>(app: &mut App<S>, visible_height: usize) -> BrowserView {
    app.clamp_browser_scroll(visible_height);

    let visible_rows: Vec<BrowserRow> = app
        .file_items
        .iter()
        .skip(app.browser_offset)
        .take(visible_height)
        .enumerate()
        .map(|(i, item)| {
            let abs_idx = i + app.browser_offset;
            BrowserRow {
                depth: item.depth,
                is_dir: item.is_dir,
                is_expanded: item.is_expanded,
                name: item.name.clone(),
                is_selected: abs_idx == app.browser_selected,
            }
        })
        .collect();

    BrowserView {
        visible_rows,
        focused: app.focus == Focus::Browser,
    }
}

fn build_content_view<S: GraphSource>(app: &mut App<S>, visible_height: usize) -> ContentView {
    // The block cursor only drives auto-scroll while content is the focused
    // pane. This matters beyond cosmetics: `content_scroll` is also driven
    // independently by content search (jumping to a match line), and
    // letting an unrelated stale `content_cursor` fight that on every
    // render would undo the search jump. Content search keeps
    // `content_cursor` in sync with `content_scroll` itself (see
    // `content_search_commit` etc.), so gating on focus here is enough to
    // keep the two mechanisms from stepping on each other.
    if app.focus == Focus::Content {
        app.clamp_content_cursor_scroll(visible_height);
    }
    app.clamp_content_scroll(visible_height);

    let title = app
        .current_file
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| format!(" {} ", url_decode(&s.to_string_lossy())))
        .unwrap_or_else(|| " (no file) ".to_string());

    let no_file_loaded = app.current_file.is_none();

    let visible_lines: Vec<ParsedLine> = if no_file_loaded {
        Vec::new()
    } else {
        app.content_lines
            .iter()
            .skip(app.content_scroll)
            .take(visible_height)
            .cloned()
            .collect()
    };

    let scrollbar = if no_file_loaded {
        None
    } else {
        let total = app.content_lines.len();
        if total > visible_height {
            Some(ScrollbarInfo {
                total: total.saturating_sub(visible_height),
                position: app.content_scroll,
            })
        } else {
            None
        }
    };

    // Compute search highlight info using the content search query
    let match_indices = if app.content_search_query.is_empty() {
        Vec::new()
    } else {
        app.match_line_indices()
    };
    let match_count = match_indices.len();
    let current_match = if app.content_search_query.is_empty() {
        None
    } else {
        app.current_match_position()
    };
    let current_match_line = if current_match.is_some() {
        Some(app.content_scroll)
    } else {
        None
    };

    // The cursor block only lights up while content is focused (see the
    // comment above `clamp_content_cursor_scroll`'s call site) — an empty
    // range means no line ever falls inside [start, end).
    let cursor_range = if !no_file_loaded && app.focus == Focus::Content {
        crate::app::block_range_at(&app.content_lines, app.content_cursor)
    } else {
        (0, 0)
    };

    let line_highlights: Vec<LineHighlight> = visible_lines
        .iter()
        .enumerate()
        .map(|(visible_idx, _line)| {
            let absolute_idx = app.content_scroll + visible_idx;
            if Some(absolute_idx) == current_match_line {
                LineHighlight::Current
            } else if absolute_idx >= cursor_range.0 && absolute_idx < cursor_range.1 {
                LineHighlight::Cursor
            } else if match_indices.contains(&absolute_idx) {
                LineHighlight::Match
            } else {
                LineHighlight::None
            }
        })
        .collect();

    ContentView {
        title,
        visible_lines,
        focused: app.focus == Focus::Content,
        scrollbar,
        no_file_loaded,
        match_count,
        current_match,
        line_highlights,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::FileItem;
    use crate::parser::{ParsedLine, Segment};
    use crate::source::FakeGraphSource;
    use std::path::PathBuf;

    fn make_app() -> App<FakeGraphSource> {
        let source = FakeGraphSource::new();
        let mut app = App::new(PathBuf::new(), source).unwrap();
        app.file_items.clear();
        app.browser_selected = 0;
        app.browser_offset = 0;
        app.current_file = None;
        app.content_lines.clear();
        app.content_scroll = 0;
        app.focus = Focus::Browser;
        app
    }

    fn dummy_file_items(count: usize) -> Vec<FileItem> {
        (0..count)
            .map(|i| FileItem {
                path: PathBuf::from(format!("item{}", i)),
                name: format!("item{}", i),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            })
            .collect()
    }

    fn dummy_lines(n: usize) -> Vec<ParsedLine> {
        (0..n)
            .map(|_| ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: Vec::new(),
                ..Default::default()
            })
            .collect()
    }

    // --- Browser clamping and slicing tests ---

    #[test]
    fn browser_clamp_selected_before_offset() {
        let mut app = make_app();
        app.file_items = dummy_file_items(13);
        app.browser_offset = 5;
        app.browser_selected = 3;

        let vm = build_view_model(&mut app, 10, 0);

        assert_eq!(app.browser_offset, 3);
        assert_eq!(vm.browser.visible_rows.len(), 10);
        assert!(vm.browser.visible_rows[0].is_selected);
    }

    #[test]
    fn browser_clamp_selected_past_window() {
        let mut app = make_app();
        app.file_items = dummy_file_items(15);
        app.browser_offset = 0;
        app.browser_selected = 10;

        let vm = build_view_model(&mut app, 5, 0);

        assert_eq!(app.browser_offset, 6);
        assert_eq!(vm.browser.visible_rows.len(), 5);
        assert!(vm.browser.visible_rows[4].is_selected);
    }

    #[test]
    fn browser_selected_within_window_unchanged() {
        let mut app = make_app();
        app.file_items = dummy_file_items(12);
        app.browser_offset = 2;
        app.browser_selected = 4;

        let vm = build_view_model(&mut app, 10, 0);

        assert_eq!(app.browser_offset, 2);
        assert_eq!(vm.browser.visible_rows.len(), 10);
        assert!(!vm.browser.visible_rows[0].is_selected);
        assert!(!vm.browser.visible_rows[1].is_selected);
        assert!(vm.browser.visible_rows[2].is_selected);
    }

    #[test]
    fn browser_visible_rows_contains_correct_data() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "file_a".to_string(),
                depth: 1,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "file_b".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_offset = 0;
        app.browser_selected = 1;

        let vm = build_view_model(&mut app, 10, 0);

        assert_eq!(vm.browser.visible_rows.len(), 2);
        assert_eq!(vm.browser.visible_rows[0].name, "file_a");
        assert_eq!(vm.browser.visible_rows[0].depth, 1);
        assert!(vm.browser.visible_rows[0].is_dir);
        assert!(vm.browser.visible_rows[0].is_expanded);
        assert!(!vm.browser.visible_rows[0].is_selected);

        assert_eq!(vm.browser.visible_rows[1].name, "file_b");
        assert_eq!(vm.browser.visible_rows[1].depth, 0);
        assert!(!vm.browser.visible_rows[1].is_dir);
        assert!(!vm.browser.visible_rows[1].is_expanded);
        assert!(vm.browser.visible_rows[1].is_selected);
    }

    #[test]
    fn browser_focused_flag() {
        let mut app = make_app();
        app.focus = Focus::Browser;

        let vm = build_view_model(&mut app, 10, 0);
        assert!(vm.browser.focused);
        assert!(!vm.content.focused);

        app.focus = Focus::Content;
        let vm = build_view_model(&mut app, 10, 0);
        assert!(!vm.browser.focused);
        assert!(vm.content.focused);
    }

    // --- Content clamping and slicing tests ---

    #[test]
    fn content_scroll_clamped_when_past_end() {
        let mut app = make_app();
        app.content_lines = dummy_lines(20);
        app.content_scroll = 15;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(app.content_scroll, 10);
        assert_eq!(vm.content.visible_lines.len(), 10);
    }

    #[test]
    fn content_scroll_unchanged_when_all_lines_fit() {
        let mut app = make_app();
        app.content_lines = dummy_lines(5);
        app.content_scroll = 0;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(app.content_scroll, 0);
        assert_eq!(vm.content.visible_lines.len(), 5);
    }

    #[test]
    fn content_scroll_already_at_end_unchanged() {
        let mut app = make_app();
        app.content_lines = dummy_lines(20);
        app.content_scroll = 10;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(app.content_scroll, 10);
        assert_eq!(vm.content.visible_lines.len(), 10);
    }

    #[test]
    fn content_scrollbar_present_when_needed() {
        let mut app = make_app();
        app.content_lines = dummy_lines(20);
        app.content_scroll = 5;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let vm = build_view_model(&mut app, 0, 10);

        assert!(vm.content.scrollbar.is_some());
        let scrollbar = vm.content.scrollbar.unwrap();
        assert_eq!(scrollbar.total, 10);
        assert_eq!(scrollbar.position, 5);
    }

    #[test]
    fn content_scrollbar_absent_when_not_needed() {
        let mut app = make_app();
        app.content_lines = dummy_lines(5);
        app.content_scroll = 0;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let vm = build_view_model(&mut app, 0, 10);

        assert!(vm.content.scrollbar.is_none());
    }

    #[test]
    fn content_no_file_loaded() {
        let mut app = make_app();
        app.current_file = None;
        app.content_lines = Vec::new();

        let vm = build_view_model(&mut app, 0, 10);

        assert!(vm.content.no_file_loaded);
        assert_eq!(vm.content.title, " (no file) ");
        assert!(vm.content.visible_lines.is_empty());
    }

    #[test]
    fn content_title_from_file() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/path/to/file.md"));
        app.content_lines = Vec::new();

        let vm = build_view_model(&mut app, 0, 10);

        assert!(!vm.content.no_file_loaded);
        assert_eq!(vm.content.title, " file ");
    }

    #[test]
    fn content_title_url_decoded() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/path/to/encoded name.md"));
        app.content_lines = Vec::new();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.title, " encoded name ");
    }

    #[test]
    fn view_model_focus_field() {
        let mut app = make_app();
        app.focus = Focus::Browser;

        let vm = build_view_model(&mut app, 10, 10);
        assert_eq!(vm.focus, Focus::Browser);

        app.focus = Focus::Content;
        let vm = build_view_model(&mut app, 10, 10);
        assert_eq!(vm.focus, Focus::Content);
    }

    // --- Search highlight tests ---

    #[test]
    fn content_view_no_search_no_highlights() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = dummy_lines_with_text(5, "line");
        app.content_scroll = 0;
        app.content_search_query = String::new();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 0);
        assert_eq!(vm.content.current_match, None);
        assert_eq!(vm.content.line_highlights.len(), 5);
        assert!(vm
            .content
            .line_highlights
            .iter()
            .all(|h| matches!(h, LineHighlight::None)));
    }

    #[test]
    fn content_view_search_highlights_matches() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line("target line 1"),
            make_line("other line"),
            make_line("target line 2"),
            make_line("target line 3"),
            make_line("no match"),
        ];
        app.content_scroll = 0;
        app.content_search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 3);
        assert_eq!(vm.content.current_match, Some(1));

        assert_eq!(vm.content.line_highlights.len(), 5);
        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current);
        assert_eq!(vm.content.line_highlights[1], LineHighlight::None);
        assert_eq!(vm.content.line_highlights[2], LineHighlight::Match);
        assert_eq!(vm.content.line_highlights[3], LineHighlight::Match);
        assert_eq!(vm.content.line_highlights[4], LineHighlight::None);
    }

    #[test]
    fn content_view_current_match_not_on_match() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line("target line 1"),
            make_line("other line"),
            make_line("target line 2"),
        ];
        app.content_scroll = 1;
        app.content_search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 2);
        assert_eq!(vm.content.current_match, None);

        assert_eq!(vm.content.line_highlights.len(), 2);
        assert_eq!(vm.content.line_highlights[0], LineHighlight::None);
        assert_eq!(vm.content.line_highlights[1], LineHighlight::Match);
    }

    #[test]
    fn content_view_scroll_to_second_match() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line("target line 1"),
            make_line("other line"),
            make_line("target line 2"),
            make_line("target line 3"),
        ];
        app.content_scroll = 2;
        app.content_search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 3);
        assert_eq!(vm.content.current_match, Some(2));
        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current);
    }

    #[test]
    fn content_view_no_matches() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![make_line("line 1"), make_line("line 2")];
        app.content_scroll = 0;
        app.content_search_query = "zzz".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 0);
        assert_eq!(vm.content.current_match, None);
        assert_eq!(vm.content.line_highlights.len(), 2);
        assert!(vm
            .content
            .line_highlights
            .iter()
            .all(|h| matches!(h, LineHighlight::None)));
    }

    #[test]
    fn content_view_partial_visibility() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line("target line 1"),
            make_line("other line 1"),
            make_line("target line 2"),
            make_line("other line 2"),
            make_line("target line 3"),
        ];
        app.content_scroll = 0;
        app.content_search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 3);

        assert_eq!(vm.content.match_count, 3);
        assert_eq!(vm.content.current_match, Some(1));
        assert_eq!(vm.content.visible_lines.len(), 3);
        assert_eq!(vm.content.line_highlights.len(), 3);

        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current);
        assert_eq!(vm.content.line_highlights[1], LineHighlight::None);
        assert_eq!(vm.content.line_highlights[2], LineHighlight::Match);
    }

    // --- Block cursor highlight tests ---

    #[test]
    fn content_view_cursor_highlights_whole_block_when_focused() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line_indent("A", 0),
            make_line_indent("A1", 1),
            make_line_indent("A2", 1),
            make_line_indent("B", 0),
        ];
        app.content_cursor = 0;

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.line_highlights[0], LineHighlight::Cursor);
        assert_eq!(vm.content.line_highlights[1], LineHighlight::Cursor);
        assert_eq!(vm.content.line_highlights[2], LineHighlight::Cursor);
        assert_eq!(vm.content.line_highlights[3], LineHighlight::None);
    }

    #[test]
    fn content_view_cursor_on_leaf_only_highlights_itself() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line_indent("A", 0),
            make_line_indent("A1", 1),
            make_line_indent("A2", 1),
        ];
        app.content_cursor = 1;

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.line_highlights[0], LineHighlight::None);
        assert_eq!(vm.content.line_highlights[1], LineHighlight::Cursor);
        assert_eq!(vm.content.line_highlights[2], LineHighlight::None);
    }

    #[test]
    fn content_view_no_cursor_highlight_when_browser_focused() {
        // Cursor highlighting is gated on content being the focused pane, so
        // it doesn't fight `content_scroll` jumps driven by other
        // mechanisms (e.g. content search) while the content pane isn't
        // even the active one.
        let mut app = make_app();
        app.focus = Focus::Browser;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![make_line_indent("A", 0), make_line_indent("A1", 1)];
        app.content_cursor = 0;

        let vm = build_view_model(&mut app, 0, 10);

        assert!(vm
            .content
            .line_highlights
            .iter()
            .all(|h| matches!(h, LineHighlight::None)));
    }

    #[test]
    fn content_view_search_current_wins_over_cursor() {
        // Priority: Current (the active search match) beats Cursor even
        // when the matched line is itself inside the cursor's block.
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line_indent("A", 0),
            make_line_indent("target", 1),
            make_line_indent("A2", 1),
        ];
        // Cursor sits on the match line itself (leaf block: line 1 has no
        // deeper-indented children, so its block is just itself). This
        // keeps content_scroll==content_cursor, avoiding the render-time
        // cursor-follow clamp pulling the scroll away from the match — in
        // real usage content_search_commit keeps the two in sync the same
        // way.
        app.content_cursor = 1;
        app.content_scroll = 1; // the active search match is line 1
        app.content_search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        // visible_lines starts at content_scroll (1): index 0 -> line 1, index 1 -> line 2
        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current); // Current beats Cursor
        assert_eq!(vm.content.line_highlights[1], LineHighlight::None); // outside the (leaf) block
    }

    #[test]
    fn content_view_cursor_wins_over_other_search_match() {
        // Priority: Cursor beats Match (a search hit that is NOT the
        // current one) when both would apply to the same line.
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            make_line_indent("A", 0),
            make_line_indent("target child", 1),
            make_line_indent("other", 0),
            make_line_indent("target C", 0),
        ];
        app.content_cursor = 0; // block = lines 0..2
        app.content_scroll = 0; // line 0 isn't a match, so there is no "current" match line
        app.content_search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.current_match, None);
        // Line 1 matches the search query AND is inside the cursor block:
        // Cursor wins over the plain Match highlight.
        assert_eq!(vm.content.line_highlights[1], LineHighlight::Cursor);
        // Line 3 matches too but is outside the block: plain Match still applies.
        assert_eq!(vm.content.line_highlights[3], LineHighlight::Match);
    }

    fn make_line_indent(text: &str, indent: usize) -> ParsedLine {
        ParsedLine {
            indent,
            is_bullet: true,
            task: None,
            segments: vec![Segment::Plain(text.to_string())],
            ..Default::default()
        }
    }

    fn make_line(text: &str) -> ParsedLine {
        ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain(text.to_string())],
            ..Default::default()
        }
    }

    fn dummy_lines_with_text(n: usize, _prefix: &str) -> Vec<ParsedLine> {
        (0..n)
            .map(|i| ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain(format!("line {}", i))],
                ..Default::default()
            })
            .collect()
    }
}
