use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use ignore::{WalkBuilder, WalkState};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EntryType {
    Files,
    Directories,
    #[default]
    Any,
}

pub fn default_ignored_dirs() -> Vec<String> {
    vec![
        ".git".to_string(),
        ".cache".to_string(),
        ".local".to_string(),
        ".cargo".to_string(),
        ".rustup".to_string(),
        ".npm".to_string(),
        ".npm-global".to_string(),
        "node_modules".to_string(),
        "target".to_string(),
        ".venv".to_string(),
        "venv".to_string(),
        ".wine".to_string(),
        ".android".to_string(),
        ".gradle".to_string(),
        ".m2".to_string(),
    ]
}

#[derive(Debug, Clone)]
pub struct WalkerOptions {
    pub root: PathBuf,
    pub hidden: bool,
    pub ignore: bool,
    pub git_exclude: bool,
    pub git_global: bool,
    pub max_depth: Option<usize>,
    pub threads: usize,
    pub entry_type: EntryType,
    pub strip_cwd_prefix: bool,
    pub ignore_dirs: Vec<String>,
}

impl Default for WalkerOptions {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            hidden: true,
            ignore: true,
            git_exclude: true,
            git_global: true,
            max_depth: None,
            threads: thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            entry_type: EntryType::Any,
            strip_cwd_prefix: true,
            ignore_dirs: default_ignored_dirs(),
        }
    }
}

pub struct AsyncWalker {
    options: WalkerOptions,
}

impl AsyncWalker {
    pub fn new(options: WalkerOptions) -> Self {
        Self { options }
    }

    pub fn from_root(root: impl AsRef<Path>) -> Self {
        Self::new(WalkerOptions {
            root: root.as_ref().to_path_buf(),
            ..Default::default()
        })
    }

    pub fn builder(&self) -> WalkBuilder {
        let mut builder = WalkBuilder::new(&self.options.root);
        builder.hidden(!self.options.hidden);
        builder.git_ignore(self.options.ignore);
        builder.git_exclude(self.options.git_exclude);
        builder.git_global(self.options.git_global);
        builder.ignore(self.options.ignore);
        let ignored: std::sync::Arc<std::collections::HashSet<String>> = std::sync::Arc::new(
            self.options
                .ignore_dirs
                .iter()
                .map(|s| s.trim_end_matches('/').to_string())
                .collect(),
        );
        builder.filter_entry(move |entry| {
            if entry.depth() == 0 {
                return true;
            }
            let file_name = entry.file_name().to_string_lossy();
            !ignored.contains(file_name.as_ref())
        });
        if let Some(depth) = self.options.max_depth {
            builder.max_depth(Some(depth));
        }
        builder.threads(self.options.threads);
        builder
    }

    /// Spawns a parallel walk in a blocking Tokio task, streaming formatted paths to `push_fn`.
    pub fn spawn_walk<F>(&self, mut push_fn: F) -> tokio::task::JoinHandle<()>
    where
        F: FnMut(String) -> Result<(), crate::nucleo::WorkerError> + Send + 'static,
    {
        let options = self.options.clone();
        let builder = self.builder();

        tokio::task::spawn_blocking(move || {
            // Pass 1: Immediate shallow walk (max_depth = 1) to deliver top-level items on Frame 0
            let shallow_walker = builder.clone().max_depth(Some(1)).build();
            for result in shallow_walker {
                let Ok(entry) = result else { continue };
                if entry.depth() == 0 {
                    continue;
                }
                let ft = entry.file_type();
                let is_dir = ft.map_or(false, |t| t.is_dir());
                let is_file = ft.map_or(false, |t| t.is_file());
                match options.entry_type {
                    EntryType::Files if !is_file => continue,
                    EntryType::Directories if !is_dir => continue,
                    _ => {}
                }
                let formatted = format_path(
                    entry.path(),
                    &options.root,
                    options.strip_cwd_prefix,
                    is_dir,
                );
                if push_fn(formatted).is_err() {
                    return;
                }
            }

            // Pass 2: Deep parallel walk for rest of directory tree (depth > 1)
            let (tx, rx) = mpsc::channel::<String>();
            let walker = builder.build_parallel();
            let entry_type = options.entry_type.clone();
            let strip_cwd = options.strip_cwd_prefix;
            let root = options.root.clone();

            thread::spawn(move || {
                walker.run(move || {
                    let tx = tx.clone();
                    let entry_type = entry_type.clone();
                    let root = root.clone();

                    Box::new(move |result| {
                        let Ok(entry) = result else {
                            return WalkState::Continue;
                        };

                        // Skip root and depth 1 (already delivered in Pass 1 for instant frame 0)
                        if entry.depth() <= 1 {
                            return WalkState::Continue;
                        }

                        let ft = entry.file_type();
                        let is_dir = ft.map_or(false, |t| t.is_dir());
                        let is_file = ft.map_or(false, |t| t.is_file());

                        match entry_type {
                            EntryType::Files if !is_file => return WalkState::Continue,
                            EntryType::Directories if !is_dir => return WalkState::Continue,
                            _ => {}
                        }

                        let path = entry.path();
                        let formatted = format_path(path, &root, strip_cwd, is_dir);

                        let _ = tx.send(formatted);
                        WalkState::Continue
                    })
                });
            });

            while let Ok(line) = rx.recv() {
                if push_fn(line).is_err() {
                    break;
                }
            }
        })
    }

