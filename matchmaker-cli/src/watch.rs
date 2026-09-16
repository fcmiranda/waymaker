use std::path::Path;
use matchmaker::utils::markdown::MarkdownOptions;

/// Check the current terminal width, falling back to Crossterm terminal size.
pub fn get_terminal_width() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .filter(|&w| w > 0)
        .or_else(|| {
            ratatui::crossterm::terminal::size()
                .map(|(w, _)| w as usize)
                .ok()
        })
}

/// Reset terminal styling and ensure cursor is visible.
pub fn reset_terminal() {
    let mut stdout = std::io::stdout().lock();
    use std::io::Write;
    let _ = write!(stdout, "\x1b[0m\x1b[?25h\n");
    let _ = stdout.flush();
}

/// Render Markdown file to an ANSI string.
pub fn render_markdown_content(
    path: &Path,
    opts: &mut MarkdownOptions,
    dynamic_width: bool,
) -> String {
    if dynamic_width {
        opts.max_width = get_terminal_width();
    }

    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => format!("Error reading {}: {}", path.display(), e),
    };

    matchmaker::utils::markdown::render_markdown_ansi(&content, opts)
}

/// Clear screen smoothly (\x1b[2J\x1b[H) and flush rendered Markdown string.
pub fn render_and_flush(
    path: &Path,
    opts: &mut MarkdownOptions,
    dynamic_width: bool,
    render_tx: &Option<tokio::sync::mpsc::UnboundedSender<()>>,
) {
    let ansi = render_markdown_content(path, opts, dynamic_width);

    let mut stdout = std::io::stdout().lock();
    use std::io::Write;
    let _ = write!(stdout, "\x1b[2J\x1b[H");
    if ansi.ends_with('\n') {
        let _ = write!(stdout, "{ansi}");
    } else {
        let _ = writeln!(stdout, "{ansi}");
    }
    let _ = stdout.flush();

    if let Some(tx) = render_tx {
        let _ = tx.send(());
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, RawFd};
    use std::path::Path;

    pub struct InotifyFd(pub RawFd);

    impl AsRawFd for InotifyFd {
        fn as_raw_fd(&self) -> RawFd {
            self.0
        }
    }

    impl Drop for InotifyFd {
        fn drop(&mut self) {
            if self.0 >= 0 {
                unsafe { libc::close(self.0) };
            }
        }
    }

    pub struct LinuxInotifyWatcher {
        async_fd: tokio::io::unix::AsyncFd<InotifyFd>,
        file_c_str: std::ffi::CString,
        file_name: std::ffi::OsString,
        file_wd: libc::c_int,
        dir_wd: libc::c_int,
        file_mask: u32,
    }

    impl LinuxInotifyWatcher {
        pub fn new(path: &Path) -> anyhow::Result<Self> {
            let abs_path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };

            let file_name = abs_path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("Path has no filename: {}", abs_path.display()))?
                .to_os_string();

            let parent_dir = abs_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("Path has no parent directory: {}", abs_path.display()))?;

            let file_c_str = std::ffi::CString::new(abs_path.as_os_str().as_bytes())?;
            let parent_c_str = std::ffi::CString::new(parent_dir.as_os_str().as_bytes())?;

            let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
            if fd < 0 {
                return Err(anyhow::anyhow!(
                    "Failed to initialize inotify: {}",
                    std::io::Error::last_os_error()
                ));
            }

            let dir_mask = (libc::IN_CLOSE_WRITE
                | libc::IN_MOVED_TO
                | libc::IN_MODIFY
                | libc::IN_ATTRIB
                | libc::IN_CREATE) as u32;

            let file_mask = (libc::IN_CLOSE_WRITE
                | libc::IN_MODIFY
                | libc::IN_ATTRIB
                | libc::IN_MOVE_SELF
                | libc::IN_DELETE_SELF) as u32;

            let dir_wd = unsafe { libc::inotify_add_watch(fd, parent_c_str.as_ptr(), dir_mask) };
            if dir_wd < 0 {
                unsafe { libc::close(fd) };
                return Err(anyhow::anyhow!(
                    "Failed to add inotify watch on parent directory '{}': {}",
                    parent_dir.display(),
                    std::io::Error::last_os_error()
                ));
            }

            let file_wd = unsafe { libc::inotify_add_watch(fd, file_c_str.as_ptr(), file_mask) };

            let async_fd = tokio::io::unix::AsyncFd::new(InotifyFd(fd))?;

            Ok(Self {
                async_fd,
                file_c_str,
                file_name,
                file_wd,
                dir_wd,
                file_mask,
            })
        }

        fn fd(&self) -> RawFd {
            self.async_fd.get_ref().0
        }

        pub fn drain_events(&self) -> bool {
            let mut target_changed = false;
            let mut buf = [0u8; 4096];

            loop {
                let n = unsafe {
                    libc::read(
                        self.fd(),
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                    )
                };

                if n < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.raw_os_error() == Some(libc::EAGAIN)
                        || err.raw_os_error() == Some(libc::EWOULDBLOCK)
                    {
                        break;
                    }
                    break;
                }
                if n == 0 {
                    break;
                }

                let mut offset = 0;
                let total = n as usize;
                while offset + std::mem::size_of::<libc::inotify_event>() <= total {
                    let event: libc::inotify_event = unsafe {
                        std::ptr::read_unaligned(
                            buf[offset..].as_ptr() as *const libc::inotify_event
                        )
                    };
                    let event_len = event.len as usize;
                    let next_offset = offset + std::mem::size_of::<libc::inotify_event>() + event_len;
                    if next_offset > total {
                        break;
                    }

                    if (event.mask & libc::IN_Q_OVERFLOW as u32) != 0 {
                        target_changed = true;
                    } else if event.wd == self.file_wd {
                        target_changed = true;
                    } else if event.wd == self.dir_wd && event_len > 0 {
                        let name_start = offset + std::mem::size_of::<libc::inotify_event>();
                        let name_bytes = &buf[name_start..name_start + event_len];
                        let nul_pos = name_bytes
                            .iter()
                            .position(|&b| b == 0)
                            .unwrap_or(name_bytes.len());
                        let event_name = std::ffi::OsStr::from_bytes(&name_bytes[..nul_pos]);
                        if event_name == self.file_name {
                            target_changed = true;
                        }
                    }

                    offset = next_offset;
                }
            }

            target_changed
        }

        pub fn refresh_file_watch(&mut self) {
            let new_wd = unsafe {
                libc::inotify_add_watch(self.fd(), self.file_c_str.as_ptr(), self.file_mask)
            };
            if new_wd >= 0 {
                self.file_wd = new_wd;
            }
        }

        pub async fn wait_for_change(&mut self) -> anyhow::Result<()> {
            loop {
                let mut guard = self.async_fd.readable().await?;
                let matched = self.drain_events();
                guard.clear_ready();

                if matched {
                    // Debounce window (30ms-50ms) to coalesce atomic editor save operations (write -> rename -> chmod)
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    let _ = self.drain_events();
                    self.refresh_file_watch();
                    return Ok(());
                }
            }
        }
    }
}

