use crate::action::Action;
use crate::parser::{line_to_plain_text, parse_file, ParsedLine};
use crate::source::{Entry, GraphSource};
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Focus {
    Browser,
    Content,
}

#[derive(Debug, Clone)]
pub struct FileItem {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub is_expanded: bool,
}

pub struct App<S: GraphSource> {
    pub graph_path: PathBuf,
    pub focus: Focus,

    // file browser
    pub file_items: Vec<FileItem>,
    pub browser_selected: usize,
    pub browser_offset: usize,

    // content
    pub current_file: Option<PathBuf>,
    pub content_lines: Vec<ParsedLine>,
    pub content_scroll: usize,

    // search state
    pub search_active: bool,
    pub search_query: String,
    pub search_saved_scroll: usize,

    // GraphSource instance
    source: S,
}

impl<S: GraphSource> App<S> {
    pub fn new(graph_path: PathBuf, source: S) -> Result<Self> {
        let mut app = App {
            graph_path,
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
            search_active: false,
            search_query: String::new(),
            search_saved_scroll: 0,
            source,
        };
        app.build_file_tree()?;
        Ok(app)
    }

    pub(crate) fn build_file_tree(&mut self) -> Result<()> {
        self.file_items.clear();

        // Load only immediate children of the graph root; expand on demand
        let entries = self.source.children(&self.graph_path)?;
        for entry in entries {
            let item = make_file_item_from_entry(entry, 0);
            self.file_items.push(item);
        }

        Ok(())
    }

    pub(crate) fn open_selected(&mut self) -> Result<()> {
        let Some(item) = self.file_items.get(self.browser_selected) else {
            return Ok(());
        };

        if item.is_dir {
            // toggle expand/collapse
            let path = item.path.clone();
            let idx = self.browser_selected;
            let is_expanded = self.file_items[idx].is_expanded;
            self.file_items[idx].is_expanded = !is_expanded;

            if is_expanded {
                // collapse: remove children
                self.collapse_dir(idx);
            } else {
                // expand: insert children
                self.expand_dir(idx, &path)?;
            }
        } else {
            // open file
            let path = item.path.clone();
            self.load_file(&path)?;
            self.focus = Focus::Content;
        }

        Ok(())
    }

    pub(crate) fn collapse_dir(&mut self, idx: usize) {
        let depth = self.file_items[idx].depth;
        let mut end = idx + 1;
        while end < self.file_items.len() && self.file_items[end].depth > depth {
            end += 1;
        }
        self.file_items.drain(idx + 1..end);
    }

    pub(crate) fn expand_dir(&mut self, parent_idx: usize, dir: &Path) -> Result<()> {
        let parent_depth = self.file_items[parent_idx].depth;
        let entries = self.source.children(dir)?;

        let insert_at = parent_idx + 1;
        for (i, entry) in entries.into_iter().enumerate() {
            let item = make_file_item_from_entry(entry, parent_depth + 1);
            self.file_items.insert(insert_at + i, item);
        }

        Ok(())
    }

    pub(crate) fn load_file(&mut self, path: &Path) -> Result<()> {
        let content = self.source.read(path)?;
        self.content_lines = parse_file(&content);
        self.current_file = Some(path.to_path_buf());
        self.content_scroll = 0;
        Ok(())
    }

    /// Main update function that handles all actions.
    /// Returns Ok(true) if the application should quit, Ok(false) otherwise.
    pub fn update(&mut self, action: Action) -> Result<bool> {
        match action {
            Action::Quit => Ok(true),
            Action::ToggleFocus => {
                self.toggle_focus();
                Ok(false)
            }
            Action::BrowserUp => {
                self.browser_up();
                Ok(false)
            }
            Action::BrowserDown => {
                self.browser_down();
                Ok(false)
            }
            Action::OpenSelected => {
                self.open_selected()?;
                Ok(false)
            }
            Action::CollapseOrParent => {
                self.collapse_or_jump_parent();
                Ok(false)
            }
            Action::ContentUp(amount) => {
                self.content_up(amount);
                Ok(false)
            }
            Action::ContentDown(amount) => {
                self.content_down(amount);
                Ok(false)
            }
            Action::ContentTop => {
                self.content_top();
                Ok(false)
            }
            Action::ContentBottom => {
                self.content_bottom();
                Ok(false)
            }
            Action::BrowserTop => {
                self.browser_top();
                Ok(false)
            }
            Action::BrowserBottom => {
                self.browser_bottom();
                Ok(false)
            }
            // Search actions
            Action::SearchStart => {
                self.search_start();
                Ok(false)
            }
            Action::SearchInput(c) => {
                self.search_input(c);
                Ok(false)
            }
            Action::SearchBackspace => {
                self.search_backspace();
                Ok(false)
            }
            Action::SearchCommit => {
                self.search_commit();
                Ok(false)
            }
            Action::SearchCancel => {
                self.search_cancel();
                Ok(false)
            }
            Action::SearchNext => {
                self.search_next();
                Ok(false)
            }
            Action::SearchPrev => {
                self.search_prev();
                Ok(false)
            }
        }
    }