    /// Collects all matching paths synchronously into a `Vec<String>`.
    pub fn collect_sync(&self) -> Vec<String> {
        let mut results = Vec::new();
        let builder = self.builder();
        let walker = builder.build();

        for result in walker {
            let Ok(entry) = result else {
                continue;
            };

            if entry.depth() == 0 {
                continue;
            }

            let ft = entry.file_type();
            let is_dir = ft.map_or(false, |t| t.is_dir());
            let is_file = ft.map_or(false, |t| t.is_file());

            match self.options.entry_type {
                EntryType::Files if !is_file => continue,
                EntryType::Directories if !is_dir => continue,
                _ => {}
            }

            let path = entry.path();
            let formatted = format_path(
                path,
                &self.options.root,
                self.options.strip_cwd_prefix,
                is_dir,
            );
            results.push(formatted);
        }

        results.sort_by_key(|item| {
            let slashes = item.bytes().filter(|&b| b == b'/' || b == b'\\').count();
            (slashes, item.clone())
        });

        results
    }
}

fn format_path(path: &Path, root: &Path, strip_cwd: bool, is_dir: bool) -> String {
    let mut s = if strip_cwd {
        if let Ok(rel) = path.strip_prefix(root) {
            rel.to_string_lossy().to_string()
        } else if let Ok(rel) = path.strip_prefix(".") {
            rel.to_string_lossy().to_string()
        } else {
            path.to_string_lossy().to_string()
        }
    } else {
        path.to_string_lossy().to_string()
    };

    if is_dir && !s.ends_with('/') && !s.ends_with('\\') {
        s.push('/');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_walker_collect_sync() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_walker");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("src"))?;
        fs::write(temp_dir.join("src").join("main.rs"), "fn main() {}")?;
        fs::write(temp_dir.join("README.md"), "# Test")?;

        let options = WalkerOptions {
            root: temp_dir.clone(),
            strip_cwd_prefix: true,
            ..Default::default()
        };

        let walker = AsyncWalker::new(options);
        let items = walker.collect_sync();

        assert!(items.iter().any(|i| i == "src/" || i == "src"));
        assert!(items.iter().any(|i| i == "src/main.rs"));
        assert!(items.iter().any(|i| i == "README.md"));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_walker_entry_type_filter() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_walker_filter");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("sub"))?;
        fs::write(temp_dir.join("file.txt"), "hello")?;

        // Files only
        let options_files = WalkerOptions {
            root: temp_dir.clone(),
            entry_type: EntryType::Files,
            ..Default::default()
        };
        let items_files = AsyncWalker::new(options_files).collect_sync();
        assert!(items_files.iter().all(|i| !i.ends_with('/')));
        assert!(items_files.iter().any(|i| i == "file.txt"));

        // Directories only
        let options_dirs = WalkerOptions {
            root: temp_dir.clone(),
            entry_type: EntryType::Directories,
            ..Default::default()
        };
        let items_dirs = AsyncWalker::new(options_dirs).collect_sync();
        assert!(items_dirs.iter().any(|i| i.ends_with('/') || i == "sub"));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_walker_hidden_files() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_walker_hidden");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join(".config"))?;
        fs::write(temp_dir.join(".config").join("settings.toml"), "a = 1")?;
        fs::write(temp_dir.join(".zshrc"), "# zsh")?;
        fs::create_dir_all(temp_dir.join(".git"))?;
        fs::write(temp_dir.join(".git").join("HEAD"), "ref: refs/heads/main")?;

        let options = WalkerOptions {
            root: temp_dir.clone(),
            strip_cwd_prefix: true,
            ..Default::default()
        };

        let walker = AsyncWalker::new(options);
        let items = walker.collect_sync();

        // Should include hidden files like .config and .zshrc
        assert!(items.iter().any(|i| i == ".config/" || i == ".config"));
        assert!(items.iter().any(|i| i == ".config/settings.toml"));
        assert!(items.iter().any(|i| i == ".zshrc"));

        // Should exclude .git folder contents
        assert!(!items.iter().any(|i| i.contains(".git/HEAD")));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[tokio::test]
    async fn test_walker_spawn_walk_unique() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_walker_spawn_walk");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("src").join("sub"))?;
        fs::write(temp_dir.join("src").join("main.rs"), "fn main() {}")?;
        fs::write(
            temp_dir.join("src").join("sub").join("lib.rs"),
            "pub fn x() {}",
        )?;
        fs::write(temp_dir.join("README.md"), "# Test")?;

        let options = WalkerOptions {
            root: temp_dir.clone(),
            strip_cwd_prefix: true,
            ..Default::default()
        };

        let walker = AsyncWalker::new(options);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = walker.spawn_walk(move |line| {
            let _ = tx.send(line);
            Ok(())
        });
        handle.await?;

        let mut items = Vec::new();
        while let Ok(item) = rx.try_recv() {
            items.push(item);
        }

        // Verify all depth 1 and depth 2 items are present
        assert!(items.iter().any(|i| i == "src/" || i == "src"));
        assert!(items.iter().any(|i| i == "README.md"));
        assert!(items.iter().any(|i| i == "src/main.rs"));
        assert!(items.iter().any(|i| i == "src/sub/" || i == "src/sub"));
        assert!(items.iter().any(|i| i == "src/sub/lib.rs"));

        // Verify uniqueness (no duplicate entries between pass 1 and pass 2)
        let mut unique_items = items.clone();
        unique_items.sort();
        unique_items.dedup();
        assert_eq!(
            items.len(),
            unique_items.len(),
            "Items should be unique without duplicates"
        );

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_walker_default_ignored_dirs() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_walker_default_ignored");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join(".cache").join("fontconfig"))?;
        fs::write(temp_dir.join(".cache").join("test.bin"), "data")?;
        fs::create_dir_all(temp_dir.join(".cargo").join("registry"))?;
        fs::create_dir_all(temp_dir.join(".local").join("share"))?;
        fs::create_dir_all(temp_dir.join("node_modules").join("pkg"))?;
        fs::create_dir_all(temp_dir.join("target").join("debug"))?;
        fs::create_dir_all(temp_dir.join(".venv").join("lib"))?;
        fs::create_dir_all(temp_dir.join(".git"))?;
        fs::write(temp_dir.join(".git").join("config"), "git")?;

        // Normal files and directories that should be included
        fs::create_dir_all(temp_dir.join(".config").join("waymaker"))?;
        fs::write(temp_dir.join(".config").join("waymaker").join("config.toml"), "a=1")?;
        fs::create_dir_all(temp_dir.join("src"))?;
        fs::write(temp_dir.join("src").join("main.rs"), "fn main() {}")?;
        fs::write(temp_dir.join(".bashrc"), "# bashrc")?;

        let options = WalkerOptions {
            root: temp_dir.clone(),
            strip_cwd_prefix: true,
            ..Default::default()
        };

        let walker = AsyncWalker::new(options);
        let items = walker.collect_sync();

        // Normal directories and files MUST be present
        assert!(items.iter().any(|i| i == ".config/" || i == ".config"));
        assert!(items.iter().any(|i| i == ".config/waymaker/config.toml"));
        assert!(items.iter().any(|i| i == "src/" || i == "src"));
        assert!(items.iter().any(|i| i == "src/main.rs"));
        assert!(items.iter().any(|i| i == ".bashrc"));

        // Noise/cache directories MUST be pruned natively
        assert!(!items.iter().any(|i| i == ".cache/" || i == ".cache" || i.starts_with(".cache/")));
        assert!(!items.iter().any(|i| i == ".cargo/" || i == ".cargo" || i.starts_with(".cargo/")));
        assert!(!items.iter().any(|i| i == ".local/" || i == ".local" || i.starts_with(".local/")));
        assert!(!items.iter().any(|i| i == "node_modules/" || i == "node_modules" || i.starts_with("node_modules/")));
        assert!(!items.iter().any(|i| i == "target/" || i == "target" || i.starts_with("target/")));
        assert!(!items.iter().any(|i| i == ".venv/" || i == ".venv" || i.starts_with(".venv/")));
        assert!(!items.iter().any(|i| i == ".git/" || i == ".git" || i.starts_with(".git/")));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_walker_custom_ignore_dirs() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_walker_custom_ignored");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("custom_noise"))?;
        fs::write(temp_dir.join("custom_noise").join("data.txt"), "noise")?;
        fs::create_dir_all(temp_dir.join("target"))?;
        fs::write(temp_dir.join("target").join("build.txt"), "build")?;

        // Allow target by removing it from custom ignore_dirs
        let options = WalkerOptions {
            root: temp_dir.clone(),
            strip_cwd_prefix: true,
            ignore_dirs: vec!["custom_noise".to_string()],
            ..Default::default()
        };

        let walker = AsyncWalker::new(options);
        let items = walker.collect_sync();

        // custom_noise should be ignored
        assert!(!items.iter().any(|i| i.contains("custom_noise")));

        // target should now be included because it's not in ignore_dirs
        assert!(items.iter().any(|i| i == "target/" || i == "target"));
        assert!(items.iter().any(|i| i == "target/build.txt"));

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