mod polling {
    use std::path::{Path, PathBuf};

    pub struct PollingWatcher {
        path: PathBuf,
        last_mtime: Option<std::time::SystemTime>,
        last_len: u64,
    }

    impl PollingWatcher {
        pub fn new(path: &Path) -> anyhow::Result<Self> {
            let (last_mtime, last_len) = match std::fs::metadata(path) {
                Ok(m) => (m.modified().ok(), m.len()),
                Err(_) => (None, 0),
            };
            Ok(Self {
                path: path.to_path_buf(),
                last_mtime,
                last_len,
            })
        }

        pub async fn wait_for_change(&mut self) -> anyhow::Result<()> {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                let (mtime, len) = match std::fs::metadata(&self.path) {
                    Ok(m) => (m.modified().ok(), m.len()),
                    Err(_) => (None, 0),
                };
                if (mtime, len) != (self.last_mtime, self.last_len) {
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    if let Ok(m) = std::fs::metadata(&self.path) {
                        self.last_mtime = m.modified().ok();
                        self.last_len = m.len();
                    }
                    return Ok(());
                }
            }
        }
    }
}

pub enum FileWatcher {
    #[cfg(target_os = "linux")]
    Linux(linux::LinuxInotifyWatcher),
    Polling(polling::PollingWatcher),
}

impl FileWatcher {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        #[cfg(target_os = "linux")]
        {
            match linux::LinuxInotifyWatcher::new(path) {
                Ok(watcher) => return Ok(Self::Linux(watcher)),
                Err(e) => {
                    log::warn!("Inotify initialization failed, falling back to polling: {e}");
                }
            }
        }
        Ok(Self::Polling(polling::PollingWatcher::new(path)?))
    }

    pub async fn wait_for_change(&mut self) -> anyhow::Result<()> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Linux(w) => w.wait_for_change().await,
            Self::Polling(w) => w.wait_for_change().await,
        }
    }
}

