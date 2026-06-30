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

        Ok(entries)
    }

    fn read(&self, path: &Path) -> anyhow::Result<String> {
        let content = std::fs::read_to_string(path)?;
        Ok(content)
    }
}

/// In-memory implementation of GraphSource for testing
pub struct FakeGraphSource {
    files: HashMap<PathBuf, String>,
    dirs: HashMap<PathBuf, Vec<PathBuf>>,
}

impl FakeGraphSource {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            dirs: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, path: PathBuf, content: &str) {
        self.files.insert(path, content.to_string());
    }

    pub fn add_dir(&mut self, path: PathBuf, children: Vec<PathBuf>) {
        self.dirs.insert(path, children);
    }

    pub fn add_dir_entries(&mut self, path: PathBuf, entries: Vec<(PathBuf, bool, &str)>) {
        let children: Vec<PathBuf> = entries.iter().map(|(p, _, _)| p.clone()).collect();
        self.dirs.insert(path, children);

        for (child_path, is_dir, content) in entries {
            if !is_dir {
                self.files.insert(child_path, content.to_string());
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

        // Sort by name for consistent ordering
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(entries)
    }

    fn read(&self, path: &Path) -> anyhow::Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", path.display()))
    }
}
