# Waymaker Jump Mode (`wm -o jump` / `j` / `z`)

**Jump Mode** (`wm -o jump`) is Waymaker's flagship interactive directory navigation and file manager preset. It transforms your terminal into a hybrid ultra-fast fuzzy finder and interactive TUI navigator, combining the instant directory jumping of tools like `zoxide`/`fzf` with the visual power of file managers like **Yazi** and **Superfiles**.

---

## 1. What is Jump Mode & Why Matchmaker?

Jump Mode is designed for instant shell directory switching (`j <keyword>`) and interactive filesystem exploration.

### How it Compares to Other Tools

| Feature | `fzf` + `zoxide` | Yazi / Superfiles | **Waymaker Jump Mode (`wm -o jump`)** |
| :--- | :--- | :--- | :--- |
| **Speed & Engine** | Go / Shell scripts | Rust (Async I/O) | **Rust (`Nucleo` engine from Helix editor)** |
| **Interactive Navigation** | Static list (Exits on pick) | Full File Manager TUI | **Dynamic `l` (enter) / `h` (parent) without closing session** |
| **Directory Prioritization** | Plain text list | Tree view | **Native 3-Tier Sorting (`dir_first` in C-speed Rust)** |
| **TUI Interface & Breadcrumbs** | Basic border | Dual/Triple pane | **Custom rounded TUI, Breadcrumbs (`/`), Debounced Previews** |
| **Hybrid Discovery** | History OR File search | File tree only | **Hybrid: Local search + Global Frecency cycling (`ctrl-z`)** |

---

## 2. Key Features & Navigation UX

### Instant Interactive & Seamless Traversal (`l`, `h`, `ctrl-l`, `ctrl-h`)
Unlike traditional fuzzy finders where picking an item terminates the session:
- **`l` / `ctrl-l`**: Enters the highlighted directory (`ChDir({=})`), clears the filter query, and reloads the view for subfolder exploration. `ctrl-l` works seamlessly directly from the filter input without pressing `Tab`.
- **`h` / `ctrl-h`**: Navigates up to the parent directory (`ChDir(..)`), clears the filter query, and reloads.
- **`Enter`**: Accepts the selected directory, automatically records it to the `frecency` database, and changes your shell working directory upon exit.

### Ancestor Jump (`ctrl-u` / `u`)
Instantly generates and loads the upward ancestor directory hierarchy (from current directory up to `/`).

> [!TIP]
> #### 🌟 Where Ancestor Jump Shines:
> - **Monorepos & Deep Directory Trees**: When you are 5+ levels deep (e.g., `~/dev/github/matchmaker/matchmaker-lib/src/render/widgets/`) and need to jump straight to the repository root (`~/dev/github/matchmaker/`) in 1 step without hitting `h` or typing `cd ..` multiple times.
> - **Sister Project Switching**: Ascend directly to a common root directory (e.g. `~/dev/github/` or `~/dev/`) to jump across distinct projects within the same Matchmaker session.
> - **Visual Context Inspection**: Audits parent directories using the live tree preview panel before selecting and jumping.
> - **Ergonomic Hotkeys**: Accessible via `ctrl-u` while typing in the filter or `u` in navigation mode.

### Hybrid Source Switching (`f` / `ctrl-f` / `@reloadnext`)
Jump Mode allows cycling between input sources on the fly using `f` or `ctrl-f`:
1. **Source 0 (Default)**: Current directory contents + subdirectories (native `AsyncWalker`).
2. **Source 1**: Global frecency directory database (`wm list --dirs`).

### Visual Previews & Breadcrumbs
- **Live Tree Previews**: Previews folders using `eza --tree --level=2 --icons --git-ignore` and files using `bat`.
- **40 FPS Debounced Rendering**: Configured with `[previewer] debounce_ms = 25` to ensure buttery smooth scrolling (`j`/`k`) without spawning duplicate child processes.
- **Top Breadcrumb**: Displays your current directory path formatted with bold cyan separators (`Cyan /`).

---

## 3. File Manager & File Manipulation Possibilities

Matchmaker Jump Mode goes beyond simple directory jumping, supporting rich overlay actions and file operations:

### Selection & Path Operations
- **Smart Selection & Rewind (`Space`)**: Pressing `Space` on an unselected item selects it and steps DOWN. Pressing `Space` on an already-selected item unselects it and steps UP (rewind chain), allowing fast 1-key multi-selection and backward unselection without extra keybindings.
- **Clear All Selections (`ClearSelections`)**: Clears all active selections instantly.
- **Yank / Cut Paths**: Mark paths for copy/move operations across directories.

### File Operations & Overlays
- **`a` Create File/Folder**: Create directories or files on the fly.
- **`r` Rename**: Rename files or directories with a live inline overlay.
- **`d` Delete / Move to Trash**: Confirm deletion with undo history.
- **`z` / `Z` Zip / Unzip**: Create an archive from the selected item(s), or extract the selected archive.
- **`D` Drag-and-Drop with ripdrag**: Send the selected item(s) to `ripdrag`; install the `ripdrag` command and drag the files from its window to the destination application.

---

## 4. Preset Configuration (`~/.config/waymaker/presets/jump.toml`)

Below is the optimized Jump Mode configuration:

