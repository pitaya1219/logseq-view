use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

pub trait GraphSource {
    fn children(&self, dir: &Path) -> anyhow::Result<Vec<Entry>>;
    fn read(&self, path: &Path) -> anyhow::Result<String>;
    /// Writes `contents` to `path`, overwriting any existing content. Keeps
    /// fs access on the port rather than in `main.rs`, so writing (e.g. the
    /// block-edit flow's splice result) stays testable via `FakeGraphSource`.
    fn write(&self, path: &Path, contents: &str) -> anyhow::Result<()>;
    /// Recursively lists every markdown file under `root`, applying the same
    /// dotfile/`bak`/extension filtering as `children`. Unlike `children`
    /// (one level, driven by the browser's lazy expand/collapse), this walks
    /// the whole subtree in one call -- for whole-graph operations like the
    /// browser content filter, which can't rely on which directories happen
    /// to be expanded in the tree.
    fn all_files(&self, root: &Path) -> anyhow::Result<Vec<PathBuf>>;
}

/// URL decode a string, handling percent-encoded characters
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

/// Implementation of GraphSource using walkdir to read from the filesystem
pub struct WalkdirGraphSource;

impl WalkdirGraphSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WalkdirGraphSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphSource for WalkdirGraphSource {
    fn children(&self, dir: &Path) -> anyhow::Result<Vec<Entry>> {
        let mut entries = Vec::new();

        let is_journals_dir = dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "journals")
            .unwrap_or(false);

        for entry in walkdir::WalkDir::new(dir)
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
        {
            let path = entry.path().to_path_buf();
            let is_dir = entry.file_type().is_dir();
            let is_md = path.extension().is_some_and(|e| e == "md");

            if !is_dir && !is_md {
                continue;
            }

            let name = url_decode(
                &path
                    .file_stem()
                    .unwrap_or(entry.file_name())
                    .to_string_lossy(),
            );

            entries.push(Entry { path, name, is_dir });
        }

        if is_journals_dir {
            entries.sort_by(|a, b| b.name.cmp(&a.name));
        }

        Ok(entries)
    }

    fn read(&self, path: &Path) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(path)?;
        Ok(content)
    }

    fn write(&self, path: &Path, contents: &str) -> anyhow::Result<()> {
        std::fs::write(path, contents)?;
        Ok(())
    }

    fn all_files(&self, root: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for entry in walkdir::WalkDir::new(root)
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
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path().to_path_buf();
            if path.extension().is_some_and(|e| e == "md") {
                files.push(path);
            }
        }

        Ok(files)
    }
}

/// In-memory implementation of GraphSource for testing
pub struct FakeGraphSource {
    // `RefCell` so `write` (part of the `GraphSource` trait, `&self`) can
    // mutate the in-memory file map like a real fs write would, while
    // `read`/`children` stay simple immutable borrows.
    files: RefCell<HashMap<PathBuf, String>>,
    dirs: HashMap<PathBuf, Vec<PathBuf>>,
}

impl FakeGraphSource {
    pub fn new() -> Self {
        Self {
            files: RefCell::new(HashMap::new()),
            dirs: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, path: PathBuf, content: &str) {
        self.files.get_mut().insert(path, content.to_string());
    }

    pub fn add_dir(&mut self, path: PathBuf, children: Vec<PathBuf>) {
        self.dirs.insert(path, children);
    }

    pub fn add_dir_entries(&mut self, path: PathBuf, entries: Vec<(PathBuf, bool, &str)>) {
        let children: Vec<PathBuf> = entries.iter().map(|(p, _, _)| p.clone()).collect();
        self.dirs.insert(path, children);

        for (child_path, is_dir, content) in entries {
            if !is_dir {
                self.files.get_mut().insert(child_path, content.to_string());
            }
        }
    }

    fn collect_all_files(&self, dir: &Path, out: &mut Vec<PathBuf>) {
        let Some(children) = self.dirs.get(dir) else {
            return;
        };
        for child in children {
            if self.dirs.contains_key(child) {
                self.collect_all_files(child, out);
            } else {
                out.push(child.clone());
            }
        }
    }
}

impl Default for FakeGraphSource {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphSource for FakeGraphSource {
    fn children(&self, dir: &Path) -> anyhow::Result<Vec<Entry>> {
        let mut entries = Vec::new();

        if let Some(children) = self.dirs.get(dir) {
            for child_path in children {
                let is_dir = self.dirs.contains_key(child_path);
                let name = url_decode(
                    &child_path
                        .file_stem()
                        .unwrap_or(child_path.file_name().unwrap_or_default())
                        .to_string_lossy(),
                );

                entries.push(Entry {
                    path: child_path.clone(),
                    name,
                    is_dir,
                });
            }
        }

        let is_journals_dir = dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "journals")
            .unwrap_or(false);

        if is_journals_dir {
            entries.sort_by(|a, b| b.name.cmp(&a.name));
        } else {
            entries.sort_by(|a, b| a.name.cmp(&b.name));
        }

        Ok(entries)
    }

