use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Check if current process is running inside a Tmux session.
pub fn in_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Query active Tmux sessions, filtering out internal utility sessions.
pub fn get_tmux_sessions() -> Vec<String> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| {
                !s.is_empty() && !s.starts_with("_popups") && !s.starts_with("_lazygitrs")
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Strip leading Nerd Font icon or status badge from a session/directory label.
pub fn strip_icon(s: &str) -> &str {
    let s = s.trim();
    // Common Nerd Font prefixes used by sesh and waymaker
    for prefix in [" ", " ", " ", "⚡ ", " ", " ", " "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return rest.trim();
        }
    }
    // Generic fallback: if starts with a non-ASCII char followed by space
    if let Some((first, rest)) = s.split_once(' ') {
        if first.chars().any(|c| !c.is_ascii()) {
            return rest.trim();
        }
    }
    s
}

/// Expand `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// Derive a deterministic, clean Tmux session name from a directory path.
/// Tmux forbids `.` in session names, so periods are replaced with underscores.
pub fn derive_session_name(path: &Path) -> String {
    let parent = path.parent();
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "session".to_string());

    if let Some(p) = parent {
        // If parent is a git worktree root (.bare) or dotfiles container (.dotfiles)
        if p.join(".bare").exists()
            || p.join(".git").exists()
            || p.file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false)
        {
            if let Some(p_name) = p.file_name() {
                let p_clean = p_name.to_string_lossy().replace(['.', ':'], "_");
                let f_clean = file_name.replace(['.', ':'], "_");
                return format!("{}/{}", p_clean, f_clean);
            }
        }
    }

    file_name.replace(['.', ':'], "_")
}

/// Query frecency store for directories, prioritizing pins and ranking by score.
pub fn get_frecency_dirs(query: Option<&str>) -> Vec<String> {
    let store = waymaker::frecency::FrecencyStore::open();
    let pins = store.list_pins();
    let pins_set = store.get_pins_set();
    let snapshot = store.get_snapshot();

    let mut results = Vec::new();
    let query_lower = query.map(|q| q.to_lowercase());

    // 1. Pinned paths first (if they exist as directories)
    for p in pins {
        if Path::new(&p).is_dir() {
            if let Some(ref q) = query_lower {
                if !p.to_lowercase().contains(q) {
                    continue;
                }
            }
            results.push(p);
        }
    }

    // 2. Frecency ranked directories
    let mut matches: Vec<(String, u32, usize)> = Vec::new();
    for (path, score) in snapshot.scores {
        if pins_set.contains(&path) {
            continue;
        }
        let p_obj = Path::new(&path);
        if !p_obj.is_dir() {
            continue;
        }
        if let Some(ref q) = query_lower {
            if !path.to_lowercase().contains(q) {
                continue;
            }
        }
        let depth = p_obj.components().count();
        matches.push((path, score, depth));
    }

    matches.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.2.cmp(&b.2)));
    for (path, _, _) in matches {
        results.push(path);
    }

    results
}

/// Connect to a target (either an existing Tmux session or a directory path).
pub fn connect(target: &str, switch_only: bool) -> anyhow::Result<()> {
    let clean_target = strip_icon(target);
    let expanded = expand_tilde(clean_target);

    let active_sessions = get_tmux_sessions();

    // 1. Direct match on an existing active Tmux session name
    if active_sessions.iter().any(|s| s == clean_target) {
        if in_tmux() {
            let status = Command::new("tmux")
                .args(["switch-client", "-t", clean_target])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to switch to tmux session '{}'", clean_target);
            }
        } else {
            let status = Command::new("tmux")
                .args(["attach-session", "-t", clean_target])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to attach to tmux session '{}'", clean_target);
            }
        }
        return Ok(());
    }

    // 2. Target is an existing directory path
    if expanded.is_dir() {
        let canonical = expanded.canonicalize().unwrap_or(expanded.clone());
        let session_name = derive_session_name(&canonical);
        let path_str = canonical.to_string_lossy().to_string();

        if !active_sessions.iter().any(|s| s == &session_name) {
            let status = Command::new("tmux")
                .args(["new-session", "-d", "-s", &session_name, "-c", &path_str])
                .status()?;
            if !status.success() {
                anyhow::bail!(
                    "Failed to create new tmux session '{}' at '{}'",
                    session_name,
                    path_str
                );
            }
        }

        if in_tmux() || switch_only {
            let status = Command::new("tmux")
                .args(["switch-client", "-t", &session_name])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to switch to tmux session '{}'", session_name);
            }
        } else {
            let status = Command::new("tmux")
                .args(["attach-session", "-t", &session_name])
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to attach to tmux session '{}'", session_name);
            }
        }

        // Record in Waymaker native frecency store
        let store = waymaker::frecency::FrecencyStore::open();
        let _ = store.add(&path_str);

        return Ok(());
    }

    // 3. Fallback: query frecency store for fuzzy match
    let candidates = get_frecency_dirs(Some(clean_target));
    if let Some(first_dir) = candidates.first() {
        return connect(first_dir, switch_only);
    }

    anyhow::bail!(
        "Target '{}' is neither an active tmux session nor a valid directory.",
        target
    );
}

