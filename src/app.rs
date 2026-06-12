use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use anyhow::Result;
use crate::parser::{parse_file, ParsedLine};

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
        let walker = WalkDir::new(&self.graph_path)
            .max_depth(1)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != "bak"
            });

        for entry in walker.flatten() {
            let path = entry.path().to_path_buf();
            let is_dir = entry.file_type().is_dir();
            let is_md = path.extension().map_or(false, |e| e == "md");

            if entry.depth() == 0 || (!is_dir && !is_md) {
                continue;
            }

            let name = url_decode(
                &path
                    .file_stem()
                    .unwrap_or(entry.file_name())
                    .to_string_lossy(),
            );

            self.file_items.push(FileItem {
                path,
                name,
                depth: 0,
                is_dir,
                is_expanded: false,
            });
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
                let depth = self.file_items[idx].depth;
                let mut end = idx + 1;
                while end < self.file_items.len() && self.file_items[end].depth > depth {
                    end += 1;
                }
                self.file_items.drain(idx + 1..end);
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

    fn expand_dir(&mut self, parent_idx: usize, dir: &Path) -> Result<()> {
        let parent_depth = self.file_items[parent_idx].depth;
        let mut new_items = Vec::new();

        let walker = WalkDir::new(dir)
            .max_depth(1)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != "bak"
            });

        for entry in walker.flatten() {
            if entry.depth() == 0 {
                continue;
            }
            let path = entry.path().to_path_buf();
            let is_dir = entry.file_type().is_dir();
            let is_md = path.extension().map_or(false, |e| e == "md");

            if !is_dir && !is_md {
                continue;
            }

            let name = url_decode(
                &path
                    .file_stem()
                    .unwrap_or(entry.file_name())
                    .to_string_lossy(),
            );

            new_items.push(FileItem {
                path,
                name,
                depth: parent_depth + 1,
                is_dir,
                is_expanded: false,
            });
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
            let depth = self.file_items[idx].depth;
            self.file_items[idx].is_expanded = false;
            let mut end = idx + 1;
            while end < self.file_items.len() && self.file_items[end].depth > depth {
                end += 1;
            }
            self.file_items.drain(idx + 1..end);
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

    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Browser => Focus::Content,
            Focus::Content => Focus::Browser,
        };
    }
}