/// Watch a Markdown file and live-reload on disk changes.
pub async fn watch_markdown_with_controls(
    path: &Path,
    mut opts: MarkdownOptions,
    dynamic_width: bool,
    mut cancel_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    render_tx: Option<tokio::sync::mpsc::UnboundedSender<()>>,
) -> anyhow::Result<()> {
    let mut watcher = FileWatcher::new(path)?;

    #[cfg(unix)]
    let mut sigwinch =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change()).ok();

    // Initial render before blocking on events
    render_and_flush(path, &mut opts, dynamic_width, &render_tx);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                log::debug!("Received SIGINT (Ctrl+C), exiting watch mode cleanly");
                break;
            }
            res = async {
                if let Some(ref mut rx) = cancel_rx {
                    rx.await.ok()
                } else {
                    std::future::pending::<Option<()>>().await
                }
            } => {
                if res.is_some() {
                    break;
                }
            }
            _ = async {
                #[cfg(unix)]
                if let Some(ref mut sw) = sigwinch {
                    sw.recv().await
                } else {
                    std::future::pending::<Option<()>>().await
                }
                #[cfg(not(unix))]
                std::future::pending::<Option<()>>().await
            } => {
                tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                render_and_flush(path, &mut opts, dynamic_width, &render_tx);
            }
            change = watcher.wait_for_change() => {
                if let Err(e) = change {
                    log::error!("Watcher error: {e}");
                    break;
                }
                render_and_flush(path, &mut opts, dynamic_width, &render_tx);
            }
        }
    }

    reset_terminal();
    Ok(())
}

