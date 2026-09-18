# Matchmaker Frecency Database & Subcommand Architecture

Matchmaker (`mm`) includes a built-in, high-performance **Frecency** (Frequency + Recency) tracking engine designed to track, rank, and provide instant access to your most frequently and recently accessed files and directories.

---

## 1. Storage & Database Architecture (`redb`)

- **Embedded Database**: Frecency data is stored using [**`redb`**](https://github.com/cberner/redb), a pure-Rust, zero-dependency, high-performance embedded ACID database.
- **Table Structure**: Data is maintained in the `frecency_v1` table (`TableDefinition<&str, &str>`), mapping normalized path strings to JSON-serialized `FrecencyRecord` structs.
- **Database Path**: Stored in the user's state directory:
  - Linux/macOS: `~/.local/state/matchmaker/matchmaker.redb`
  - Windows: `%LOCALAPPDATA%\matchmaker\matchmaker.redb`

---

## 2. Frecency Scoring Algorithm

Each path record maintains an access count, last access timestamp, and a sliding window of up to 50 recent access timestamps.

### Continuous Half-Life Exponential Decay (Default)
Scores are computed using a **continuous exponential half-life decay** model (default: `frecency.half_life_days = 7`):

$$\text{Score} = \sum_{i=1}^{N} 100 \times 2^{-\frac{\text{now} - t_i}{t_{\text{half-life}}}}$$

- **Smooth Monotonic Decay**: Eliminates cliff-edge discontinuities (where crossing a 24-hour boundary suddenly dropped 50% of the score at once).
- **Configurable Half-Life**: Controlled by `matcher.frecency.half_life_days` (or `[worker.frecency] half_life_days = 7`, alias `--hl`).

### Legacy Discrete Bucket Fallback
Setting `frecency.half_life_days = 0` re-enables the legacy discrete 5-bucket weighting model:

| Access Recency | Time Window | Bonus Score per Access |
| :--- | :--- | :--- |
| **< 1 Hour** | `< 3,600 s` | **100 points** |
| **< 1 Day** | `< 86,400 s` | **80 points** |
| **< 1 Week** | `< 604,800 s` | **40 points** |
| **< 1 Month** | `< 2,592,000 s` | **20 points** |
| **>= 1 Month** | `>= 2,592,000 s` | **10 points** |

---

## 3. Automatic Recording on Selection, Execution & Navigation

When `frecency = true` (or `--frecency` / `-f`) is enabled (such as in **Jump Mode** `mm -o jump`, the default config, or shell integrations `j` / `z`):
- **Automatic Selection & Accept Tracking**: Whenever you select/accept a directory or file in navigation mode (`nav_mode = true`) or hit `Enter`/`@accept` to pick a path, Matchmaker automatically records access via `FrecencyStore::add()`.
- **Automatic Execution & Editor Tracking**: Whenever items are opened or executed via `Execute` (e.g. `e` / `Ctrl+E` with `Execute(nvim {+})`), `ExecuteAndQuit`, or `Become`, Matchmaker automatically registers all opened files/directories into the frecency database.
- **Multi-Selection Tracking**: When multiple items are selected (using <kbd>Tab</kbd>), every selected path is automatically tracked and boosted in the frecency database.
- **Dynamic Score Growth**: Every access increases the path's access count, updates its `last_accessed` timestamp, and boosts its frecency score, making frequently visited and edited files and directories naturally climb to the top of future searches.
- **Intentional vs Transient Selection**: Navigation hops (`ChDir` with `l`/`h`) do not pollute intermediate paths; only deliberate selection and execution actions commit access.

---

## 4. In-Memory Search Engine (`FrecencySnapshot`) & Location Bias

To prevent database disk reads during active TUI fuzzy filtering, Matchmaker loads all active frecency records into an in-memory `FrecencySnapshot`:

- **Zero-Allocation Lookups**: Uses `rustc_hash::FxHashMap` for absolute paths and CWD-relative resolution without heap allocations during search.
- **Strict Exact Path Resolution**: Matches absolute paths, tilde-expanded paths (`~/...`), and paths relative to the current working directory (`self.cwd`). Generic basename boosting is omitted to avoid phantom frecency pollution across sibling repositories.
- **Contextual Location Bias**: When searching, items located within or relative to the current working directory receive a configurable percentage bonus boost (`matcher.location_bias = 30` or `worker.location_bias = 30`, alias `--lb`). This gives local project files natural priority over distant filesystem paths.
- **Sub-Millisecond Scoring**: During active search or sorting (`frecency = true`), the snapshot evaluates score boosts in **~5 nanoseconds per item** in RAM.

---

## 5. Frecency CLI Commands & Subcommands

Matchmaker provides a comprehensive set of CLI subcommands for managing and inspecting the frecency database:

### Record & Query
- **`mm add <path>`**: Records an access event for a file or directory path (increases score and timestamp).
- **`mm rank <path>`**: Displays the current frecency score, total access count, last access time, and raw records for a path.
- **`mm list [-d / --dirs / --dirs-only] [keywords...]`** / **`mm query`**: Lists tracked paths sorted by frecency score descending, filtered to items matching ALL specified keywords.
  - Option `-d` / `--dirs`: Filters and outputs **directories only** (used in `jump` shell integrations like `j` / `z`).

### Maintenance & Management
- **`mm rm <path>`** / **`mm remove <path>`**: Manually deletes a path entry from the frecency database.
- **`mm clean`** / **`mm prune`**: Scans the database and purges stale entries (paths that no longer exist on the local filesystem).
- **`mm cache [path]`**: Warm-indexes directory structures into local cache for ultra-fast startup.

### Integrations & Migrations
- **`mm import zoxide`**: Imports historical directory access records and scores directly from `zoxide`.
- **`mm init <shell> [--cmd <alias>]`**: Generates shell integration code for `zsh`, `bash`, `fish`, `nushell`, or `powershell` with optional command alias (e.g. `mm init zsh --cmd j`).

---

## 6. Smart Sorting, Ranking Formula & Real-World Examples

### A. The `sort = "smart"` Mode Behavior

- **Empty Query (`query = ""`)**:
  Preserves stream order while applying **`dir_first`** native tiering:
  - **Tier 0 (Top)**: Direct subdirectories of the current directory, sorted alphabetically.
  - **Tier 1 (Middle)**: Direct files of the current directory, sorted alphabetically.
  - **Tier 2 (Bottom)**: Deeper subfolder items.

- **Active Filtering (`query = "keyword"`)**:
  Activates the Nucleo fuzzy matcher and evaluates items in real time using the unified ranking score formula.

---

### B. The Unified Score Ranking Formula

For every matched candidate, Rust computes:

`effective_score = base_score + frecency_bonus + dir_priority - (depth * depth_penalty)`

1. **`base_score`**: Similarity match score generated by **Nucleo** (rewards exact matches, word boundaries, and prefix matches).
2. **`frecency_bonus`**: `frecency_weight` * `FrecencySnapshot` score (with location bias applied for local items, computed in ~5ns).
3. **`dir_priority`**: When `dir_first = true`:
   - **Tier 0** (Direct subdirectories): **+2,000,000,000** points.
   - **Tier 1** (Direct files): **+1,000,000,000** points.
   - **Tier 2** (Deeper items): **0** points.
4. **`depth_penalty`**: Subtractions per `/` depth level (e.g. `15` points per level).

---

### C. `sort_cap` Performance Bounding

In directories with 600,000+ files:
- **`sort_cap`** (default `1000`, preset `2000`) restricts full CPU-intensive re-sorting to the top candidate entries.
- If navigation extends beyond `sort_cap`, Matchmaker transparently streams Nucleo matches without UI stutter or blank screens.

---

### D. Practical Real-World Examples

#### Example 1: Filtering in Home (`/home/fecavmi`)
- **Query `""`**: Direct folders (`.antigravity/`, `.config/`, `dev/`) rank at Tier 0 in alphabetical order.
- **Query `"dot"`**: `.dotfiles/` receives Tier 0 priority (+2B) + frecency bonus points + Nucleo prefix match, rising to **#1 absolute position**.

#### Example 2: Filtering in Project Directory (`/home/fecavmi/dev/github`)
- **Query `"mat"`**:
  - `matchmaker/` (direct folder): Tier 0 (+2B) + Frecency bonus (+300) + Nucleo prefix match -> **Score ~2,000,000,350** (**#1 Position**).
  - `matchmaker/fecavmi/src/main.rs` (deep file): Tier 2 (0) + Depth penalty (-45) -> **Score ~850** (Ranks far below, preserving root folder navigation).

---

## 7. Universal Frecency for Piped & Non-File Text Streams

Unlike filesystem-only indexers (such as FFF daemon) that only index directory paths, Matchmaker's `frecency.rs` engine operates on **arbitrary string keys**. When piping text lines from any shell command into `mm` (`command | mm`), selection events automatically record frecency scores for those exact string lines.

### Key Use Cases:

- **Docker Containers & Kubernetes Pods**: `docker exec -it $(docker ps --format '{{.Names}}' | mm) bash`  
  Frequently accessed container names (e.g. `api-server-dev`) rise to the top on Frame 0, allowing instant 1-press `Enter` selection.
- **Git Branches**: `git checkout $(git branch --sort=-committerdate | awk '{print $1}' | mm)`  
  Active feature branches stay prioritized at the top of the list during branch switching.
- **SSH Hosts**: `ssh $(grep -i "^Host " ~/.ssh/config | awk '{print $2}' | grep -v '*' | mm)`  
  Frequently visited SSH remote hosts are automatically boosted in search results.
- **Tmux Sessions**: `tmux switch-client -t $(tmux list-sessions -F "#{session_name}" | mm)`  
  Session names you switch to most often appear at the top.
- **Make / Just / Task Recipes**: `just $(just --summary | tr ' ' '\n' | mm)`  
  Build and test tasks used daily are ranked higher than rarely used recipes.