/// Switch to the previous Tmux session (sesh last equivalent).
pub fn last() -> anyhow::Result<()> {
    let status = Command::new("tmux")
        .args(["switch-client", "-l"])
        .status()?;
    if !status.success() {
        anyhow::bail!("No previous tmux session to return to.");
    }
    Ok(())
}

/// List sessions and frecency directories with icons and filtering.
pub fn list(icons: bool, tmux_only: bool, dirs_only: bool) -> anyhow::Result<()> {
    let show_all = (tmux_only && dirs_only) || (!tmux_only && !dirs_only);
    let home = dirs::home_dir();

    // 1. Output active Tmux sessions
    if show_all || tmux_only {
        let sessions = get_tmux_sessions();
        for sess in sessions {
            if icons {
                println!(" {}", sess);
            } else {
                println!("{}", sess);
            }
        }
    }

    // 2. Output frecency directories
    if show_all || dirs_only {
        let dirs = get_frecency_dirs(None);
        for d in dirs {
            let display_path = if let Some(ref h) = home {
                if let Ok(rel) = Path::new(&d).strip_prefix(h) {
                    format!("~/{}", rel.display())
                } else {
                    d.clone()
                }
            } else {
                d.clone()
            };

            if icons {
                println!(" {}", display_path);
            } else {
                println!("{}", display_path);
            }
        }
    }

    Ok(())
}

/// Render a live preview for an item (Tmux pane capture or directory inspection).
pub fn preview(target: &str) -> anyhow::Result<()> {
    let clean_target = strip_icon(target);
    let active_sessions = get_tmux_sessions();

    // If target is an active Tmux session, capture its terminal screen
    if active_sessions.iter().any(|s| s == clean_target) {
        let output = Command::new("tmux")
            .args(["capture-pane", "-ep", "-t", clean_target])
            .output()?;
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return Ok(());
    }

    let expanded = expand_tilde(clean_target);
    if expanded.is_dir() {
        // If eza is available, render formatted directory tree
        if which::which("eza").is_ok() {
            let output = Command::new("eza")
                .args([
                    "--tree",
                    "--level=2",
                    "--color=always",
                    "--icons=always",
                    &expanded.to_string_lossy(),
                ])
                .output()?;
            print!("{}", String::from_utf8_lossy(&output.stdout));
            return Ok(());
        }

        // Standard directory listing fallback
        if let Ok(entries) = std::fs::read_dir(&expanded) {
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
                .take(35)
                .collect();
            names.sort();
            for name in names {
                println!("{}", name);
            }
        }
        return Ok(());
    }

    println!("Preview: {}", target);
    Ok(())
}

/// Handle CLI invocation when waymaker is invoked via `sesh` symlink or alias.
pub async fn handle_sesh_cli() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return run_interactive_session_picker().await;
    }

    let cmd = args[0].as_str();
    match cmd {
        "connect" => {
            let mut switch_only = false;
            let mut target = None;

            for arg in &args[1..] {
                if arg == "-s" || arg == "--switch" {
                    switch_only = true;
                } else if !arg.starts_with('-') && target.is_none() {
                    target = Some(arg.as_str());
                }
            }

            if let Some(t) = target {
                if let Err(e) = connect(t, switch_only) {
                    eprintln!("sesh connect error: {}", e);
                    return 1;
                }
                0
            } else {
                eprintln!("Usage: sesh connect [--switch] <session_or_dir>");
                1
            }
        }
        "last" => {
            if let Err(e) = last() {
                eprintln!("sesh last error: {}", e);
                return 1;
            }
            0
        }
        "list" => {
            let mut icons = false;
            let mut tmux_only = false;
            let mut dirs_only = false;

            for arg in &args[1..] {
                if arg.starts_with("--") {
                    match arg.as_str() {
                        "--icons" => icons = true,
                        "--tmux" => tmux_only = true,
                        "--dirs" | "--zoxide" => dirs_only = true,
                        _ => {}
                    }
                } else if arg.starts_with('-') {
                    for c in arg[1..].chars() {
                        match c {
                            'i' => icons = true,
                            't' => tmux_only = true,
                            'z' => dirs_only = true,
                            _ => {}
                        }
                    }
                }
            }

            if let Err(e) = list(icons, tmux_only, dirs_only) {
                eprintln!("sesh list error: {}", e);
                return 1;
            }
            0
        }
        "preview" => {
            if let Some(target) = args.get(1) {
                if let Err(e) = preview(target) {
                    eprintln!("sesh preview error: {}", e);
                    return 1;
                }
                0
            } else {
                eprintln!("Usage: sesh preview <session_or_dir>");
                1
            }
        }
        "-h" | "--help" | "help" => {
            println!("sesh (Waymaker native workspace & session engine)");
            println!("\nUSAGE:");
            println!("    sesh [command] [options]");
            println!("\nCOMMANDS:");
            println!("    connect <target>     Connect to or create a session");
            println!("    last                 Switch to the previous session");
            println!("    list [--icons]       List sessions and frecency directories");
            println!("    preview <target>     Live preview for session or directory");
            0
        }
        _ => {
            // If unknown argument, treat as target to connect to directly
            if !cmd.starts_with('-') {
                if let Err(e) = connect(cmd, false) {
                    eprintln!("sesh error: {}", e);
                    return 1;
                }
                0
            } else {
                eprintln!("Unknown sesh command '{}'", cmd);
                1
            }
        }
    }
}