    fn read(&self, path: &Path) -> anyhow::Result<String> {
        self.files
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", path.display()))
    }

    fn write(&self, path: &Path, contents: &str) -> anyhow::Result<()> {
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), contents.to_string());
        Ok(())
    }

    fn all_files(&self, root: &Path) -> anyhow::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        self.collect_all_files(root, &mut files);
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_journals_dir_sorts_descending() {
        let mut source = FakeGraphSource::new();

        let journals_dir = PathBuf::from("/journals");
        let entries = vec![
            (PathBuf::from("/journals/2026_01_01.md"), false, ""),
            (PathBuf::from("/journals/2026_06_30.md"), false, ""),
            (PathBuf::from("/journals/2026_03_15.md"), false, ""),
        ];

        source.add_dir_entries(journals_dir.clone(), entries);

        let children = source.children(&journals_dir).unwrap();
        let names: Vec<&str> = children.iter().map(|e| e.name.as_str()).collect();

        assert_eq!(names, vec!["2026_06_30", "2026_03_15", "2026_01_01"]);
    }

    #[test]
    fn test_pages_dir_sorts_ascending() {
        let mut source = FakeGraphSource::new();

        let pages_dir = PathBuf::from("/pages");
        let entries = vec![
            (PathBuf::from("/pages/zzz.md"), false, ""),
            (PathBuf::from("/pages/aaa.md"), false, ""),
            (PathBuf::from("/pages/mmm.md"), false, ""),
        ];

        source.add_dir_entries(pages_dir.clone(), entries);

        let children = source.children(&pages_dir).unwrap();
        let names: Vec<&str> = children.iter().map(|e| e.name.as_str()).collect();

        assert_eq!(names, vec!["aaa", "mmm", "zzz"]);
    }

    #[test]
    fn test_non_journals_dir_sorts_ascending() {
        let mut source = FakeGraphSource::new();

        let custom_dir = PathBuf::from("/custom");
        let entries = vec![
            (PathBuf::from("/custom/file_c.md"), false, ""),
            (PathBuf::from("/custom/file_a.md"), false, ""),
            (PathBuf::from("/custom/file_b.md"), false, ""),
        ];

        source.add_dir_entries(custom_dir.clone(), entries);

        let children = source.children(&custom_dir).unwrap();
        let names: Vec<&str> = children.iter().map(|e| e.name.as_str()).collect();

        assert_eq!(names, vec!["file_a", "file_b", "file_c"]);
    }

    // --- write ---

    #[test]
    fn fake_graph_source_write_then_read_round_trips() {
        let mut source = FakeGraphSource::new();
        let path = PathBuf::from("/graph/pages/foo.md");
        source.add_file(path.clone(), "original content");

        source.write(&path, "new content").unwrap();

        assert_eq!(source.read(&path).unwrap(), "new content");
    }

    #[test]
    fn fake_graph_source_write_creates_new_file() {
        let source = FakeGraphSource::new();
        let path = PathBuf::from("/graph/pages/new.md");

        source.write(&path, "hello").unwrap();

        assert_eq!(source.read(&path).unwrap(), "hello");
    }

    // --- all_files ---

    #[test]
    fn all_files_recurses_into_nested_dirs() {
        let root = PathBuf::from("/graph");
        let pages = root.join("pages");
        let nested = pages.join("sub");

        let mut source = FakeGraphSource::new();
        source.add_dir_entries(
            root.clone(),
            vec![
                (pages.clone(), true, ""),
                (root.join("readme.md"), false, ""),
            ],
        );
        source.add_dir_entries(
            pages.clone(),
            vec![
                (pages.join("apple.md"), false, ""),
                (nested.clone(), true, ""),
            ],
        );
        source.add_dir_entries(nested.clone(), vec![(nested.join("child.md"), false, "")]);

        let mut files = source.all_files(&root).unwrap();
        files.sort();

        assert_eq!(
            files,
            vec![
                pages.join("apple.md"),
                nested.join("child.md"),
                root.join("readme.md"),
            ]
        );
    }

    #[test]
    fn all_files_empty_root_returns_empty() {
        let source = FakeGraphSource::new();
        let files = source.all_files(&PathBuf::from("/graph")).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn walkdir_all_files_recursively_finds_md_files_skipping_dot_and_bak() {
        let root =
            std::env::temp_dir().join(format!("logseq-view-all-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("pages/sub")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(root.join("bak")).unwrap();
        std::fs::write(root.join("pages/top.md"), "top").unwrap();
        std::fs::write(root.join("pages/sub/nested.md"), "nested").unwrap();
        std::fs::write(root.join("pages/notes.txt"), "not markdown").unwrap();
        std::fs::write(root.join(".hidden/secret.md"), "hidden").unwrap();
        std::fs::write(root.join("bak/old.md"), "old").unwrap();

        let source = WalkdirGraphSource::new();
        let mut files: Vec<String> = source
            .all_files(&root)
            .unwrap()
            .into_iter()
            .map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        files.sort();

        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(files, vec!["pages/sub/nested.md", "pages/top.md"]);
    }
}
