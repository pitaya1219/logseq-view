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

/// A side effect requested by `update()` for `main.rs` to execute.
///
/// This is the TEA "Cmd" equivalent: `update()` stays pure state transition +
/// data, and anything that touches the terminal or spawns a process is
/// described here and interpreted by the shell's event loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Launch `$EDITOR` (with a fallback chain) on `path`, suspending the TUI
    /// for the duration. `main.rs` owns the terminal/process control; `app.rs`
    /// only describes the intent.
    LaunchEditor { path: PathBuf },
    /// Launch `$EDITOR` on just the current block: `main.rs` extracts the
    /// raw lines `[raw_start, raw_end)` (half-open, see `ParsedLine`'s span
    /// convention) of `path`, edits only that slice in a temp file, then
    /// splices the result back into `path`. `app.rs` only describes the
    /// intent and the (pure) computed range; the actual file/temp-file/
    /// process work lives in `main.rs`.
    EditBlock {
        path: PathBuf,
        raw_start: usize,
        raw_end: usize,
    },
}

/// The result of a single `update()` call: whether the app should quit, plus
/// any effects for `main.rs` to execute.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Update {
    pub quit: bool,
    pub effects: Vec<Effect>,
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
    /// Absolute line index of the first line of the current block (see
    /// `block_range_at`). Separate from `content_scroll` (the viewport's top
    /// line), mirroring `browser_selected` vs. `browser_offset`.
    pub content_cursor: usize,

    // content search state
    pub content_search_active: bool,
    pub content_search_query: String,
    pub content_search_saved_scroll: usize,
    pub content_search_saved_cursor: usize,

    // browser search state
    pub browser_search_active: bool,
    pub browser_search_query: String,
    pub browser_search_saved_selected: usize,

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
            content_cursor: 0,
            content_search_active: false,
            content_search_query: String::new(),
            content_search_saved_scroll: 0,
            content_search_saved_cursor: 0,
            browser_search_active: false,
            browser_search_query: String::new(),
            browser_search_saved_selected: 0,
            source,
        };
        app.build_file_tree()?;
        Ok(app)
    }

    pub(crate) fn build_file_tree(&mut self) -> Result<()> {
        self.file_items.clear();

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
            let path = item.path.clone();
            let idx = self.browser_selected;
            let is_expanded = self.file_items[idx].is_expanded;
            self.file_items[idx].is_expanded = !is_expanded;

            if is_expanded {
                self.collapse_dir(idx);
            } else {
                self.expand_dir(idx, &path)?;
            }
        } else {
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
        self.content_cursor = 0;
        Ok(())
    }

    /// Re-reads and re-parses the currently open file (if any) from the
    /// `GraphSource`, e.g. after the file was edited externally via
    /// `Effect::LaunchEditor`. Unlike `load_file`, this preserves the scroll
    /// position on a best-effort basis, clamping it to the new content
    /// length instead of resetting to the top.
    pub fn reload_current_file(&mut self) -> Result<()> {
        let Some(path) = self.current_file.clone() else {
            return Ok(());
        };
        let content = self.source.read(&path)?;
        self.content_lines = parse_file(&content);
        let max = self.content_lines.len().saturating_sub(1);
        self.content_scroll = self.content_scroll.min(max);
        self.content_cursor = self.content_cursor.min(max);
        Ok(())
    }

    /// Reads `path` through the `GraphSource` port. Exposed so `main.rs` can
    /// read raw file content (e.g. for block-level editing) without reaching
    /// past the port for ad-hoc fs access.
    pub fn read_file(&self, path: &Path) -> Result<String> {
        self.source.read(path)
    }

    /// Writes `contents` to `path` through the `GraphSource` port. See
    /// `read_file`.
    pub fn write_file(&self, path: &Path, contents: &str) -> Result<()> {
        self.source.write(path, contents)
    }

    /// Maps the current block (see `block_range_at`, content-LINE indices)
    /// to a RAW-file line range using each `ParsedLine`'s source span:
    /// `raw_start` is the first content line's `src_start`, `raw_end` is the
    /// LAST content line's `src_end` (half-open, matching `ParsedLine`'s own
    /// convention). Returns `None` when no file is open or the content is
    /// empty.
    pub fn current_block_src_range(&self) -> Option<(usize, usize)> {
        self.current_file.as_ref()?;
        if self.content_lines.is_empty() {
            return None;
        }
        let (start, end) = block_range_at(&self.content_lines, self.content_cursor);
        if end <= start || start >= self.content_lines.len() {
            return None;
        }
        let raw_start = self.content_lines[start].src_start;
        let raw_end = self.content_lines[end - 1].src_end;
        Some((raw_start, raw_end))
    }

    /// Main update function that handles all actions.
    /// Returns an `Update` describing whether the app should quit and any
    /// effects for `main.rs` to execute.
    pub fn update(&mut self, action: Action) -> Result<Update> {
        let effects = self.effects_for(&action);
        let quit = self.update_quit(action)?;
        Ok(Update { quit, effects })
    }

    /// Computes the effects (if any) that `action` should produce, without
    /// mutating state or performing the effect itself. Kept separate from
    /// `update_quit` so the core stays a pure state transition: it only
    /// *describes* the effect (e.g. `Effect::LaunchEditor`); `main.rs` is
    /// responsible for actually launching a process or touching the terminal.
    fn effects_for(&self, action: &Action) -> Vec<Effect> {
        match action {
            Action::EditCurrentPage => match &self.current_file {
                Some(path) => vec![Effect::LaunchEditor { path: path.clone() }],
                None => Vec::new(),
            },
            Action::EditCurrentBlock => {
                match (&self.current_file, self.current_block_src_range()) {
                    (Some(path), Some((raw_start, raw_end))) => vec![Effect::EditBlock {
                        path: path.clone(),
                        raw_start,
                        raw_end,
                    }],
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// Applies the action to the model, returning whether the app should quit.
    fn update_quit(&mut self, action: Action) -> Result<bool> {
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
            // Search actions are routed by current focus
            Action::SearchStart => {
                match self.focus {
                    Focus::Content => self.content_search_start(),
                    Focus::Browser => self.browser_search_start(),
                }
                Ok(false)
            }
            Action::SearchInput(c) => {
                match self.focus {
                    Focus::Content => self.content_search_input(c),
                    Focus::Browser => self.browser_search_input(c),
                }
                Ok(false)
            }
            Action::SearchBackspace => {
                match self.focus {
                    Focus::Content => self.content_search_backspace(),
                    Focus::Browser => self.browser_search_backspace(),
                }
                Ok(false)
            }
            Action::SearchCommit => {
                match self.focus {
                    Focus::Content => self.content_search_commit(),
                    Focus::Browser => self.browser_search_commit(),
                }
                Ok(false)
            }
            Action::SearchCancel => {
                match self.focus {
                    Focus::Content => self.content_search_cancel(),
                    Focus::Browser => self.browser_search_cancel(),
                }
                Ok(false)
            }
            Action::SearchNext => {
                match self.focus {
                    Focus::Content => self.content_search_next(),
                    Focus::Browser => self.browser_search_next(),
                }
                Ok(false)
            }
            Action::SearchPrev => {
                match self.focus {
                    Focus::Content => self.content_search_prev(),
                    Focus::Browser => self.browser_search_prev(),
                }
                Ok(false)
            }
            // The effect (if any) is computed by `effects_for`; here we only
            // decide whether to quit, which is always "no" for this action.
            Action::EditCurrentPage => Ok(false),
            Action::EditCurrentBlock => Ok(false),
        }
    }

    // --- Navigation ---

    pub(crate) fn collapse_or_jump_parent(&mut self) {
        let Some(item) = self.file_items.get(self.browser_selected) else {
            return;
        };

        if item.is_dir && item.is_expanded {
            let idx = self.browser_selected;
            self.file_items[idx].is_expanded = false;
            self.collapse_dir(idx);
        } else {
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

    /// Moves the block cursor forward `amount` blocks (see `block_range_at`
    /// and `next_block_start`). `content_scroll` is untouched here; it is
    /// made to follow the cursor separately, at render time, by
    /// `clamp_content_cursor_scroll` — mirroring how `browser_down` only
    /// touches `browser_selected` and `browser_offset` follows via
    /// `clamp_browser_scroll`.
    pub(crate) fn content_down(&mut self, amount: usize) {
        for _ in 0..amount {
            self.content_cursor = next_block_start(&self.content_lines, self.content_cursor);
        }
    }

    /// Moves the block cursor backward `amount` blocks. See `content_down`.
    pub(crate) fn content_up(&mut self, amount: usize) {
        for _ in 0..amount {
            self.content_cursor = prev_block_start(self.content_cursor);
        }
    }

    /// Moves the cursor to the first block. A deliberate UX change from the
    /// previous "scroll to top" behavior: this now selects/highlights the
    /// first block rather than just moving the viewport.
    pub(crate) fn content_top(&mut self) {
        self.content_cursor = 0;
    }

    /// Moves the cursor to the last block (see `content_top`).
    pub(crate) fn content_bottom(&mut self) {
        self.content_cursor = self.content_lines.len().saturating_sub(1);
    }

    /// Jump to the top of the current directory scope in the browser.
    pub(crate) fn browser_top(&mut self) {
        let selected_depth = self.file_items[self.browser_selected].depth;

        if selected_depth == 0 {
            self.browser_selected = 0;
        } else {
            let parent_idx = self.find_parent_index(self.browser_selected);
            if parent_idx + 1 < self.file_items.len() {
                self.browser_selected = parent_idx + 1;
            }
        }
    }

    /// Jump to the bottom of the current directory scope in the browser.
    pub(crate) fn browser_bottom(&mut self) {
        let selected_depth = self.file_items[self.browser_selected].depth;

        if selected_depth == 0 {
            if !self.file_items.is_empty() {
                self.browser_selected = self.file_items.len() - 1;
            }
        } else {
            let parent_idx = self.find_parent_index(self.browser_selected);
            let parent_depth = self.file_items[parent_idx].depth;

            let end = self.find_subtree_end(parent_idx, parent_depth);
            if end > parent_idx + 1 {
                self.browser_selected = end - 1;
            }
        }
    }

    fn find_parent_index(&self, idx: usize) -> usize {
        let depth = self.file_items[idx].depth;
        for i in (0..idx).rev() {
            if self.file_items[i].depth < depth {
                return i;
            }
        }
        0
    }

    fn find_subtree_end(&self, parent_idx: usize, parent_depth: usize) -> usize {
        let mut end = parent_idx + 1;
        while end < self.file_items.len() && self.file_items[end].depth > parent_depth {
            end += 1;
        }
        end
    }

    // --- Content search methods ---

    /// Start a new content search, saving the current scroll position
    pub(crate) fn content_search_start(&mut self) {
        if self.current_file.is_some() {
            self.content_search_active = true;
            self.content_search_query.clear();
            self.content_search_saved_scroll = self.content_scroll;
            self.content_search_saved_cursor = self.content_cursor;
        }
    }

    /// Add a character to the content search query
    pub(crate) fn content_search_input(&mut self, c: char) {
        if self.content_search_active {
            self.content_search_query.push(c);
        }
    }

    /// Remove the last character from the content search query
    pub(crate) fn content_search_backspace(&mut self) {
        if self.content_search_active {
            self.content_search_query.pop();
        }
    }

    /// Commit the content search - find the first matching line and scroll to it
    pub(crate) fn content_search_commit(&mut self) {
        if !self.content_search_active || self.content_search_query.is_empty() {
            self.content_search_cancel();
            return;
        }

        if let Some(matching_line) = self.content_find_next_match(self.content_scroll, true) {
            self.content_scroll = matching_line;
            self.content_cursor = matching_line;
            self.content_search_active = false;
        }
        // If no match found, stay in search mode
    }

    /// Cancel content search and restore the saved scroll position
    pub(crate) fn content_search_cancel(&mut self) {
        self.content_search_active = false;
        self.content_search_query.clear();
        self.content_scroll = self.content_search_saved_scroll;
        self.content_cursor = self.content_search_saved_cursor;
    }

    /// Find the next content match (for n key)
    pub(crate) fn content_search_next(&mut self) {
        if !self.content_search_active && self.content_search_query.is_empty() {
            return;
        }

        let start_pos = if self.content_search_active {
            self.content_scroll
        } else {
            self.content_scroll + 1
        };

        if let Some(matching_line) = self.content_find_next_match(start_pos, false) {
            self.content_scroll = matching_line;
            self.content_cursor = matching_line;
            if self.content_search_active {
                self.content_search_active = false;
            }
        }
    }

    /// Find the previous content match (for N key)
    pub(crate) fn content_search_prev(&mut self) {
        if !self.content_search_active && self.content_search_query.is_empty() {
            return;
        }

        let start_pos = self.content_scroll;

        if let Some(matching_line) = self.content_find_prev_match(start_pos, false) {
            self.content_scroll = matching_line;
            self.content_cursor = matching_line;
            if self.content_search_active {
                self.content_search_active = false;
            }
        }
    }

    /// Find the next line matching the content search query, starting from start_pos
    pub(crate) fn content_find_next_match(&self, start_pos: usize, wrap: bool) -> Option<usize> {
        if self.content_search_query.is_empty() || self.content_lines.is_empty() {
            return None;
        }

        let query = self.content_search_query.to_lowercase();
        let total_lines = self.content_lines.len();

        for i in start_pos..total_lines {
            let line_text = line_to_plain_text(&self.content_lines[i]).to_lowercase();
            if line_text.contains(&query) {
                return Some(i);
            }
        }

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

    /// Find the previous line matching the content search query, starting from start_pos
    pub(crate) fn content_find_prev_match(&self, start_pos: usize, wrap: bool) -> Option<usize> {
        if self.content_search_query.is_empty() || self.content_lines.is_empty() {
            return None;
        }

        let query = self.content_search_query.to_lowercase();
        let total_lines = self.content_lines.len();

        if start_pos > 0 {
            for i in (0..start_pos).rev() {
                let line_text = line_to_plain_text(&self.content_lines[i]).to_lowercase();
                if line_text.contains(&query) {
                    return Some(i);
                }
            }
        }

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

    /// Returns a vector of all line indices that match the current content search query.
    pub fn match_line_indices(&self) -> Vec<usize> {
        if self.content_search_query.is_empty() || self.content_lines.is_empty() {
            return Vec::new();
        }

        let query = self.content_search_query.to_lowercase();
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

    /// Returns the total number of matching lines for the current content search query
    pub fn match_count(&self) -> usize {
        self.match_line_indices().len()
    }

    /// Returns the 1-based position of the current match (line at content_scroll)
    /// among all matching lines. Returns None if the current line is not a match.
    pub fn current_match_position(&self) -> Option<usize> {
        let matches = self.match_line_indices();
        if matches.is_empty() {
            return None;
        }
        matches
            .iter()
            .position(|&idx| idx == self.content_scroll)
            .map(|pos| pos + 1)
    }

    // --- Browser search methods ---

    /// Start a new browser name search: save current selection, clear query, enter input mode
    pub(crate) fn browser_search_start(&mut self) {
        self.browser_search_saved_selected = self.browser_selected;
        self.browser_search_query.clear();
        self.browser_search_active = true;
    }

    /// Append a character to the browser search query and find the next match
    pub(crate) fn browser_search_input(&mut self, c: char) {
        self.browser_search_query.push(c);
        self.browser_find_next_match(true);
    }

    /// Remove the last character from the browser search query and find the next match
    pub(crate) fn browser_search_backspace(&mut self) {
        self.browser_search_query.pop();
        self.browser_find_next_match(true);
    }

    /// Commit the browser search: exit input mode, keep current selection
    pub(crate) fn browser_search_commit(&mut self) {
        self.browser_search_active = false;
    }

    /// Cancel the browser search: exit input mode, restore saved selection
    pub(crate) fn browser_search_cancel(&mut self) {
        self.browser_search_query.clear();
        self.browser_search_active = false;
        self.browser_selected = self.browser_search_saved_selected;
    }

    /// Find the next browser match (for n key after commit)
    pub(crate) fn browser_search_next(&mut self) {
        self.browser_find_next_match(false);
    }

    /// Find the previous browser match (for N key after commit)
    pub(crate) fn browser_search_prev(&mut self) {
        self.browser_find_prev_match(false);
    }

    /// Find the next matching file/dir item, wrapping around
    fn browser_find_next_match(&mut self, include_current: bool) {
        if self.browser_search_query.is_empty() {
            return;
        }

        let query_lower = self.browser_search_query.to_lowercase();
        let start_idx = if include_current {
            self.browser_selected
        } else {
            self.browser_selected + 1
        };

        let num_items = self.file_items.len();
        if num_items == 0 {
            return;
        }

        for i in start_idx..num_items {
            if self.item_matches(&self.file_items[i], &query_lower) {
                self.browser_selected = i;
                return;
            }
        }

        for i in 0..start_idx {
            if self.item_matches(&self.file_items[i], &query_lower) {
                self.browser_selected = i;
                return;
            }
        }
    }

    /// Find the previous matching file/dir item, wrapping around
    fn browser_find_prev_match(&mut self, include_current: bool) {
        if self.browser_search_query.is_empty() {
            return;
        }

        let query_lower = self.browser_search_query.to_lowercase();
        let start_idx = if include_current {
            self.browser_selected
        } else {
            self.browser_selected.saturating_sub(1)
        };

        let num_items = self.file_items.len();
        if num_items == 0 {
            return;
        }

        let mut idx = start_idx;
        loop {
            if idx < num_items && self.item_matches(&self.file_items[idx], &query_lower) {
                self.browser_selected = idx;
                return;
            }

            if idx == 0 {
                idx = num_items;
            }
            idx = idx.saturating_sub(1);

            if idx == start_idx && !include_current {
                break;
            }
        }
    }

    fn item_matches(&self, item: &FileItem, query_lower: &str) -> bool {
        item.name.to_lowercase().contains(query_lower)
    }

    /// Returns true if there is a committed browser search query (non-empty, not in input mode)
    pub fn browser_has_committed_search(&self) -> bool {
        !self.browser_search_query.is_empty() && !self.browser_search_active
    }

    // --- Scroll clamping ---

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

    /// Adjusts `content_scroll` so `content_cursor` stays within the visible
    /// window, mirroring `clamp_browser_scroll`'s selection-follows-viewport
    /// pattern (`browser_selected` / `browser_offset`) for the content pane
    /// (`content_cursor` / `content_scroll`).
    pub(crate) fn clamp_content_cursor_scroll(&mut self, visible_height: usize) {
        if self.content_cursor < self.content_scroll {
            self.content_scroll = self.content_cursor;
        } else if self.content_cursor >= self.content_scroll + visible_height {
            self.content_scroll = self.content_cursor + 1 - visible_height;
        }
    }

    pub(crate) fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Content,
            Focus::Content => Focus::Browser,
        };
    }
}

/// Computes the `[start, end)` line-index range of the Logseq block that
/// starts at `line_idx`: the line itself, plus all immediately following
/// lines whose indent is strictly greater than `lines[line_idx]`'s indent,
/// stopping at (not including) the next line whose indent is `<=` it.
///
/// Every line index is a valid block start under this definition: a leaf
/// line (nothing more deeply indented follows it) is simply a single-line
/// block. Shared exactly with the future block-editing feature (#47), so
/// changes here should stay in lock-step with that definition.
///
/// Returns `(line_idx, line_idx)` (an empty range) if `line_idx` is out of
/// bounds, e.g. an empty file.
pub fn block_range_at(lines: &[ParsedLine], line_idx: usize) -> (usize, usize) {
    if line_idx >= lines.len() {
        return (line_idx, line_idx);
    }
    let base_indent = lines[line_idx].indent;
    let mut end = line_idx + 1;
    while end < lines.len() && lines[end].indent > base_indent {
        end += 1;
    }
    (line_idx, end)
}

/// The next block-start line after `line_idx`.
///
/// Because every line is a valid block start (see `block_range_at`) and the
/// file is already stored in depth-first pre-order (a bullet immediately
/// followed by its own nested children), the next block is simply the next
/// line — clamped so the cursor stops at the last line rather than running
/// past the end.
fn next_block_start(lines: &[ParsedLine], line_idx: usize) -> usize {
    if lines.is_empty() {
        return 0;
    }
    (line_idx + 1).min(lines.len() - 1)
}

/// The previous block-start line before `line_idx` (see `next_block_start`).
fn prev_block_start(line_idx: usize) -> usize {
    line_idx.saturating_sub(1)
}

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
        App::new(PathBuf::new(), FakeGraphSource::new()).unwrap()
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

    // --- clamp_content_cursor_scroll (cursor-follows-viewport, mirrors
    //     clamp_browser_scroll) ---

    #[test]
    fn content_cursor_scroll_cursor_before_offset_clamps_up() {
        let mut app = make_app();
        app.content_scroll = 5;
        app.content_cursor = 3;
        app.clamp_content_cursor_scroll(10);
        assert_eq!(app.content_scroll, 3);
    }

    #[test]
    fn content_cursor_scroll_cursor_past_window_slides_down() {
        let mut app = make_app();
        app.content_scroll = 0;
        app.content_cursor = 10;
        app.clamp_content_cursor_scroll(5);
        assert_eq!(app.content_scroll, 6);
    }

    #[test]
    fn content_cursor_scroll_cursor_within_window_unchanged() {
        let mut app = make_app();
        app.content_scroll = 2;
        app.content_cursor = 4;
        app.clamp_content_cursor_scroll(10);
        assert_eq!(app.content_scroll, 2);
    }

    // --- block_range_at ---

    fn lines_with_indents(indents: &[usize]) -> Vec<ParsedLine> {
        indents
            .iter()
            .map(|&indent| ParsedLine {
                indent,
                is_bullet: true,
                task: None,
                segments: Vec::new(),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn block_range_at_single_line_file_is_its_own_block() {
        let lines = lines_with_indents(&[0]);
        assert_eq!(block_range_at(&lines, 0), (0, 1));
    }

    #[test]
    fn block_range_at_leaf_among_siblings() {
        // 0:A(0) 1:B(0) 2:C(0) — flat siblings, each its own single-line block.
        let lines = lines_with_indents(&[0, 0, 0]);
        assert_eq!(block_range_at(&lines, 0), (0, 1));
        assert_eq!(block_range_at(&lines, 1), (1, 2));
        assert_eq!(block_range_at(&lines, 2), (2, 3));
    }

    #[test]
    fn block_range_at_single_level_children() {
        // 0:A(0) 1:A1(1) 2:A2(1) 3:B(0)
        let lines = lines_with_indents(&[0, 1, 1, 0]);
        assert_eq!(block_range_at(&lines, 0), (0, 3));
        assert_eq!(block_range_at(&lines, 1), (1, 2));
        assert_eq!(block_range_at(&lines, 2), (2, 3));
        assert_eq!(block_range_at(&lines, 3), (3, 4));
    }

    #[test]
    fn block_range_at_nested_multi_level() {
        // 0:A(0) 1:A1(1) 2:A1a(2) 3:A2(1) 4:B(0)
        let lines = lines_with_indents(&[0, 1, 2, 1, 0]);
        assert_eq!(block_range_at(&lines, 0), (0, 4));
        assert_eq!(block_range_at(&lines, 1), (1, 3));
        assert_eq!(block_range_at(&lines, 2), (2, 3));
        assert_eq!(block_range_at(&lines, 3), (3, 4));
        assert_eq!(block_range_at(&lines, 4), (4, 5));
    }

    #[test]
    fn block_range_at_last_line_with_children_extends_to_end() {
        // 0:A(0) 1:A1(1) 2:A2(1) — block A has no following sibling, extends to EOF.
        let lines = lines_with_indents(&[0, 1, 1]);
        assert_eq!(block_range_at(&lines, 0), (0, 3));
    }

    #[test]
    fn block_range_at_out_of_bounds_returns_empty_range() {
        let lines = lines_with_indents(&[0, 1]);
        assert_eq!(block_range_at(&lines, 5), (5, 5));
    }

    #[test]
    fn block_range_at_empty_file_returns_empty_range() {
        let lines: Vec<ParsedLine> = Vec::new();
        assert_eq!(block_range_at(&lines, 0), (0, 0));
    }

    // --- content_down / content_up block-cursor navigation ---

    #[test]
    fn content_down_advances_cursor_through_nested_lines() {
        // Per-line DFS-order cursor movement: descending into children is a
        // normal "next block" step, since block_range_at(parent) only
        // matters for highlighting the parent's whole subtree, not for
        // where the cursor is allowed to land.
        let mut app = make_app();
        app.content_lines = lines_with_indents(&[0, 1, 1, 0]);
        app.content_cursor = 0;

        app.content_down(1);
        assert_eq!(app.content_cursor, 1);
        app.content_down(1);
        assert_eq!(app.content_cursor, 2);
        app.content_down(1);
        assert_eq!(app.content_cursor, 3);
    }

    #[test]
    fn content_down_clamped_at_last_line() {
        let mut app = make_app();
        app.content_lines = lines_with_indents(&[0, 1]);
        app.content_cursor = 1;

        app.content_down(5);
        assert_eq!(app.content_cursor, 1);
    }

    #[test]
    fn content_up_retreats_cursor() {
        let mut app = make_app();
        app.content_lines = lines_with_indents(&[0, 1, 1, 0]);
        app.content_cursor = 3;

        app.content_up(1);
        assert_eq!(app.content_cursor, 2);
        app.content_up(1);
        assert_eq!(app.content_cursor, 1);
    }

    #[test]
    fn content_up_clamped_at_zero() {
        let mut app = make_app();
        app.content_lines = lines_with_indents(&[0, 1]);
        app.content_cursor = 0;

        app.content_up(5);
        assert_eq!(app.content_cursor, 0);
    }

    #[test]
    fn content_top_moves_cursor_to_first_block() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_cursor = 7;

        app.content_top();
        assert_eq!(app.content_cursor, 0);
    }

    #[test]
    fn content_bottom_moves_cursor_to_last_block() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_cursor = 0;

        app.content_bottom();
        assert_eq!(app.content_cursor, 9);
    }

    #[test]
    fn reload_current_file_clamps_cursor_too() {
        let mut source = FakeGraphSource::new();
        let path = PathBuf::from("/graph/pages/foo.md");
        source.add_file(path.clone(), "line one\nline two\n");

        let mut app = App::new(PathBuf::from("/graph"), source).unwrap();
        app.current_file = Some(path);
        app.content_lines = dummy_lines(50);
        app.content_cursor = 40;

        app.reload_current_file().unwrap();

        assert_eq!(app.content_lines.len(), 2);
        assert_eq!(app.content_cursor, 1);
    }

    // --- content search keeps content_cursor in sync with content_scroll ---

    #[test]
    fn content_search_commit_moves_cursor_to_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("first line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line".to_string())],
                ..Default::default()
            },
        ];
        app.content_cursor = 0;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('r')).unwrap();
        app.update(Action::SearchCommit).unwrap();

        assert_eq!(app.content_cursor, 1);
    }

    #[test]
    fn content_search_cancel_restores_cursor() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = dummy_lines(10);
        app.content_cursor = 5;
        app.content_scroll = 5;

        app.update(Action::SearchStart).unwrap();
        app.content_cursor = 8;

        app.update(Action::SearchCancel).unwrap();
        assert_eq!(app.content_cursor, 5);
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

        let mut app = App::new(root.clone(), source).unwrap();
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
        source.add_dir_entries(root.clone(), vec![(dir1.clone(), true, "")]);
        source.add_dir_entries(
            dir1.clone(),
            vec![(dir1.join("child.md"), false, "content")],
        );

        let mut app = App::new(root.clone(), source).unwrap();
        app.build_file_tree().unwrap();
        assert_eq!(app.file_items.len(), 1);
        assert_eq!(app.file_items[0].name, "dir1");
        assert_eq!(app.file_items[0].depth, 0);

        app.expand_dir(0, &dir1).unwrap();

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
        let should_quit = app.update(Action::Quit).unwrap().quit;
        assert!(should_quit);
    }

    #[test]
    fn reload_current_file_reparses_and_clamps_scroll() {
        let mut source = FakeGraphSource::new();
        let path = PathBuf::from("/graph/pages/foo.md");
        source.add_file(path.clone(), "line one\nline two\n");

        let mut app = App::new(PathBuf::from("/graph"), source).unwrap();
        app.current_file = Some(path);
        // Simulate stale state left over from a longer previous version of
        // the file (e.g. before an external edit shortened it).
        app.content_lines = dummy_lines(50);
        app.content_scroll = 40;

        app.reload_current_file().unwrap();

        assert_eq!(app.content_lines.len(), 2);
        assert_eq!(app.content_scroll, 1); // clamped to new last-line index
    }

    #[test]
    fn reload_current_file_without_current_file_is_noop() {
        let mut app = make_app();
        app.content_lines = dummy_lines(5);
        app.content_scroll = 3;

        app.reload_current_file().unwrap();

        assert_eq!(app.content_lines.len(), 5);
        assert_eq!(app.content_scroll, 3);
    }

    #[test]
    fn update_edit_current_page_with_file_returns_launch_editor_effect() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/graph/pages/foo.md"));

        let update = app.update(Action::EditCurrentPage).unwrap();

        assert!(!update.quit);
        assert_eq!(
            update.effects,
            vec![Effect::LaunchEditor {
                path: PathBuf::from("/graph/pages/foo.md")
            }]
        );
    }

    #[test]
    fn update_edit_current_page_without_file_returns_no_effect() {
        let mut app = make_app();
        app.current_file = None;

        let update = app.update(Action::EditCurrentPage).unwrap();

        assert!(!update.quit);
        assert!(update.effects.is_empty());
    }

    #[test]
    fn update_edit_current_block_returns_edit_block_effect_with_computed_range() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/graph/pages/foo.md"));
        // Block: line 0 (parent) has children at raw lines 1 and 2 (each a
        // single-raw-line ParsedLine by default span == index..index+1), so
        // the block's raw range should be [0, 3).
        app.content_lines = lines_with_indents(&[0, 1, 1, 0]);
        for (i, line) in app.content_lines.iter_mut().enumerate() {
            line.src_start = i;
            line.src_end = i + 1;
        }
        app.content_cursor = 0;

        let update = app.update(Action::EditCurrentBlock).unwrap();

        assert!(!update.quit);
        assert_eq!(
            update.effects,
            vec![Effect::EditBlock {
                path: PathBuf::from("/graph/pages/foo.md"),
                raw_start: 0,
                raw_end: 3,
            }]
        );
    }

    #[test]
    fn update_edit_current_block_without_file_returns_no_effect() {
        let mut app = make_app();
        app.current_file = None;
        app.content_lines = dummy_lines(3);

        let update = app.update(Action::EditCurrentBlock).unwrap();

        assert!(!update.quit);
        assert!(update.effects.is_empty());
    }

    #[test]
    fn update_edit_current_block_with_empty_content_returns_no_effect() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/graph/pages/foo.md"));
        app.content_lines = Vec::new();

        let update = app.update(Action::EditCurrentBlock).unwrap();

        assert!(!update.quit);
        assert!(update.effects.is_empty());
    }

    #[test]
    fn current_block_src_range_uses_first_and_last_line_spans() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/graph/pages/foo.md"));
        app.content_lines = lines_with_indents(&[0, 1, 1, 0]);
        // Simulate a folded code-block child spanning raw lines 1..=5.
        app.content_lines[1].src_start = 1;
        app.content_lines[1].src_end = 6;
        app.content_lines[2].src_start = 6;
        app.content_lines[2].src_end = 7;
        app.content_lines[0].src_start = 0;
        app.content_lines[0].src_end = 1;
        app.content_cursor = 0;

        let range = app.current_block_src_range();
        assert_eq!(range, Some((0, 7)));
    }

    #[test]
    fn current_block_src_range_none_without_current_file() {
        let mut app = make_app();
        app.current_file = None;
        app.content_lines = dummy_lines(3);

        assert_eq!(app.current_block_src_range(), None);
    }

    #[test]
    fn current_block_src_range_none_when_content_empty() {
        let mut app = make_app();
        app.current_file = Some(PathBuf::from("/graph/pages/foo.md"));
        app.content_lines = Vec::new();

        assert_eq!(app.current_block_src_range(), None);
    }

    /// End-to-end block-edit round trip through the `GraphSource` port
    /// (`read_file` -> `splice_raw_lines` -> `write_file`), the same
    /// sequence `main.rs::launch_block_editor` performs around the actual
    /// `$EDITOR` call. Asserts only the block's own raw lines are replaced
    /// and every other line survives untouched.
    #[test]
    fn block_edit_round_trip_replaces_only_block_raw_range() {
        let mut source = FakeGraphSource::new();
        let path = PathBuf::from("/graph/pages/foo.md");
        source.add_file(
            path.clone(),
            "- A\n  - A1\n  - A2\n- B\n- C\n", // block "A" = raw lines 0..3
        );

        let mut app = App::new(PathBuf::from("/graph"), source).unwrap();
        app.load_file(&path).unwrap();
        app.content_cursor = 0; // on block A

        let (raw_start, raw_end) = app.current_block_src_range().unwrap();
        assert_eq!((raw_start, raw_end), (0, 3));

        let original = app.read_file(&path).unwrap();
        let new_content =
            crate::parser::splice_raw_lines(&original, raw_start, raw_end, "- A EDITED\n");
        app.write_file(&path, &new_content).unwrap();
        app.reload_current_file().unwrap();

        assert_eq!(
            app.read_file(&path).unwrap(),
            "- A EDITED\n- B\n- C\n",
            "only the block's raw range should be replaced; B and C must survive untouched"
        );
    }

    #[test]
    fn update_toggle_focus_switches_from_browser_to_content() {
        let mut app = make_app();
        app.focus = Focus::Browser;
        let should_quit = app.update(Action::ToggleFocus).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.focus, Focus::Content);
    }

    #[test]
    fn update_toggle_focus_switches_from_content_to_browser() {
        let mut app = make_app();
        app.focus = Focus::Content;
        let should_quit = app.update(Action::ToggleFocus).unwrap().quit;
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

        let should_quit = app.update(Action::BrowserDown).unwrap().quit;
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

        let should_quit = app.update(Action::BrowserUp).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 0);
    }

    // NOTE: as of the block-cursor feature (#45), ContentDown/ContentUp/
    // ContentTop/ContentBottom move `content_cursor` (the block cursor), not
    // `content_scroll` directly. `content_scroll` now only follows the
    // cursor at render time via `clamp_content_cursor_scroll`. These tests
    // supersede the pre-cursor versions that asserted on `content_scroll`.

    #[test]
    fn update_content_down_moves_cursor_not_scroll() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_cursor = 0;
        app.content_scroll = 0;

        let should_quit = app.update(Action::ContentDown(1)).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_cursor, 1);
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn update_content_up_moves_cursor_not_scroll() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_cursor = 5;
        app.content_scroll = 5;

        let should_quit = app.update(Action::ContentUp(1)).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_cursor, 4);
        assert_eq!(app.content_scroll, 5);
    }

    #[test]
    fn update_content_top_sets_cursor_to_first_block() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_cursor = 5;

        let should_quit = app.update(Action::ContentTop).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_cursor, 0);
    }

    #[test]
    fn update_content_bottom_sets_cursor_to_last_block() {
        let mut app = make_app();
        app.content_lines = dummy_lines(10);
        app.content_cursor = 0;

        let should_quit = app.update(Action::ContentBottom).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_cursor, 9);
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

        let should_quit = app.update(Action::BrowserTop).unwrap().quit;
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

        let should_quit = app.update(Action::BrowserBottom).unwrap().quit;
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
        app.browser_selected = 2;

        let should_quit = app.update(Action::BrowserTop).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 1);
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
        app.browser_selected = 1;

        let should_quit = app.update(Action::BrowserBottom).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 2);
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
        app.browser_selected = 3;

        let should_quit = app.update(Action::BrowserTop).unwrap().quit;
        assert!(!should_quit);
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
        app.browser_selected = 2;

        let should_quit = app.update(Action::BrowserBottom).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.browser_selected, 4);
    }

    // --- Content search tests ---

    #[test]
    fn content_search_start_activates_search() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        let should_quit = app.update(Action::SearchStart).unwrap().quit;
        assert!(!should_quit);
        assert!(app.content_search_active);
        assert!(app.content_search_query.is_empty());
    }

    #[test]
    fn content_search_input_adds_char() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.content_search_active = true;

        app.update(Action::SearchInput('t')).unwrap();
        assert_eq!(app.content_search_query, "t");

        app.update(Action::SearchInput('e')).unwrap();
        assert_eq!(app.content_search_query, "te");
    }

    #[test]
    fn content_search_input_when_not_active_no_op() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.content_search_active = false;

        app.update(Action::SearchInput('t')).unwrap();
        assert!(!app.content_search_active);
        assert!(app.content_search_query.is_empty());
    }

    #[test]
    fn content_search_backspace_removes_char() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.content_search_active = true;
        app.content_search_query = "test".to_string();

        app.update(Action::SearchBackspace).unwrap();
        assert_eq!(app.content_search_query, "tes");

        app.update(Action::SearchBackspace).unwrap();
        assert_eq!(app.content_search_query, "te");
    }

    #[test]
    fn content_search_backspace_empty_query_no_op() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.content_search_active = true;
        app.content_search_query = "t".to_string();

        app.update(Action::SearchBackspace).unwrap();
        assert_eq!(app.content_search_query, "");

        app.update(Action::SearchBackspace).unwrap();
        assert_eq!(app.content_search_query, "");
    }

    #[test]
    fn content_search_cancel_restores_scroll() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));
        app.content_lines = dummy_lines(10);
        app.content_scroll = 5;

        app.update(Action::SearchStart).unwrap();
        assert_eq!(app.content_search_saved_scroll, 5);

        app.content_scroll = 7;

        app.update(Action::SearchCancel).unwrap();
        assert!(!app.content_search_active);
        assert!(app.content_search_query.is_empty());
        assert_eq!(app.content_scroll, 5);
    }

    #[test]
    fn content_search_commit_finds_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("first line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("another line".to_string())],
                ..Default::default()
            },
        ];
        app.content_scroll = 0;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('r')).unwrap();

        let should_quit = app.update(Action::SearchCommit).unwrap().quit;
        assert!(!should_quit);
        assert!(!app.content_search_active);
        assert_eq!(app.content_search_query, "tar");
        assert_eq!(app.content_scroll, 1);
    }

    #[test]
    fn content_search_commit_no_match_stays_in_search() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("first line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("another line".to_string())],
                ..Default::default()
            },
        ];
        app.content_scroll = 0;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('z')).unwrap();
        app.update(Action::SearchInput('z')).unwrap();

        let should_quit = app.update(Action::SearchCommit).unwrap().quit;
        assert!(!should_quit);
        assert!(app.content_search_active);
        assert_eq!(app.content_search_query, "zz");
    }

    #[test]
    fn content_search_next_finds_next_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 1".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 2".to_string())],
                ..Default::default()
            },
        ];
        app.content_scroll = 0;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchCommit).unwrap();

        assert_eq!(app.content_scroll, 0);

        let should_quit = app.update(Action::SearchNext).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 2);
    }

    #[test]
    fn content_search_prev_finds_previous_match() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 1".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 2".to_string())],
                ..Default::default()
            },
        ];
        app.content_scroll = 2;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchCommit).unwrap();

        assert_eq!(app.content_scroll, 2);

        let should_quit = app.update(Action::SearchPrev).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn content_search_next_no_active_query_no_op() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.content_scroll = 0;

        let should_quit = app.update(Action::SearchNext).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn content_search_prev_no_active_query_no_op() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.content_scroll = 5;

        let should_quit = app.update(Action::SearchPrev).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 5);
    }

    #[test]
    fn content_search_case_insensitive() {
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("UPPER CASE TARGET".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("lower case line".to_string())],
                ..Default::default()
            },
        ];
        app.content_scroll = 0;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('t')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('r')).unwrap();

        let should_quit = app.update(Action::SearchCommit).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0);
    }

    #[test]
    fn content_search_with_mixed_segments() {
        use Segment;
        let mut app = make_app();
        app.focus = Focus::Content;
        app.current_file = Some(PathBuf::from("/test/file.md"));

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
                ..Default::default()
            },
        ];
        app.content_scroll = 0;

        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('p')).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('g')).unwrap();
        app.update(Action::SearchInput('e')).unwrap();

        let should_quit = app.update(Action::SearchCommit).unwrap().quit;
        assert!(!should_quit);
        assert_eq!(app.content_scroll, 0);
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
            ..Default::default()
        }];
        app.content_search_query = String::new();

        let matches = app.match_line_indices();
        assert!(matches.is_empty());
    }

    #[test]
    fn match_line_indices_empty_content() {
        let mut app = make_app();
        app.content_search_query = "test".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "target".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("other line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 2".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("target line 3".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "target".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("lower case".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "case".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("no match here".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 2".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "match".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 2".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 3".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "match".to_string();

        app.content_scroll = 0;
        assert_eq!(app.current_match_position(), Some(1));

        app.content_scroll = 1;
        assert_eq!(app.current_match_position(), Some(2));

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("no match".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("match 2".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "match".to_string();

        app.content_scroll = 1;
        assert_eq!(app.current_match_position(), Some(2));

        app.content_search_query = "xyz".to_string();
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
            ..Default::default()
        }];
        app.content_search_query = "zzz".to_string();

        app.content_scroll = 0;
        assert_eq!(app.current_match_position(), None);
    }

    // --- Content search with task keywords ---

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("regular line".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Done),
                segments: vec![Segment::Plain("finished".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "TODO".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Done),
                segments: vec![Segment::Plain("finished".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "todo".to_string();

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
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("regular".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Done),
                segments: vec![Segment::Plain("second".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("third".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "TODO".to_string();

        let matches = app.match_line_indices();
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0], 0);
        assert_eq!(matches[1], 3);
    }

    #[test]
    fn content_find_next_match_finds_todo_task() {
        use crate::parser::TaskState;
        let mut app = make_app();
        app.content_lines = vec![
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: None,
                segments: vec![Segment::Plain("regular".to_string())],
                ..Default::default()
            },
            ParsedLine {
                indent: 0,
                is_bullet: false,
                task: Some(TaskState::Todo),
                segments: vec![Segment::Plain("task here".to_string())],
                ..Default::default()
            },
        ];
        app.content_search_query = "TODO".to_string();

        let result = app.content_find_next_match(0, false);
        assert_eq!(result, Some(1));
    }

    // --- Browser search tests ---

    fn make_app_with_files() -> App<FakeGraphSource> {
        let mut app = make_app();
        app.file_items = vec![
            FileItem {
                path: PathBuf::from("a"),
                name: "apple".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("b"),
                name: "banana".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("c"),
                name: "cherry".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
            FileItem {
                path: PathBuf::from("d"),
                name: "date".to_string(),
                depth: 0,
                is_dir: false,
                is_expanded: false,
            },
        ];
        app.focus = Focus::Browser;
        app
    }

    #[test]
    fn browser_search_start_saves_selection_and_enters_input_mode() {
        let mut app = make_app_with_files();
        app.browser_selected = 2;

        app.update(Action::SearchStart).unwrap();

        assert!(app.browser_search_active);
        assert_eq!(app.browser_search_saved_selected, 2);
        assert!(app.browser_search_query.is_empty());
    }

    #[test]
    fn browser_search_input_appends_char_and_finds_match() {
        let mut app = make_app_with_files();
        app.update(Action::SearchStart).unwrap();

        app.update(Action::SearchInput('a')).unwrap();

        assert_eq!(app.browser_search_query, "a");
        assert_eq!(app.browser_selected, 0); // "apple" contains 'a'
    }

    #[test]
    fn browser_search_input_case_insensitive() {
        let mut app = make_app_with_files();
        app.update(Action::SearchStart).unwrap();

        app.update(Action::SearchInput('A')).unwrap();

        assert_eq!(app.browser_selected, 0); // "apple" matches 'A' case-insensitively
    }

    #[test]
    fn browser_search_backspace_removes_char_and_updates() {
        let mut app = make_app_with_files();
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('a')).unwrap();
        app.update(Action::SearchInput('l')).unwrap();

        assert_eq!(app.browser_search_query, "al");

        app.update(Action::SearchBackspace).unwrap();

        assert_eq!(app.browser_search_query, "a");
    }

    #[test]
    fn browser_search_commit_exits_input_mode_keeps_query() {
        let mut app = make_app_with_files();
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('a')).unwrap();

        app.update(Action::SearchCommit).unwrap();

        assert!(!app.browser_search_active);
        assert_eq!(app.browser_search_query, "a");
    }

    #[test]
    fn browser_search_cancel_restores_selection() {
        let mut app = make_app_with_files();
        app.browser_selected = 2;
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('b')).unwrap(); // Moves to "banana" at index 1

        app.update(Action::SearchCancel).unwrap();

        assert!(!app.browser_search_active);
        assert!(app.browser_search_query.is_empty());
        assert_eq!(app.browser_selected, 2); // Restored to original
    }

    #[test]
    fn browser_search_next_finds_next_match() {
        let mut app = make_app_with_files();
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('a')).unwrap(); // Moves to "apple" at index 0
        assert_eq!(app.browser_selected, 0);
        app.update(Action::SearchCommit).unwrap();

        // Matches for 'a': 0 "apple", 1 "banana", 3 "date"
        // Next after 0 should be 1
        app.update(Action::SearchNext).unwrap();

        assert_eq!(app.browser_selected, 1);
    }

    #[test]
    fn browser_search_prev_finds_previous_match() {
        let mut app = make_app_with_files();
        app.browser_selected = 3; // Start at "date"
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('a')).unwrap(); // "date" contains 'a', stays at 3
        app.update(Action::SearchCommit).unwrap();

        // Matches for 'a': 0 "apple", 1 "banana", 3 "date"
        // Prev from 3 → 2 (no match) → 1 (match)
        app.update(Action::SearchPrev).unwrap();

        assert_eq!(app.browser_selected, 1);
    }

    #[test]
    fn browser_search_wraps_around() {
        let mut app = make_app_with_files();
        app.update(Action::SearchStart).unwrap();
        app.update(Action::SearchInput('a')).unwrap();

        assert_eq!(app.browser_selected, 0);
        app.update(Action::SearchCommit).unwrap();

        app.update(Action::SearchNext).unwrap();
        assert_eq!(app.browser_selected, 1); // "banana"

        app.update(Action::SearchNext).unwrap();
        assert_eq!(app.browser_selected, 3); // "date"

        app.update(Action::SearchNext).unwrap();
        assert_eq!(app.browser_selected, 0); // wraps to "apple"
    }

    #[test]
    fn browser_has_committed_search_returns_true() {
        let mut app = make_app();
        app.browser_search_query = "test".to_string();
        app.browser_search_active = false;

        assert!(app.browser_has_committed_search());
    }

    #[test]
    fn browser_has_committed_search_returns_false_when_input_active() {
        let mut app = make_app();
        app.browser_search_query = "test".to_string();
        app.browser_search_active = true;

        assert!(!app.browser_has_committed_search());
    }

    #[test]
    fn browser_has_committed_search_returns_false_when_empty() {
        let mut app = make_app();
        app.browser_search_query.clear();
        app.browser_search_active = false;

        assert!(!app.browser_has_committed_search());
    }
}