    // --- Navigation ---

    pub(crate) fn collapse_or_jump_parent(&mut self) {
        let Some(item) = self.file_items.get(self.browser_selected) else {
            return;
        };

        if item.is_dir && item.is_expanded {
            // collapse this directory
            let idx = self.browser_selected;
            self.file_items[idx].is_expanded = false;
            self.collapse_dir(idx);
        } else {
            // jump to parent directory
            let depth = item.depth;
            if depth == 0 {
                return;
            }
            let idx = self.browser_selected;
            for i in (0..idx).rev() {
                if self.file_items[i].depth < depth {
                    self.browser_selected = i;
                    break;
                }
            }
        }
    }

    pub(crate) fn browser_down(&mut self) {
        if self.browser_selected + 1 < self.file_items.len() {
            self.browser_selected += 1;
        }
    }

    pub(crate) fn browser_up(&mut self) {
        if self.browser_selected > 0 {
            self.browser_selected -= 1;
        }
    }

    pub(crate) fn content_down(&mut self, amount: usize) {
        let max = self.content_lines.len().saturating_sub(1);
        self.content_scroll = (self.content_scroll + amount).min(max);
    }

    pub(crate) fn content_up(&mut self, amount: usize) {
        self.content_scroll = self.content_scroll.saturating_sub(amount);
    }

    pub(crate) fn content_top(&mut self) {
        self.content_scroll = 0;
    }

    pub(crate) fn content_bottom(&mut self) {
        self.content_scroll = self.content_lines.len().saturating_sub(1);
    }

    /// Jump to the top of the current directory scope in the browser.
    /// If at depth 0, goes to index 0 (first item in the whole list).
    /// If inside a directory (depth > 0), goes to the first child of the parent directory.
    pub(crate) fn browser_top(&mut self) {
        let selected_depth = self.file_items[self.browser_selected].depth;

        if selected_depth == 0 {
            // At top level: go to first item
            self.browser_selected = 0;
        } else {
            // Inside a directory: find the parent and go to its first child
            let parent_idx = self.find_parent_index(self.browser_selected);
            if parent_idx + 1 < self.file_items.len() {
                self.browser_selected = parent_idx + 1;
            }
        }
    }

    /// Jump to the bottom of the current directory scope in the browser.
    /// If at depth 0, goes to the last item in the whole list.
    /// If inside a directory (depth > 0), goes to the last item of the parent's subtree.
    pub(crate) fn browser_bottom(&mut self) {
        let selected_depth = self.file_items[self.browser_selected].depth;

        if selected_depth == 0 {
            // At top level: go to last item
            if !self.file_items.is_empty() {
                self.browser_selected = self.file_items.len() - 1;
            }
        } else {
            // Inside a directory: find the parent and go to the last item in its subtree
            let parent_idx = self.find_parent_index(self.browser_selected);
            let parent_depth = self.file_items[parent_idx].depth;

            // Find the end of the parent's subtree
            let end = self.find_subtree_end(parent_idx, parent_depth);
            if end > parent_idx + 1 {
                self.browser_selected = end - 1;
            }
        }
    }

    /// Find the index of the parent directory for the item at idx.
    /// Returns the index of the nearest preceding item with depth < current depth.
    fn find_parent_index(&self, idx: usize) -> usize {
        let depth = self.file_items[idx].depth;
        for i in (0..idx).rev() {
            if self.file_items[i].depth < depth {
                return i;
            }
        }
        // Should not happen if depth > 0 and tree is valid, but fallback to 0
        0
    }

    /// Find the end index (exclusive) of the subtree starting at parent_idx with parent_depth.
    /// Returns the first index after parent_idx where depth <= parent_depth.
    fn find_subtree_end(&self, parent_idx: usize, parent_depth: usize) -> usize {
        let mut end = parent_idx + 1;
        while end < self.file_items.len() && self.file_items[end].depth > parent_depth {
            end += 1;
        }
        end
    }

