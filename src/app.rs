use crate::parser::{parse_file, ParsedLine};
use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn url_decode(s: &str) -> String {
    let mut buf: Vec<u8> = Vec::with_capacity(s.len());
    let src = s.as_bytes();
    let mut i = 0;
    while i < src.len() {
        if src[i] == b'%' && i + 2 < src.len() {
            let hi = (src[i + 1] as char).to_digit(16);
            let lo = (src[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                buf.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        buf.push(src[i]);
        i += 1;
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn walk_dir(dir: &Path) -> Vec<walkdir::DirEntry> {
    WalkDir::new(dir)
        .max_depth(1)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && name != "bak"
        })
        .flatten()
        .filter(|e| e.depth() > 0)
        .collect()
}

fn make_file_item(entry: &walkdir::DirEntry, depth: usize) -> Option<FileItem> {
    let path = entry.path().to_path_buf();
    let is_dir = entry.file_type().is_dir();
    let is_md = path.extension().is_some_and(|e| e == "md");

    if !is_dir && !is_md {
        return None;
    }

    let name = url_decode(
        &path
            .file_stem()
            .unwrap_or(entry.file_name())
            .to_string_lossy(),
    );

    Some(FileItem {
        path,
        name,
        depth,
        is_dir,
        is_expanded: false,
    })
}

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

pub struct App {
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
}

impl App {
    pub fn new(graph_path: PathBuf) -> Result<Self> {
        let mut app = App {
            graph_path,
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
        };
        app.build_file_tree()?;
        Ok(app)
    }

    pub fn build_file_tree(&mut self) -> Result<()> {
        self.file_items.clear();

        // Load only immediate children of the graph root; expand on demand
        for entry in walk_dir(&self.graph_path) {
            if let Some(item) = make_file_item(&entry, 0) {
                self.file_items.push(item);
            }
        }

        Ok(())
    }

    pub fn open_selected(&mut self) -> Result<()> {
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

    fn collapse_dir(&mut self, idx: usize) {
        let depth = self.file_items[idx].depth;
        let mut end = idx + 1;
        while end < self.file_items.len() && self.file_items[end].depth > depth {
            end += 1;
        }
        self.file_items.drain(idx + 1..end);
    }

    fn expand_dir(&mut self, parent_idx: usize, dir: &Path) -> Result<()> {
        let parent_depth = self.file_items[parent_idx].depth;
        let mut new_items = Vec::new();

        for entry in walk_dir(dir) {
            if let Some(item) = make_file_item(&entry, parent_depth + 1) {
                new_items.push(item);
            }
        }

        let insert_at = parent_idx + 1;
        for (i, item) in new_items.into_iter().enumerate() {
            self.file_items.insert(insert_at + i, item);
        }

        Ok(())
    }

    pub fn load_file(&mut self, path: &Path) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        self.content_lines = parse_file(&content);
        self.current_file = Some(path.to_path_buf());
        self.content_scroll = 0;
        Ok(())
    }

    // --- Navigation ---

    pub fn collapse_or_jump_parent(&mut self) {
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

    pub fn browser_down(&mut self) {
        if self.browser_selected + 1 < self.file_items.len() {
            self.browser_selected += 1;
        }
    }

    pub fn browser_up(&mut self) {
        if self.browser_selected > 0 {
            self.browser_selected -= 1;
        }
    }

    pub fn content_down(&mut self, amount: usize) {
        let max = self.content_lines.len().saturating_sub(1);
        self.content_scroll = (self.content_scroll + amount).min(max);
    }

    pub fn content_up(&mut self, amount: usize) {
        self.content_scroll = self.content_scroll.saturating_sub(amount);
    }

    pub fn content_top(&mut self) {
        self.content_scroll = 0;
    }

    pub fn content_bottom(&mut self) {
        self.content_scroll = self.content_lines.len().saturating_sub(1);
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

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Content,
            Focus::Content => Focus::Browser,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::ParsedLine;
    use std::fs;

    fn make_app() -> App {
        App {
            graph_path: PathBuf::new(),
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
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

    // --- File tree tests ---

    #[test]
    fn build_file_tree_includes_dirs_and_markdown() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create test structure
        fs::write(temp_path.join("note.md"), "# Note").unwrap();
        fs::write(temp_path.join("ignore.txt"), "ignored").unwrap();
        fs::create_dir(temp_path.join("subfolder")).unwrap();
        fs::write(temp_path.join(".hidden"), "hidden file").unwrap();
        fs::write(temp_path.join("bak"), "backup file").unwrap();

        let mut app = App {
            graph_path: temp_path.to_path_buf(),
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
        };

        app.build_file_tree().unwrap();

        // Should include .md file and directory, but not .txt, .hidden, or bak
        let names: Vec<_> = app.file_items.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"note"));
        assert!(names.contains(&"subfolder"));
        assert!(!names.contains(&"ignore"));
        assert!(!names.contains(&".hidden"));
        assert!(!names.contains(&"bak"));
    }

    #[test]
    fn expand_dir_inserts_children_with_correct_depth() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let temp_path = temp_dir.path();

        // Create nested structure
        let subdir = temp_path.join("dir1");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("child.md"), "content").unwrap();

        let mut app = App {
            graph_path: temp_path.to_path_buf(),
            focus: Focus::Browser,
            file_items: Vec::new(),
            browser_selected: 0,
            browser_offset: 0,
            current_file: None,
            content_lines: Vec::new(),
            content_scroll: 0,
        };

        app.build_file_tree().unwrap();
        assert_eq!(app.file_items.len(), 1);
        assert_eq!(app.file_items[0].name, "dir1");
        assert_eq!(app.file_items[0].depth, 0);

        // Expand the directory
        app.expand_dir(0, &subdir).unwrap();

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
}
