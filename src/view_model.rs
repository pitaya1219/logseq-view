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

/// Highlight state for a visible line during search
#[derive(Debug, Clone, PartialEq)]
pub enum LineHighlight {
    None,
    Match,
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
    pub search_active: bool,
    pub search_query: String,
}

/// Build a ViewModel from the App state and visible heights.
/// This is the presenter function that performs scroll clamping and slicing.
pub fn build_view_model<S: GraphSource>(
    app: &mut App<S>,
    browser_visible_height: usize,
    content_visible_height: usize,
) -> ViewModel {
    // Clamp and slice browser
    let browser_view = build_browser_view(app, browser_visible_height);

    // Clamp and slice content
    let content_view = build_content_view(app, content_visible_height);

    ViewModel {
        browser: browser_view,
        content: content_view,
        focus: app.focus,
        search_active: app.search_active,
        search_query: app.search_query.clone(),
    }
}

fn build_browser_view<S: GraphSource>(app: &mut App<S>, visible_height: usize) -> BrowserView {
    // Perform scroll clamping using the app's method
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
    // Perform scroll clamping using the app's method
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

    // Compute scrollbar info
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

    // Compute search-related info if there's an active search query
    let (match_count, current_match, line_highlights) = if app.search_query.is_empty() {
        (0, None, vec![LineHighlight::None; visible_lines.len()])
    } else {
        let match_indices = app.match_line_indices();
        let match_count = match_indices.len();

        // Find current match position (1-based)
        let current_match = app.current_match_position();

        // Get the absolute index of the current match (the line at content_scroll)
        // This is the line that should be highlighted as Current
        let current_match_line = if current_match.is_some() {
            // The line at content_scroll is the current match
            Some(app.content_scroll)
        } else {
            // content_scroll is not a match line, but we still want to highlight
            // the first visible match if any, or None
            None
        };

        // Build line highlights for visible lines
        let line_highlights: Vec<LineHighlight> = visible_lines
            .iter()
            .enumerate()
            .map(|(visible_idx, _line)| {
                let absolute_idx = app.content_scroll + visible_idx;
                if let Some(current_line) = current_match_line {
                    if absolute_idx == current_line {
                        LineHighlight::Current
                    } else if match_indices.contains(&absolute_idx) {
                        LineHighlight::Match
                    } else {
                        LineHighlight::None
                    }
                } else {
                    // No current match (content_scroll is not a match)
                    if match_indices.contains(&absolute_idx) {
                        LineHighlight::Match
                    } else {
                        LineHighlight::None
                    }
                }
            })
            .collect();

        (match_count, current_match, line_highlights)
    };

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
        // Create app with empty FakeGraphSource - children will return empty vec
        // which is fine for our tests
        let source = FakeGraphSource::new();
        let mut app = App::new(PathBuf::new(), source).unwrap();
        // Reset to clean state
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
            })
            .collect()
    }

    // --- Browser clamping and slicing tests ---

    #[test]
    fn browser_clamp_selected_before_offset() {
        let mut app = make_app();
        // Need at least 13 items to show 10 visible rows after offset clamping
        app.file_items = dummy_file_items(13);
        app.browser_offset = 5;
        app.browser_selected = 3;

        let vm = build_view_model(&mut app, 10, 0);

        assert_eq!(app.browser_offset, 3);
        assert_eq!(vm.browser.visible_rows.len(), 10);
        // Selected should be at index 0 (3 - 3 = 0)
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
        // Selected should be at index 4 (10 - 6 = 4)
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
        // Selected should be at index 2 (4 - 2 = 2)
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
        assert_eq!(scrollbar.total, 10); // 20 - 10 = 10
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
        // Note: In the real app, file names are already URL-decoded by the GraphSource.
        // The presenter still applies url_decode for safety (idempotent for already-decoded names).
        let mut app = make_app();
        // Simulate a file whose name was URL-encoded in the filesystem
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
        app.search_query = String::new();

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
        app.search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 3);
        // Current scroll is at line 0 which is a match, so current_match should be Some(1)
        assert_eq!(vm.content.current_match, Some(1));

        // All visible lines (5 lines) should have highlights
        assert_eq!(vm.content.line_highlights.len(), 5);
        // Line 0: Current match
        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current);
        // Line 1: No match
        assert_eq!(vm.content.line_highlights[1], LineHighlight::None);
        // Line 2: Match (but not current)
        assert_eq!(vm.content.line_highlights[2], LineHighlight::Match);
        // Line 3: Match (but not current)
        assert_eq!(vm.content.line_highlights[3], LineHighlight::Match);
        // Line 4: No match
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
        app.content_scroll = 1; // Scroll to line 1 which is NOT a match
        app.search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 2);
        // Current scroll is at line 1 which is NOT a match
        assert_eq!(vm.content.current_match, None);

        // visible_lines will be [1, 2] (2 lines) since content_scroll=1 and we have 3 lines
        assert_eq!(vm.content.line_highlights.len(), 2);
        // Line 0 (absolute index 1+0=1): "other line" is NOT a match
        assert_eq!(vm.content.line_highlights[0], LineHighlight::None);
        // Line 1 (absolute index 1+1=2): "target line 2" IS a match
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
        app.content_scroll = 2; // Scroll to line 2 which IS a match
        app.search_query = "target".to_string();

        let vm = build_view_model(&mut app, 0, 10);

        assert_eq!(vm.content.match_count, 3);
        // Current scroll is at line 2 which is the second match
        assert_eq!(vm.content.current_match, Some(2));

        // The first visible line is at index 2 which should be Current
        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current);
    }

    #[test]
    fn content_view_no_matches() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![make_line("line 1"), make_line("line 2")];
        app.content_scroll = 0;
        app.search_query = "zzz".to_string();

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
        app.search_query = "target".to_string();

        // Only show 3 lines at a time
        let vm = build_view_model(&mut app, 0, 3);

        assert_eq!(vm.content.match_count, 3);
        assert_eq!(vm.content.current_match, Some(1));
        assert_eq!(vm.content.visible_lines.len(), 3);
        assert_eq!(vm.content.line_highlights.len(), 3);

        // Line 0: Current match (target line 1)
        assert_eq!(vm.content.line_highlights[0], LineHighlight::Current);
        // Line 1: No match (other line 1)
        assert_eq!(vm.content.line_highlights[1], LineHighlight::None);
        // Line 2: Match (target line 2)
        assert_eq!(vm.content.line_highlights[2], LineHighlight::Match);
    }

    fn make_line(text: &str) -> ParsedLine {
        ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain(text.to_string())],
        }
    }

    fn dummy_lines_with_text(n: usize, _prefix: &str) -> Vec<ParsedLine> {
        (0..n)
            .map(|i| ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain(format!("line {}", i))],
            })
            .collect()
    }
}