    // --- Search methods ---

    /// Start a new search, saving the current scroll position
    pub(crate) fn search_start(&mut self) {
        if self.focus == Focus::Content && self.current_file.is_some() {
            self.search_active = true;
            self.search_query.clear();
            self.search_saved_scroll = self.content_scroll;
        }
    }

    /// Add a character to the search query
    pub(crate) fn search_input(&mut self, c: char) {
        if self.search_active {
            self.search_query.push(c);
        }
    }

    /// Remove the last character from the search query
    pub(crate) fn search_backspace(&mut self) {
        if self.search_active {
            self.search_query.pop();
        }
    }

    /// Commit the search - find the first matching line and scroll to it
    pub(crate) fn search_commit(&mut self) {
        if !self.search_active || self.search_query.is_empty() {
            self.search_cancel();
            return;
        }

        if let Some(matching_line) = self.find_next_match(self.content_scroll, true) {
            self.content_scroll = matching_line;
            self.search_active = false;
        }
        // If no match found, stay in search mode
    }

    /// Cancel search and restore the saved scroll position
    pub(crate) fn search_cancel(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.content_scroll = self.search_saved_scroll;
    }

    /// Find the next matching line (wrapping around)
    pub(crate) fn search_next(&mut self) {
        if !self.search_active && self.search_query.is_empty() {
            return;
        }

        let start_pos = if self.search_active {
            // In search mode, start from current scroll position
            self.content_scroll
        } else {
            // After commit, start from current scroll position + 1
            self.content_scroll + 1
        };

        if let Some(matching_line) = self.find_next_match(start_pos, false) {
            self.content_scroll = matching_line;
            if self.search_active {
                // In search mode, commit implicitly by exiting search mode
                self.search_active = false;
            }
        }
    }

    /// Find the previous matching line (wrapping around)
    pub(crate) fn search_prev(&mut self) {
        if !self.search_active && self.search_query.is_empty() {
            return;
        }

        let start_pos = if self.search_active {
            // In search mode, start from current scroll position
            self.content_scroll
        } else {
            // After commit, start from current scroll position
            self.content_scroll
        };

        if let Some(matching_line) = self.find_prev_match(start_pos, false) {
            self.content_scroll = matching_line;
            if self.search_active {
                // In search mode, commit implicitly by exiting search mode
                self.search_active = false;
            }
        }
    }

    /// Find the next line that matches the search query, starting from start_pos
    /// If wrap is true and no match found after end, search from beginning
    fn find_next_match(&self, start_pos: usize, wrap: bool) -> Option<usize> {
        if self.search_query.is_empty() || self.content_lines.is_empty() {
            return None;
        }

        let query = self.search_query.to_lowercase();
        let total_lines = self.content_lines.len();

        // Search from start_pos to end
        for i in start_pos..total_lines {
            let line_text = line_to_plain_text(&self.content_lines[i]).to_lowercase();
            if line_text.contains(&query) {
                return Some(i);
            }
        }

        // Search from beginning to start_pos (excluding start_pos)
        if wrap {
            for i in 0..start_pos {
                let line_text = line_to_plain_text(&self.content_lines[i]).to_lowercase();
                if line_text.contains(&query) {
                    return Some(i);
                }
            }
        }

        None
    }

    /// Find the previous line that matches the search query, starting from start_pos
    /// If wrap is true and no match found before 0, search from end
    fn find_prev_match(&self, start_pos: usize, wrap: bool) -> Option<usize> {
        if self.search_query.is_empty() || self.content_lines.is_empty() {
            return None;
        }

        let query = self.search_query.to_lowercase();
        let total_lines = self.content_lines.len();

        // Search backwards from start_pos to 0 (excluding start_pos)
        // We need to go in reverse order: start_pos-1, start_pos-2, ..., 0
        if start_pos > 0 {
            for i in (0..start_pos).rev() {
                let line_text = line_to_plain_text(&self.content_lines[i]).to_lowercase();
                if line_text.contains(&query) {
                    return Some(i);
                }
            }
        }

        // Search from end to start_pos
        if wrap {
            for i in (start_pos..total_lines).rev() {
                let line_text = line_to_plain_text(&self.content_lines[i]).to_lowercase();
                if line_text.contains(&query) {
                    return Some(i);
                }
            }
        }

        None
    }