/// Default entrypoint for live-reload markdown watching.
pub async fn watch_markdown(
    path: &Path,
    opts: MarkdownOptions,
    dynamic_width: bool,
) -> anyhow::Result<()> {
    watch_markdown_with_controls(path, opts, dynamic_width, None, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_watch_direct_write_and_cancel() {
        let temp_dir = std::env::temp_dir().join("mm_test_watch_direct");
        let _ = std::fs::create_dir_all(&temp_dir);
        let md_path = temp_dir.join("test_watch.md");
        std::fs::write(&md_path, "# Initial Header\n\nInitial content").unwrap();

        let opts = MarkdownOptions {
            max_width: Some(60),
            base_path: Some(temp_dir.clone()),
            render_mermaid: false,
            mermaid_ascii: true,
            show_line_numbers: false,
            mermaid_image: false,
            inline_diagrams: false,
            inline_images: false,
            diagram_theme: matchmaker::config::DiagramTheme::Auto,
            diagram_background: matchmaker::config::DiagramBackground::Transparent,
        };

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (render_tx, mut render_rx) = tokio::sync::mpsc::unbounded_channel();

        let md_path_clone = md_path.clone();
        let handle = tokio::spawn(async move {
            watch_markdown_with_controls(
                &md_path_clone,
                opts,
                false,
                Some(cancel_rx),
                Some(render_tx),
            )
            .await
        });

        // Wait for initial render
        let init = tokio::time::timeout(std::time::Duration::from_millis(500), render_rx.recv())
            .await
            .expect("timeout waiting for initial render")
            .expect("render channel closed");
        assert_eq!(init, ());

        // Now modify the file directly
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        std::fs::write(&md_path, "# Updated Header\n\nNew updated content").unwrap();

        // Expect second render from file watch
        let update = tokio::time::timeout(std::time::Duration::from_millis(1500), render_rx.recv())
            .await
            .expect("timeout waiting for live reload render")
            .expect("render channel closed");
        assert_eq!(update, ());

        // Send cancel signal
        let _ = cancel_tx.send(());
        let res = handle.await.expect("task join failed");
        assert!(res.is_ok());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_watch_atomic_rename_save() {
        let temp_dir = std::env::temp_dir().join("mm_test_watch_atomic");
        let _ = std::fs::create_dir_all(&temp_dir);
        let md_path = temp_dir.join("test_atomic.md");
        std::fs::write(&md_path, "# V1\n").unwrap();

        let opts = MarkdownOptions {
            max_width: Some(60),
            base_path: Some(temp_dir.clone()),
            render_mermaid: false,
            mermaid_ascii: true,
            show_line_numbers: false,
            mermaid_image: false,
            inline_diagrams: false,
            inline_images: false,
            diagram_theme: matchmaker::config::DiagramTheme::Auto,
            diagram_background: matchmaker::config::DiagramBackground::Transparent,
        };

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (render_tx, mut render_rx) = tokio::sync::mpsc::unbounded_channel();

        let md_path_clone = md_path.clone();
        let handle = tokio::spawn(async move {
            watch_markdown_with_controls(
                &md_path_clone,
                opts,
                false,
                Some(cancel_rx),
                Some(render_tx),
            )
            .await
        });

        // Wait for initial render
        render_rx.recv().await.unwrap();

        // Simulate editor atomic save: write temp file then rename over target
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let tmp_file = temp_dir.join("test_atomic.md.tmp");
        std::fs::write(&tmp_file, "# V2 via atomic rename\n").unwrap();
        std::fs::rename(&tmp_file, &md_path).unwrap();

        // Expect render
        let update = tokio::time::timeout(std::time::Duration::from_millis(1500), render_rx.recv())
            .await
            .expect("timeout waiting for atomic save live reload")
            .expect("render channel closed");
        assert_eq!(update, ());

        let _ = cancel_tx.send(());
        let res = handle.await.expect("task join failed");
        assert!(res.is_ok());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_watch_ignores_unrelated_files() {
        let temp_dir = std::env::temp_dir().join("mm_test_watch_unrelated");
        let _ = std::fs::create_dir_all(&temp_dir);
        let md_path = temp_dir.join("main.md");
        std::fs::write(&md_path, "# Main\n").unwrap();

        let opts = MarkdownOptions {
            max_width: Some(60),
            base_path: Some(temp_dir.clone()),
            render_mermaid: false,
            mermaid_ascii: true,
            show_line_numbers: false,
            mermaid_image: false,
            inline_diagrams: false,
            inline_images: false,
            diagram_theme: matchmaker::config::DiagramTheme::Auto,
            diagram_background: matchmaker::config::DiagramBackground::Transparent,
        };

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (render_tx, mut render_rx) = tokio::sync::mpsc::unbounded_channel();

        let md_path_clone = md_path.clone();
        let handle = tokio::spawn(async move {
            watch_markdown_with_controls(
                &md_path_clone,
                opts,
                false,
                Some(cancel_rx),
                Some(render_tx),
            )
            .await
        });

        // Initial render
        render_rx.recv().await.unwrap();

        // Touch an unrelated file in the same directory
        let unrelated = temp_dir.join("unrelated.txt");
        std::fs::write(&unrelated, "hello world").unwrap();

        // Verify no render event arrives for unrelated file
        let timeout_res =
            tokio::time::timeout(std::time::Duration::from_millis(200), render_rx.recv()).await;
        assert!(timeout_res.is_err(), "should timeout with no render for unrelated file");

        let _ = cancel_tx.send(());
        let _ = handle.await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[tokio::test]
    async fn test_watch_resilient_to_temporary_deletion() {
        let temp_dir = std::env::temp_dir().join("mm_test_watch_del_restore");
        let _ = std::fs::create_dir_all(&temp_dir);
        let md_path = temp_dir.join("ephemeral.md");
        std::fs::write(&md_path, "# Before Delete\n").unwrap();

        let opts = MarkdownOptions {
            max_width: Some(60),
            base_path: Some(temp_dir.clone()),
            render_mermaid: false,
            mermaid_ascii: true,
            show_line_numbers: false,
            mermaid_image: false,
            inline_diagrams: false,
            inline_images: false,
            diagram_theme: matchmaker::config::DiagramTheme::Auto,
            diagram_background: matchmaker::config::DiagramBackground::Transparent,
        };

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        let (render_tx, mut render_rx) = tokio::sync::mpsc::unbounded_channel();

        let md_path_clone = md_path.clone();
        let handle = tokio::spawn(async move {
            watch_markdown_with_controls(
                &md_path_clone,
                opts,
                false,
                Some(cancel_rx),
                Some(render_tx),
            )
            .await
        });

        // Initial render
        render_rx.recv().await.unwrap();

        // Delete file
        let _ = std::fs::remove_file(&md_path);
        // Wait a tiny bit and recreate
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        std::fs::write(&md_path, "# Recreated\n").unwrap();

        // Should receive render event for recreated file
        let update = tokio::time::timeout(std::time::Duration::from_millis(1500), render_rx.recv())
            .await
            .expect("timeout waiting for recreate live reload")
            .expect("render channel closed");
        assert_eq!(update, ());

        let _ = cancel_tx.send(());
        let _ = handle.await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_render_markdown_content_with_mermaid() {
        let temp_dir = std::env::temp_dir().join("mm_test_render_mermaid");
        let _ = std::fs::create_dir_all(&temp_dir);
        let md_path = temp_dir.join("mermaid.md");
        std::fs::write(
            &md_path,
            "# Architecture\n\n```mermaid\ngraph LR\n  A[Client] --> B[Server]\n```\n",
        )
        .unwrap();

        let mut opts = MarkdownOptions {
            max_width: Some(70),
            base_path: Some(temp_dir.clone()),
            render_mermaid: true,
            mermaid_ascii: true,
            show_line_numbers: false,
            mermaid_image: false,
            inline_diagrams: false,
            inline_images: false,
            diagram_theme: matchmaker::config::DiagramTheme::Auto,
            diagram_background: matchmaker::config::DiagramBackground::Transparent,
        };

        let output = render_markdown_content(&md_path, &mut opts, false);
        assert!(output.contains("Architecture"));
        assert!(output.contains("Client"));
        assert!(output.contains("Mermaid Diagram"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