```toml
[ui.nav]
active = true
bar = "Plain"
color = "Black"

[matcher]
depth_penalty = 15
typo_tolerance = true
dir_first = true

[matcher.sort]
threshold = "smart"
cap = 2000

[matcher.frecency]
active = true
weight = 2

[results]
reverse = false
icons = true
uncolor_current_icon = true
row_connection = "Full"
current_nav_bar = "QuadrantInside"

[results.symlink]
active = true
style.fg = "cyan"

[results.current_nav_bar_style]
fg = "yellow"

[results.current_style]
fg = "black"
bg = "lightblue"
modifier = "BOLD"

[previewer]
debounce_ms = 25
delay_clear = true

[preview]

[preview.media]
active = true

[preview.border]
sides = "LEFT"
color = "Darkgray"
title_fg = "Cyan"

[[preview.layout]]
command = "p={1}; p=\"${p/#\\~/$HOME}\"; if [ -d \"$p\" ]; then eza --tree --level=2 --icons --git-ignore --color=always \"$p\"; else bat --style=numbers --color=always --line-range=:300 \"$p\"; fi"
side = "right"
percentage = 80

[binds]
"@reloadnext" = "ReloadNext"
"ctrl-f" = "@reloadnext"
"ctrl-j" = "Down"
"ctrl-k" = "Up"
"ctrl-p" = "SwitchPreview"
"ctrl-e" = "Execute(nvim {+})"
"ctrl-l" = ["ChDir({=})", "Cancel", "Reload", "Pos(0)"]
"ctrl-h" = ["ChDir(..)", "Cancel", "Reload", "Pos(0)"]
"ctrl-u" = ["Reload(curr=\"$(pwd)\"; while [ \"$curr\" != \"/\" ] && [ -n \"$curr\" ]; do curr=\"$(dirname \"$curr\")\"; echo \"$curr\"; done)", "Cancel", "Pos(0)"]

[ui.nav.binds]
"f" = "@reloadnext"
"e" = "Execute(nvim {+})"
"l" = ["ChDir({=})", "Cancel", "Reload", "Pos(0)"]
"h" = ["ChDir(..)", "Cancel", "Reload", "Pos(0)"]
"ctrl-e" = "Execute(nvim {+})"
"ctrl-l" = ["ChDir({=})", "Cancel", "Reload", "Pos(0)"]
"ctrl-h" = ["ChDir(..)", "Cancel", "Reload", "Pos(0)"]
"p" = "SwitchPreview"
"ctrl-p" = "SwitchPreview"
"u" = ["Reload(curr=\"$(pwd)\"; while [ \"$curr\" != \"/\" ] && [ -n \"$curr\" ]; do curr=\"$(dirname \"$curr\")\"; echo \"$curr\"; done)", "Cancel", "Pos(0)"]
"ctrl-u" = ["Reload(curr=\"$(pwd)\"; while [ \"$curr\" != \"/\" ] && [ -n \"$curr\" ]; do curr=\"$(dirname \"$curr\")\"; echo \"$curr\"; done)", "Cancel", "Pos(0)"]

[breadcrumb]
show = true
separator = "/"
style.fg = "Cyan"
style.modifier = "BOLD"
separator_style.fg = "Cyan"

[start]

[start.command]
command = ""
additional = [
    "",
    "fd -H -E.git -Enode_modules -E.cache -Etarget -Edist -Ebuild --strip-cwd-prefix . 2>/dev/null",
    "wm list --dirs 2>/dev/null | sed -E \"s#^$HOME(/|$)##\" | grep -v \"^$\""
]

[start.command]
command = '''
IGNORE="-E.git -Enode_modules -E.cache -Etarget -Edist -Ebuild -E__pycache__ -E.venv -Evenv -Eenv -Evendor -E.gradle -E.npm -E.pnpm-store -E.next -Eout -Ecoverage -E.tox -E.mypy_cache -E.pytest_cache -E.cargo -E.rustup -E.mozilla -E.thunderbird"

if command -v fd >/dev/null 2>&1; then
    fd -H $IGNORE --strip-cwd-prefix --min-depth 1 --max-depth 1 . 2>/dev/null
    fd -H $IGNORE --strip-cwd-prefix --min-depth 2 . 2>/dev/null
else
    find .
fi
'''
```

---

## 5. Shell Integration & Object-First Buffer Ergonomics

When Waymaker Jump Mode is paired with shell widgets (e.g. `_wm_jump_widget` in Zsh via `eval "$(wm init zsh)"`), it uses **Context-Aware Buffer Placement** and **Canonical Path Resolution**:

### Object-First Command Composition (`CURSOR=0` on Empty Prompt)
When invoked from an empty prompt (e.g. `<Tab>` or `Ctrl+T`):
1. Selecting a **directory** performs an immediate `cd` into that directory.
2. Selecting **file(s)** formats them intelligently:
   - **Inside `$PWD`**: Clean relative path (e.g. `completion.md` or `docs/shell/completion.md`).
   - **Outside `$PWD`**: Canonical path with tilde compression (`~/.dotfiles/...`).
   Prefixes a leading space and sets `CURSOR=0`:
   ```zsh
   ❯ █ completion.md
   ```
3. Type the command (`nvim`, `bat`, `cat`, `rm`) and press `Enter` — no cursor navigation keystrokes required!

### Mid-Command Argument Insertion
When invoked while typing a command (e.g. `git add ` or `cp `), the selected files are appended at the active cursor position with trailing spaces, preserving the existing command without side effects.