    /// Returns a vector of all line indices that match the current search query.
    /// Case-insensitive matching using line_to_plain_text.
    pub fn match_line_indices(&self) -> Vec<usize> {
        if self.search_query.is_empty() || self.content_lines.is_empty() {
            return Vec::new();
        }

        let query = self.search_query.to_lowercase();
        self.content_lines
            .iter()
            .enumerate()
            .filter_map(|(i, line)| {
                let line_text = line_to_plain_text(line).to_lowercase();
                if line_text.contains(&query) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns the total number of matching lines for the current search query
    pub fn match_count(&self) -> usize {
        self.match_line_indices().len()
    }

    /// Returns the 1-based position of the current match (line at content_scroll)
    /// among all matching lines. Returns None if the current line is not a match
    /// or if there are no matches.
    pub fn current_match_position(&self) -> Option<usize> {
        let matches = self.match_line_indices();
        if matches.is_empty() {
            return None;
        }
        // Find the position of content_scroll in the matches list
        matches
            .iter()
            .position(|&idx| idx == self.content_scroll)
            .map(|pos| pos + 1) // Convert to 1-based
    }

    // Adjusts browser_offset so that browser_selected stays within the visible window.
    pub(crate) fn clamp_browser_scroll(&mut self, visible_height: usize) {
        if self.browser_selected < self.browser_offset {
            self.browser_offset = self.browser_selected;
        } else if self.browser_selected >= self.browser_offset + visible_height {
            self.browser_offset = self.browser_selected + 1 - visible_height;
        }
    }

    pub(crate) fn clamp_content_scroll(&mut self, visible_height: usize) {
        let total = self.content_lines.len();
        if self.content_scroll + visible_height > total && total > visible_height {
            self.content_scroll = total - visible_height;
        }
    }

    pub(crate) fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Content,
            Focus::Content => Focus::Browser,
        };
    }
}

/// Create a FileItem from a GraphSource Entry with the given depth
fn make_file_item_from_entry(entry: Entry, depth: usize) -> FileItem {
    FileItem {
        path: entry.path,
        name: entry.name,
        depth,
        is_dir: entry.is_dir,
        is_expanded: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Segment;
    use crate::source::{url_decode, FakeGraphSource};
    use std::path::PathBuf;

    fn make_app() -> App<FakeGraphSource> {
        App {
            graph_path: PathBuf::new(),
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
            search_active: false,
            search_query: String::new(),
            search_saved_scroll: 0,
            source: FakeGraphSource::new(),
        }
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

    // --- url_decode tests ---

    #[test]
    fn url_decode_percent_encoded_space() {
        assert_eq!(url_decode("hello%20world"), "hello world");
    }

    #[test]
    fn url_decode_invalid_percent_left_as_is() {
        assert_eq!(url_decode("hello%2"), "hello%2");
        assert_eq!(url_decode("hello%ZZ"), "hello%ZZ");
    }

    #[test]
    fn url_decode_utf8_multibyte() {
        // %E3%81%82 is the UTF-8 encoding for あ
        assert_eq!(url_decode("%E3%81%82"), "あ");
    }

    #[test]
    fn url_decode_mixed_encoded_and_plain() {
        assert_eq!(
            url_decode("file%20name%20with%20spaces.txt"),
            "file name with spaces.txt"
        );
    }

    // --- clamp_browser_scroll ---

    #[test]
    fn browser_scroll_selected_before_offset_clamps_up() {
        let mut app = make_app();
        app.browser_offset = 5;
        app.browser_selected = 3;
        app.clamp_browser_scroll(10);
        assert_eq!(app.browser_offset, 3);
    }

    #[test]
    fn browser_scroll_selected_past_window_slides_down() {
        let mut app = make_app();
        app.browser_offset = 0;
        app.browser_selected = 10;
        app.clamp_browser_scroll(5);
        assert_eq!(app.browser_offset, 6);
    }

    #[test]
    fn browser_scroll_selected_within_window_unchanged() {
        let mut app = make_app();
        app.browser_offset = 2;
        app.browser_selected = 4;
        app.clamp_browser_scroll(10);
        assert_eq!(app.browser_offset, 2);
    }

    // --- clamp_content_scroll ---

    #[test]
    fn content_scroll_clamped_when_past_end() {
        let mut app = make_app();
        app.content_lines = dummy_lines(20);
        app.content_scroll = 15;
        app.clamp_content_scroll(10);
        assert_eq!(app.content_scroll, 10);
    }

    #[test]
    fn content_scroll_unchanged_when_all_lines_fit() {
        let mut app = make_app();
        app.content_lines = dummy_lines(5);
        app.content_scroll = 0;
        app.clamp_content_scroll(10);
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn content_scroll_already_at_end_unchanged() {
        let mut app = make_app();
        app.content_lines = dummy_lines(20);
        app.content_scroll = 10;
        app.clamp_content_scroll(10);
        assert_eq!(app.content_scroll, 10);
    }

    // --- File tree tests using FakeGraphSource ---

    #[test]
    fn build_file_tree_includes_dirs_and_markdown() {
        let root = PathBuf::from("/test");

        let mut source = FakeGraphSource::new();
        // Add entries: note.md (file), subfolder (dir), ignore.txt (should be excluded by filtering)
        // Note: In FakeGraphSource, we only return what we explicitly add via add_dir_entries
        // So we only add the .md file and directory, not the .txt file
        source.add_dir_entries(
            root.clone(),
            vec![
                (root.join("note.md"), false, "# Note"),
                (root.join("subfolder"), true, ""),
            ],
        );

        let mut app = App {
            graph_path: root.clone(),
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
            search_active: false,
            search_query: String::new(),
            search_saved_scroll: 0,
            source,
        };

        app.build_file_tree().unwrap();

        // Should include .md file and directory
        let names: Vec<_> = app.file_items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"note"));
        assert!(names.contains(&"subfolder"));
        assert!(!names.contains(&"ignore"));
        assert!(!names.contains(&".hidden"));
        assert!(!names.contains(&"bak"));
    }

    #[test]
    fn expand_dir_inserts_children_with_correct_depth() {
        let root = PathBuf::from("/test");
        let dir1 = root.join("dir1");

        let mut source = FakeGraphSource::new();
        // Add dir1 as a directory under root
        source.add_dir_entries(root.clone(), vec![(dir1.clone(), true, "")]);
        // Add child.md under dir1
        source.add_dir_entries(
            dir1.clone(),
            vec![(dir1.join("child.md"), false, "content")],
        );

        let mut app = App {
            graph_path: root.clone(),
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
            search_active: false,
            search_query: String::new(),
            search_saved_scroll: 0,
            source,
        };

        app.build_file_tree().unwrap();
        assert_eq!(app.file_items.len(), 1);
        assert_eq!(app.file_items[0].name, "dir1");
        assert_eq!(app.file_items[0].depth, 0);

        // Expand the directory
        app.expand_dir(0, &dir1).unwrap();

        // Should now have parent and child
        assert_eq!(app.file_items.len(), 2);
        assert_eq!(app.file_items[0].name, "dir1");
        assert_eq!(app.file_items[0].depth, 0);
        assert_eq!(app.file_items[1].name, "child");
        assert_eq!(app.file_items[1].depth, 1);
    }

    #[test]
    fn collapse_dir_drains_children() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "a".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "b".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("c"),
                name: "c".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("d"),
                name: "d".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: false,
            },
        ];