/// Handle CLI invocation for explicit `wm session`, `wm connect`, `wm last` subcommands.
pub async fn handle_session_cli(config_args: &[String]) -> Option<i32> {
    if config_args.is_empty() {
        return None;
    }

    let first = config_args[0].as_str();

    match first {
        "session" | "sessions" => {
            if config_args.len() == 1 {
                return Some(run_interactive_session_picker().await);
            }

            let sub = config_args[1].as_str();
            match sub {
                "list" => {
                    let mut icons = false;
                    let mut tmux_only = false;
                    let mut dirs_only = false;

                    for arg in &config_args[2..] {
                        if arg.starts_with("--") {
                            match arg.as_str() {
                                "--icons" => icons = true,
                                "--tmux" => tmux_only = true,
                                "--dirs" | "--zoxide" => dirs_only = true,
                                _ => {}
                            }
                        } else if arg.starts_with('-') {
                            for c in arg[1..].chars() {
                                match c {
                                    'i' => icons = true,
                                    't' => tmux_only = true,
                                    'z' => dirs_only = true,
                                    _ => {}
                                }
                            }
                        }
                    }

                    let _ = list(icons, tmux_only, dirs_only);
                    Some(0)
                }
                "connect" => {
                    let switch_only = config_args.iter().any(|a| a == "--switch" || a == "-s");
                    let target = config_args.iter().skip(2).find(|a| !a.starts_with('-'));
                    if let Some(t) = target {
                        if let Err(e) = connect(t, switch_only) {
                            eprintln!("Error: {}", e);
                            Some(1)
                        } else {
                            Some(0)
                        }
                    } else {
                        eprintln!("Usage: wm session connect [--switch] <target>");
                        Some(1)
                    }
                }
                "last" => {
                    if let Err(e) = last() {
                        eprintln!("Error: {}", e);
                        Some(1)
                    } else {
                        Some(0)
                    }
                }
                "preview" => {
                    if let Some(target) = config_args.get(2) {
                        let _ = preview(target);
                        Some(0)
                    } else {
                        eprintln!("Usage: wm session preview <target>");
                        Some(1)
                    }
                }
                _ => {
                    // Treat as target to connect
                    if !sub.starts_with('-') {
                        if let Err(e) = connect(sub, false) {
                            eprintln!("Error: {}", e);
                            Some(1)
                        } else {
                            Some(0)
                        }
                    } else {
                        Some(run_interactive_session_picker().await)
                    }
                }
            }
        }
        "connect" => {
            let switch_only = config_args.iter().any(|a| a == "--switch" || a == "-s");
            let target = config_args.iter().skip(1).find(|a| !a.starts_with('-'));
            if let Some(t) = target {
                if let Err(e) = connect(t, switch_only) {
                    eprintln!("Error: {}", e);
                    Some(1)
                } else {
                    Some(0)
                }
            } else {
                eprintln!("Usage: wm connect [--switch] <target>");
                Some(1)
            }
        }
        "last" => {
            if let Err(e) = last() {
                eprintln!("Error: {}", e);
                Some(1)
            } else {
                Some(0)
            }
        }
        _ => None,
    }
}

/// Run interactive TUI picker for sessions and frecency directories.
async fn run_interactive_session_picker() -> i32 {
    // Generate the combined list: active Tmux sessions + frecency directories
    let mut items = Vec::new();
    let home = dirs::home_dir();

    for sess in get_tmux_sessions() {
        items.push(format!(" {}", sess));
    }

    for d in get_frecency_dirs(None) {
        let display_path = if let Some(ref h) = home {
            if let Ok(rel) = Path::new(&d).strip_prefix(h) {
                format!("~/{}", rel.display())
            } else {
                d.clone()
            }
        } else {
            d.clone()
        };
        items.push(format!(" {}", display_path));
    }

    let input_data = items.join("\n");

    // Launch Waymaker TUI with piped input and sesh-picker preset
    let mut child = match Command::new(std::env::current_exe().unwrap_or_else(|_| "wm".into()))
        .args(["-o", "sesh-picker"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to launch waymaker picker: {}", e);
            return 1;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(input_data.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Error waiting for waymaker: {}", e);
            return 1;
        }
    };

    if output.status.success() {
        let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !selected.is_empty() {
            if let Err(e) = connect(&selected, false) {
                eprintln!("Connect error: {}", e);
                return 1;
            }
        }
        0
    } else {
        output.status.code().unwrap_or(1)
    }
}