        app.collapse_dir(0);

        assert_eq!(app.file_items.len(), 2);
        assert_eq!(app.file_items[0].name, "a");
        assert_eq!(app.file_items[1].name, "d");
    }

    #[test]
    fn collapse_or_jump_parent_jumps_to_parent() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("parent"),
                name: "parent".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("child"),
                name: "child".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
        ];

        app.browser_selected = 1;
        app.collapse_or_jump_parent();

        assert_eq!(app.browser_selected, 0);
    }

    // --- update method tests ---

    #[test]
    fn update_quit_returns_true() {
        let mut app = make_app();
        let should_quit = app.update(Action::Quit).unwrap();
        assert!(should_quit);
    }

    #[test]
    fn update_toggle_focus_switches_from_browser_to_content() {
        let mut app = make_app();
        app.focus = Focus::Browser;
        let should_quit = app.update(Action::ToggleFocus).unwrap();
        assert!(!should_quit);
        assert_eq!(app.focus, Focus::Content);
    }

    #[test]
    fn update_toggle_focus_switches_from_content_to_browser() {
        let mut app = make_app();
        app.focus = Focus::Content;
        let should_quit = app.update(Action::ToggleFocus).unwrap();
        assert!(!should_quit);
        assert_eq!(app.focus, Focus::Browser);
    }

    #[test]
    fn update_browser_down_increments_selected() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "a".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "b".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 0;

        let should_quit = app.update(Action::BrowserDown).unwrap();
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 1);
    }

    #[test]
    fn update_browser_up_decrements_selected() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "a".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "b".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 1;

        let should_quit = app.update(Action::BrowserUp).unwrap();
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 0);
    }

    #[test]
    fn update_content_down_increments_scroll() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_scroll = 0;

        let should_quit = app.update(Action::ContentDown(1)).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 1);
    }

    #[test]
    fn update_content_up_decrements_scroll() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_scroll = 5;

        let should_quit = app.update(Action::ContentUp(1)).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 4);
    }

    #[test]
    fn update_content_top_sets_scroll_to_zero() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_scroll = 5;

        let should_quit = app.update(Action::ContentTop).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn update_content_bottom_sets_scroll_to_max() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_scroll = 0;

        let should_quit = app.update(Action::ContentBottom).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 9);
    }

    // --- browser_top / browser_bottom tests ---

    #[test]
    fn browser_top_at_depth_0_goes_to_first_item() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "a".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "b".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("c"),
                name: "c".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 2;

        let should_quit = app.update(Action::BrowserTop).unwrap();
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 0);
    }

    #[test]
    fn browser_bottom_at_depth_0_goes_to_last_item() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "a".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "b".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("c"),
                name: "c".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 0;

        let should_quit = app.update(Action::BrowserBottom).unwrap();
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 2);
    }

    #[test]
    fn browser_top_inside_dir_goes_to_first_child() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("parent"),
                name: "parent".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("child1"),
                name: "child1".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("child2"),
                name: "child2".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("sibling"),
                name: "sibling".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 2; // child2 (depth 1)

        let should_quit = app.update(Action::BrowserTop).unwrap();
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 1); // first child of parent
    }

    #[test]
    fn browser_bottom_inside_dir_goes_to_last_child() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("parent"),
                name: "parent".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("child1"),
                name: "child1".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("child2"),
                name: "child2".to_string(),
                depth: 1,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("sibling"),
                name: "sibling".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 1; // child1 (depth 1)

        let should_quit = app.update(Action::BrowserBottom).unwrap();
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 2); // last child of parent
    }

    #[test]
    fn browser_top_inside_nested_dir_goes_to_first_child() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("parent"),
                name: "parent".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("subdir"),
                name: "subdir".to_string(),
                depth: 1,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("child1"),
                name: "child1".to_string(),
                depth: 2,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("child2"),
                name: "child2".to_string(),
                depth: 2,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("child3"),
                name: "child3".to_string(),
                depth: 2,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 3; // child2 (depth 2)

        let should_quit = app.update(Action::BrowserTop).unwrap();
        assert!(!should_quit);
        // parent of child2 (depth 2) is subdir (depth 1) at index 1
        // first item in subdir's subtree is at index 2 (child1, the first child)
        assert_eq!(app.browser_selected, 2);
    }

    #[test]
    fn browser_bottom_inside_nested_dir_goes_to_last_in_subtree() {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("parent"),
                name: "parent".to_string(),
                depth: 0,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("subdir"),
                name: "subdir".to_string(),
                depth: 1,
                is_dir: true,
                is_expanded: true,
            },
            FileItem {
                path: PathBuf::from("child1"),
                name: "child1".to_string(),
                depth: 2,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("child2"),
                name: "child2".to_string(),
                depth: 2,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("child3"),
                name: "child3".to_string(),
                depth: 2,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.browser_selected = 2; // child1 (depth 2)

        let should_quit = app.update(Action::BrowserBottom).unwrap();
        assert!(!should_quit);
        // parent of child1 (depth 2) is subdir (depth 1) at index 1
        // parent_depth = 1
        // end = first index after 1 where depth <= 1: index 5 (out of bounds), so end = 5
        // subtree of subdir is [2, 5) = [2, 3, 4]
        // last item in subdir's subtree is at index 4 (child3)
        assert_eq!(app.browser_selected, 4);
    }

    // --- Search tests ---

    #[test]
    fn search_start_in_content_focus_activates_search() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let should_quit = app.update(Action::SearchStart).unwrap();
        assert!(!should_quit);
        assert!(app.search_active);
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn search_start_in_browser_focus_no_op() {
        let mut app = make_app();
        app.focus = Focus::Browser;

        let should_quit = app.update(Action::SearchStart).unwrap();
        assert!(!should_quit);
        assert!(!app.search_active);
    }

    #[test]
    fn search_input_adds_char() {
        let mut app = make_app();
        app.search_active = true;

        let should_quit = app.update(Action::SearchInput('t')).unwrap();
        assert!(!should_quit);
        assert!(app.search_active);
        assert_eq!(app.search_query, "t");

        let should_quit = app.update(Action::SearchInput('e')).unwrap();
        assert!(!should_quit);
        assert_eq!(app.search_query, "te");
    }

    #[test]
    fn search_input_when_not_active_no_op() {
        let mut app = make_app();
        app.search_active = false;

        let should_quit = app.update(Action::SearchInput('t')).unwrap();
        assert!(!should_quit);
        assert!(!app.search_active);
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn search_backspace_removes_char() {
        let mut app = make_app();
        app.search_active = true;
        app.search_query = "test".to_string();

        let should_quit = app.update(Action::SearchBackspace).unwrap();
        assert!(!should_quit);
        assert_eq!(app.search_query, "tes");

        let should_quit = app.update(Action::SearchBackspace).unwrap();
        assert!(!should_quit);
        assert_eq!(app.search_query, "te");
    }

    #[test]
    fn search_backspace_empty_query_no_op() {
        let mut app = make_app();
        app.search_active = true;
        app.search_query = "t".to_string();

        let should_quit = app.update(Action::SearchBackspace).unwrap();
        assert!(!should_quit);
        assert_eq!(app.search_query, "");

        // Backspace on empty query should do nothing
        let should_quit = app.update(Action::SearchBackspace).unwrap();
        assert!(!should_quit);
        assert_eq!(app.search_query, "");
    }

    #[test]
    fn search_cancel_restore_scroll() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = dummy_lines(10);
        app.content_scroll = 5;

        // Start search
        app.update(Action::SearchStart).unwrap();
        assert_eq!(app.search_saved_scroll, 5);

        // Change scroll
        app.content_scroll = 7;

        // Cancel search
        let should_quit = app.update(Action::SearchCancel).unwrap();
        assert!(!should_quit);
        assert!(!app.search_active);
        assert!(app.search_query.is_empty());
        assert_eq!(app.content_scroll, 5); // Restored to saved position
    }

    #[test]
    fn search_commit_finds_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        // Create content with searchable text
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("first line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("another line".to_string())],
            },
        ];
        app.content_scroll = 0;

        // Start search and type query
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('r')).unwrap();

        // Commit search
        let should_quit = app.update(Action::SearchCommit).unwrap();
        assert!(!should_quit);
        assert!(!app.search_active);
        assert_eq!(app.search_query, "tar");
        assert_eq!(app.content_scroll, 1); // Should scroll to line 1 ("target line")
    }

    #[test]
    fn search_commit_no_match_stays_in_search() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        // Create content with searchable text
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("first line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("another line".to_string())],
            },
        ];
        app.content_scroll = 0;

        // Start search and type query that won't match
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('z')).unwrap();
        app.update(Action::SearchInput('z')).unwrap();

        // Commit search - should stay in search mode since no match
        let should_quit = app.update(Action::SearchCommit).unwrap();
        assert!(!should_quit);
        assert!(app.search_active); // Still in search mode
        assert_eq!(app.search_query, "zz");
    }

    #[test]
    fn search_next_finds_next_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        // Create content with multiple matching lines
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 1".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 2".to_string())],
            },
        ];
        app.content_scroll = 0;

        // Start search and commit
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchCommit).unwrap();

        assert_eq!(app.content_scroll, 0); // First match at line 0

        // Search next
        let should_quit = app.update(Action::SearchNext).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 2); // Next match at line 2
    }

    #[test]
    fn search_prev_finds_previous_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        // Create content with multiple matching lines
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 1".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 2".to_string())],
            },
        ];
        app.content_scroll = 2; // Start at line 2

        // Start search and commit
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchCommit).unwrap();

        // Since we're at line 2 which matches, commit keeps us there
        assert_eq!(app.content_scroll, 2);

        // Search prev
        let should_quit = app.update(Action::SearchPrev).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0); // Previous match at line 0
    }

    #[test]
    fn search_next_no_active_query_no_op() {
        let mut app = make_app();
        app.content_scroll = 0;

        let should_quit = app.update(Action::SearchNext).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0); // No change
    }

    #[test]
    fn search_prev_no_active_query_no_op() {
        let mut app = make_app();
        app.content_scroll = 5;

        let should_quit = app.update(Action::SearchPrev).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 5); // No change
    }

    #[test]
    fn search_case_insensitive() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        // Create content with mixed case
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("UPPER CASE TARGET".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("lower case line".to_string())],
            },
        ];
        app.content_scroll = 0;

        // Search with lowercase query
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('r')).unwrap();

        let should_quit = app.update(Action::SearchCommit).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0); // Should find "UPPER CASE TARGET"
    }

    #[test]
    fn search_with_mixed_segments() {
        use Segment;
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        // Create content with mixed segments
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![
                    Segment::Plain("Check ".to_string()),
                    Segment::PageLink("my page".to_string()),
                    Segment::Plain(" here".to_string()),
                ],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
            },
        ];
        app.content_scroll = 0;

        // Search for "page" which is inside a PageLink segment
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('p')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('g')).unwrap();
        app.update(Action::SearchInput('e')).unwrap();

        let should_quit = app.update(Action::SearchCommit).unwrap();
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0); // Should find line with PageLink containing "my page"
    }

    // --- match_line_indices tests ---

    #[test]
    fn match_line_indices_empty_query() {
        let mut app = make_app();
        app.content_lines = vec![ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain("test line".to_string())],
        }];
        app.search_query = String::new();

        let matches = app.match_line_indices();
        assert!(matches.is_empty());
    }

    #[test]
    fn match_line_indices_empty_content() {
        let mut app = make_app();
        app.search_query = "test".to_string();

        let matches = app.match_line_indices();
        assert!(matches.is_empty());
    }

    #[test]
    fn match_line_indices_single_match() {
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("first line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
            },
        ];
        app.search_query = "target".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches, vec![1]);
    }

    #[test]
    fn match_line_indices_multiple_matches() {
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 1".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 2".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 3".to_string())],
            },
        ];
        app.search_query = "target".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches, vec![0, 2, 3]);
    }

    #[test]
    fn match_line_indices_case_insensitive() {
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("UPPER CASE".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("lower case".to_string())],
            },
        ];
        app.search_query = "case".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches, vec![0, 1]);
    }

    #[test]
    fn match_count_correct() {
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 1".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("no match here".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 2".to_string())],
            },
        ];
        app.search_query = "match".to_string();

        // "match" appears in "match 1", "no match here", and "match 2" - 3 matches
        assert_eq!(app.match_count(), 3);
    }

    #[test]
    fn current_match_position_when_on_match() {
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 1".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 2".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 3".to_string())],
            },
        ];
        app.search_query = "match".to_string();

        // Position at first match
        app.content_scroll = 0;
        assert_eq!(app.current_match_position(), Some(1));

        // Position at second match
        app.content_scroll = 1;
        assert_eq!(app.current_match_position(), Some(2));

        // Position at third match
        app.content_scroll = 2;
        assert_eq!(app.current_match_position(), Some(3));
    }

    #[test]
    fn current_match_position_when_not_on_match() {
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 1".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("no match".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 2".to_string())],
            },
        ];
        app.search_query = "match".to_string();

        // Position at line 1 which contains "no match" - this IS a match for "match"
        // So current_match_position should be Some(2) (second match in 1-based)
        app.content_scroll = 1;
        assert_eq!(app.current_match_position(), Some(2));

        // Position at a line that doesn't match at all
        app.search_query = "xyz".to_string();
        app.content_scroll = 1;
        assert_eq!(app.current_match_position(), None);
    }

    #[test]
    fn current_match_position_no_matches() {
        let mut app = make_app();
        app.content_lines = vec![ParsedLine {
            indent: 0,
            is_bullet: false,
            task: None,
            segments: vec![Segment::Plain("no match".to_string())],
        }];
        app.search_query = "zzz".to_string();

        app.content_scroll = 0;
        assert_eq!(app.current_match_position(), None);
    }

    // --- Search with task keywords tests ---

    #[test]
    fn match_line_indices_finds_todo_task() {
        use crate::parser::TaskState;
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("buy milk".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("regular line".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Done),
                segments: vec![Segment::Plain("finished".to_string())],
            },
        ];
        app.search_query = "TODO".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], 0);
    }

    #[test]
    fn match_line_indices_finds_task_case_insensitive() {
        use crate::parser::TaskState;
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("buy milk".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Done),
                segments: vec![Segment::Plain("finished".to_string())],
            },
        ];
        // Search for lowercase "todo"
        app.search_query = "todo".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], 0);
    }

    #[test]
    fn match_line_indices_finds_multiple_task_states() {
        use crate::parser::TaskState;
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("first".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("regular".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Done),
                segments: vec![Segment::Plain("second".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("third".to_string())],
            },
        ];
        app.search_query = "TODO".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], 0);
        assert_eq!(matches[1], 3);
    }

    #[test]
    fn find_next_match_finds_todo_task() {
        use crate::parser::TaskState;
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("regular".to_string())],
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("task here".to_string())],
            },
        ];
        app.search_query = "TODO".to_string();

        // Start from position 0, should find the TODO at position 1
        let result = app.find_next_match(0, false);
        assert_eq!(result, Some(1));
    }
}
