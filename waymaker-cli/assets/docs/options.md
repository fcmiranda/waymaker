# Command Line Options

Matchmaker allows you to override any configuration setting directly from the command line. Overrides are specified as key-value pairs following the standard arguments.

## Syntax

Overrides follow the pattern `path=value` or `path value`.

- **Hierarchical Paths**: Use dot notation to navigate the configuration structure (e.g., `results.style.fg`).
- **Flattened Fields**: Several major configuration blocks are "flattened," meaning their children can be accessed as top-level keys.
- **Shortcuts**: Many common fields have short aliases:
  - `binds` -> `b`
  - `start` -> `s`
  - `header.header_lines` -> `h.h`
  - `results.reverse` -> `r.r`
  - `results.wrap` -> `r.w`
  - `preview.layout` -> `p.l`
  - `preview.initial` -> `p.i`
- **Absolute Aliases**: The following common paths can be accessed directly:
  - `preview.layout.command` -> `px`
  - `start.input_separator` -> `i`
  - `start.output_template` -> `o`
  - `start.command` -> `x`
  - `start.command` -> `cmd`
  - `start.ansi` -> `a`
  - `start.trim` -> `t`
  - `columns.split` -> `d`
  - `preview.layout` -> `P`
  - `header.content` -> `h`
  - `matcher.sort` -> `S`

For example to split input on space, use `wm d " "`.

# Miscellaneous

### Presets and Named Overrides (`-o` / `--override`)

The `-o` flag allows you to layer additional configuration files on top of your base config. This is allows for consistent keybindings and settings between many different workflows.

- **Relative Paths**: `-o` accepts an absolute path, but if you provide a relative path without an extension, Waymaker will automatically look for a matching `.toml` file in the `presets` directory of your waymaker configuration directory.
- **Example**: `wm -o git/status` will attempt to load `presets/git/status.toml` from the installation directory.
- **Source Field**: Overrides support a `source` field at the top level, allowing them to inherit from another preset (one level of recursion is supported).

### Values

If a "leaf" value contains multiple settings (like a [border](#border-settings) or a bind with multiple actions), you can specify them within a single string joined by `|||` (which can be escaped like `\|||`).

A few illustrative (but not very practical) examples:

```bash
# Example:
# If you started with one preview layout, the following overrides the first preview layout to just display hi and have a minimum width of 3, and adds two new ones. It also sets 3 binds.
wm p.l command=ls p.l "x=echo hi|||min=3" b "ctrl-c=Quit|||?=preview(echo hi)" b.ctrl-a cancel

# Example:
# Setting the column splitting delimiter
wm m.c.split "\w+|||/\w+" # Sets the field: columns.split = Split::Regexes([Regex('\w'), Regex('/\w+')])

# Note that the same effect is NOT achieved by specifying wm m.c.split "\w+" m.c.split "/\w+" in this case:
# both declare a single (delimiter) regex, and the second command overwrites the first.
```

Note however, that when declaring a bind, you should prefer to use `wm b.ctrl-x "ExecuteSilent(rm {+})|||Reload"` over `wm b "ctrl-x=ExecuteSilent(rm {+})"`, since as you can see, the second format doesn't support chained actions, while the first does.

Bool values can be specified with true, false, or "".

```bash
# Example:
# Enable result wrapping and scroll wrapping
wm p.w= r.r=
```

### Collections (Lists/Vectors)

Two of the collections: `preview.layout` and `columns.names`, are consumed additively:

1. **Adding Elements**: Each time a collection path is specified, a new partial element is added to that collection.
2. **Merging**: When the configuration is finalized:
   - The first $N$ overrides for a collection are merged into the first $N$ elements of the base configuration (from your config file). (Or in the case of of binds, existing keys are overridden).
   - Any additional overrides are appended as new elements.

### Colors and Modifiers

All colors and modifiers come from ratatui:

- https://ratatui.rs/examples/style/colors/
- https://docs.rs/ratatui/latest/ratatui/style/struct.Modifier.html

## Available Options

### Start (`start.`, `s`)

- `command`: (string or object) The shell command used to generate items.
  - Absolute alias: `x`, `cmd`.
  - If an object:
    - `command`: (string) The shell command.
    - `separator`: (char) Input separator (overrides `start.input_separator` for this command).
- `input_separator`: (char) Character separating input items.
  - Absolute alias: `i`.
- `os`, `output_separator`: (string) String separating output selections.
- `output_template`: (string) Template string used to print results.
  - Absolute alias: `o`.
- `sync`: (bool) Whether to wait for the command to finish before starting.
- `trim`: (bool) Trim whitespace from input lines.
  - Absolute alias: `t`.
- `ansi`: (bool) Parse ansi codes from input.
  - Absolute alias: `a`.
- `ax`, `additional_commands`: ([String]) Additional commands that can be cycled through using the ReloadNext action.
- `mode`: (string) The initial mode of the application. Default values (`tty`, `t0`, `piped`, `t1`) depend on whether stdin and stdout are connected to /dev/tty.
- `directory`: (string) Change directory context.
  - `~` is resolved to home directory.
  - If an object:
    - `value`: (string) The directory path or command resolving to the directory path.
    - `exec`: (bool) If true, the directory is read from the stdout of the executed value (default false).
    - `force`: (bool) If true, exit application if directory could not be changed to.
- `group_prefix`: (string) Specify a prefix that indicates a line is a group header. Used as `--group-prefix` on the command line.

### Exit (`exit.`, `e`)

- `select_1`: (bool) Exit automatically if there is only one match.
- `allow_empty`: (bool) Allow returning without any items selected.
- `abort_empty`: (bool) Abort if no items are provided (default true).

### Matcher (`matcher.`, `m`)

- `normalize`: (bool) Enable/disable normalization of characters (e.g., matching 'e' with 'é').
- `ignore_case`: (bool) Enable/disable case-insensitive matching.
- `prefer_prefix`: (bool) Prioritize matches that start with the query.

#### Worker *(flattened)*

- `sort_threshold`, `sort`: (number | bool | string) Similarity threshold or mode: `0` or `true` to always sort, `false` or `u32::MAX` to never sort (preserve stream insertion order), `"smart"` or `"auto"` to preserve natural stream order on empty query and rank by fuzzy relevance score when typing.
- `depth_penalty`, `dp`: (number) Penalty subtracted from fuzzy match rank score per directory depth level ('/' or '\'). Default `0` (disabled). Set to e.g. `15` to prioritize root/shallow files over deeply nested subfolders.
- `frecency`, `frec`: (bool) Enable frecency (frequency + recency) score boosting for matched items. Default `false`.
- `frecency_weight`: (number) Weight multiplier for frecency bonus points. Default `1`.
- `sort_cap`, `sc`: (number) Maximum number of top matched items to re-sort by frecency/depth penalty. Default `1000`. Set to `0` for unlimited.
- `typo_tolerance`, `tt`: (bool) Enable typo-tolerant fuzzy matching for queries >= 3 characters. Default `false`.
- `dir_first`, `df`: (bool) Prioritize direct child directories over files and deeper paths, putting local subfolders at the top for interactive directory navigation. Default `false`.
- `raw`: Enable raw mode where non-matching items are also displayed in a dimmed color. (unimplemented)
- `track`: Track the current selection when the result list is updated. (unimplemented)
- `reverse`: Reverse the order of the input (unimplemented)

### Columns (`columns.`, `c`)

- `s`, `split`: Defines how the input line is divided into columns. This can be `None`, a single `Delimiter` regex, or a list of `Regexes`.
  - **No Splitting** (`null`): The entire line is treated as a single column.
  - **Single Regex** (`"regex"`):
    - **No Capture Groups**: The regex is treated as a delimiter. Columns are the segments *between* matches.
    - **Unnamed Capture Groups**: If the regex contains capture groups (e.g., `(\d+) (\w+)`), each group's match becomes a column in order.
    - **Named Capture Groups**: If the regex contains named groups (e.g., `(?P<size>\d+) (?P<name>\w+)`), matches are mapped to columns with matching names defined in `columns.names`.
  - **Multiple Regexes** (`"[re1] [re2].."`): Each regex is searched independently; the match becoming the corresponding column.
- `names`, `n`: List of column names/settings.
  - `name`: (string) Name of the column.
    - Must be alphanumeric.
- `max_columns`: (number) Maximum number of autogenerated columns.
- `default_column`: (string) The name of the default column (default: first column).

### UI & Rendering

#### Global UI (`ui.`)

- `tick_rate`: (number) Refresh rate of the UI (default 60).
- `border`: [Border Settings](#border-settings).

#### Query Bar (`query.`, `q`)

- `prompt`: (string) The prompt prefix (default "> ").
- `initial`: (string) Initial text in the input bar.
- `style`: [Style Settings](#style-settings) for the input text.
- `prompt_style`: [Style Settings](#style-settings) for the prompt.
- `cursor`: Cursor style.
- `border`: [Border Settings](#border-settings).

#### Results Table (`results.`, `r`)

- `multi_prefix`: (string) Prefix for multi-selected items.
- `unselected_prefix`: (string) Marker displayed for normal, unselected items (default: `"  "`).
- `default_prefix`: (string) Prefix for normal items.
- `current_prefix`: (string) Prefix for the current item.
- `spinner_prefix`: (string) Input prefix character that triggers spinner animation for the row (e.g. `?` or `@`).
- `spinner`: (string) Named spinner frame set to animate (options: `dot`, `line`, `jump`, `pulse`, `points`, `meter`, `hamburger`, `ellipsis`, `globe`, `moon`, `monkey`, `arc`, `nerd`, `nerdarc`, `minidot`).
- `style`: [Style Settings](#style-settings) (default).
- `inactive_style`, `inactive`: [Style Settings](#style-settings) for inactive columns.
- `inactive_current_style`, `inactive_current`: [Style Settings](#style-settings) for the current item in inactive columns.
- `match_style`, `match`: [Style Settings](#style-settings) for matching characters.
- `current_style`, `current`: [Style Settings](#style-settings) for the highlighted item.
- `prefix_style`, `prefix`: [Style Settings](#style-settings) for the prefix of the active.
- `inactive_prefix_style`, `inactive_prefix`: [Style Settings](#style-settings) for the prefix of inactive items.
- `unselected_prefix_style`, `unselected_prefix`: [Style Settings](#style-settings) for the unselected prefix marker.
- `spinner_style`: [Style Settings](#style-settings) for the animated spinner.
- `row_connection`: `Disjoint`, `Capped`, or `Full`. Controls how current item styles apply across the row.
- `icons`: (bool) Prepend file-type Nerd Font icons before the first column text.
- `uncolor_current_icon`: (bool) Whether file icons on the cursor/focused row lose individual color to match the line selection style (Yazi parity).
- `invert_current_icon`: (bool) Whether file icons on the cursor/focused row invert their colors (e.g. Blue <-> Yellow, Red <-> Cyan).
- `current_icon_style`: [Style Settings](#style-settings) Custom style override for the icon glyph on the focused row.
- `current_nav_bar`: `Plain` (│), `Thick` (█), `Double` (║), `Rounded` (│), `QuadrantOutside` (▌), `QuadrantInside` (▐). Independent border thickness/character override for the navigation bar cell on the focused row.
- `current_nav_bar_style`: [Style Settings](#style-settings) Custom style (fg, bg, modifier) override for the navigation bar cell on the focused row.
- `symlink`: Symlink target display settings.
  - `active` / `target`: (bool) Append symlink target text (`-> destination`) to the first column.
  - `style`: [Style Settings](#style-settings) for the appended symlink target text.
- `tier`: 3-Tier ranking separator settings (when `dir_first` is enabled).
  - `separator`: (none, empty, light, normal, heavy, dashed, top, bottom, underline) Show a separator divider line between the 3 tiers (direct dirs, direct files, deep items; default `Top`).
  - `style`: [Style Settings](#style-settings) Custom style override for the tier separator line (set via `--color tier-separator:<color>`).
- `autoscroll`: Control how the results table scrolls horizontally to keep matches in view.
  - Alias: `a`.
  - `enabled`: (bool) Enable/disable horizontal autoscroll.
  - `initial_preserved`: (number) Number of characters at the start of the line to always keep visible.
  - `context`: (number) Number of characters to show around the match.
  - `end`: (bool) Whether to autoscroll to the end of the line.
- `right_align_last`: (bool) Right-align the last column.
- `border`: [Border Settings](#border-settings).

#### Status Line (`status.`)

- `style`: [Style Settings](#style-settings).
- `show`: (bool) Show/hide the status line.
- `template`: (string) The following replacements are available:
  - `\r` -> current index
  - `\c` -> current column
  - `\m` -> match count
  - `\t` -> total count
  - `\s` -> Available whitespace / #count
  - `\S` -> Increments the count denominator without displaying whitespace
- `interactions`: ([index, action]) Define interactive regions. See [Interactions](template.md#interaction-regions).

#### Preview Panel (`preview.`, `p`)

- `show`: (bool) Toggle the preview window.
- `scrollbar`: (bool) Enable scrollbar in the preview window (alias: `sb`, default: `false`).
- `scroll_wrap`: (bool) Enable scroll wrapping in preview.
- `wrap`: (bool) Enable line wrapping in preview.
- `layout`: List of preview settings. This path overrides the existing preview layouts in order.
  - Absolute alias: `l`.
  - `x`, `command`: Command to run for preview. `{}` is replaced by the item.
    - Absolute alias: `px`.
  - `layout` *(flattened)*:
    - `side`: `top`, `bottom`, `left`, `right`.
    - `percentage`: Percentage of the screen to occupy.
    - `min`, `max`: Pixel/row constraints for the preview size. On `left` or `right`, `min`/`max` restrict the preview row height. Setting `max` to 0 disables a preview layout.
    - `scroll` *(flattened)*: Initial scroll settings for this layout. See [Initial](#initial) for available fields.
- `border`: [Border Settings](#border-settings).
- `initial`: Control the initial scroll offset of the preview window.
  - Alias: `i`.
  - `index` (string, optional) – Extract the initial display index `n` of the preview window from this column. `n` lines are skipped after the header lines are consumed.
  - `o`, `offset` (integer) – Adjust the initial scroll index relative to `index`.
  - `p`, `percentage` (0-100) – How far from the bottom of the preview window the scroll offset should appear.
  - `h`, `header_lines` (number) – Keep the top N lines as a fixed header so that they are always visible.
  - `t`, `tail` (bool) – Start with the scroll at the bottom of the preview window.
- `drag`: (Optional<bool>) Width along the divider strip between the preview and results pane enabled for mouse detection dragging. 0 to disable. (default is the [border](#border-settings) width).
- `media`: Media preview configuration.
  - `active`: (bool) Whether native terminal image/media previewing is enabled (default `false`).
  - `fit`: (string) `fit`, `crop`, or `scale`.
  - `size`: (number) Pixel resolution for media previews (images, videos, PDFs; default `1280`, 0 for original).
  - `zoom`: (float) Scaling factor for media previews (default `1.0`).
  - `protocol`: (string) Image transmission protocol (`kitty`, `sixel`, `halfblocks`, `iterm2`).
- `markdown`: (bool) Whether to enable native markdown rendering (default `true`).
- `diagrams`: Diagram rendering configuration.
  - `active`: (bool) Whether embedded Mermaid diagrams are rendered (default `true`).
  - `inline`: (bool) Whether diagrams are rendered inline using Kitty Unicode Placeholders (default `true`).
  - `theme`: (string) Mermaid theme (`auto`, `dark`, `light`, `forest`, `neutral`, `base`).
  - `background`: (string) Background style (`transparent`, `theme`, `black`, `white`).

### Previewer (`previewer.`)

- `try_lossy`: (bool) Enable lossy UTF-8 conversion for preview command output.
- `delay_clear`: (bool) If true, prevents clearing the preview window until the new command starts producing output (default true).
- `debounce_ms`: (number) Debounce delay for preview commands in milliseconds (default 0).
- `max_procs`: (number) Maximum number of concurrent preview processes (default 4).
- `always_trigger`: (bool) If false, skips running the preview command if it is the same as the last one executed (default true).
- `shell`: (list of strings) The shell used to execute preview commands (e.g., `["sh", "-c"]`).
- `trim_commands`: (bool) Trim whitespace from preview commands.
- `hide_semantic_help`: (bool) Hide semantic help in the preview window (default true).
- `cache`: (number) Reserved for future use.

### Header & Footer (`header.`, `footer.`, `h`, `f`)

- `content`: (string or list) Static content to display.
  - Absolute alias: `h`.
- `style`: [Style Settings](#style-settings).
- `match_indent`: (bool) Indent content to match the results table.
- `wrap`: (bool) Enable line wrapping.
- `row_connection`: Controls the effective width of the displayed content. See [Results Table](#results-table-results-r) for variants.
- `t`, `header_lines`: (number, header only) Number of lines to read from input for the header.
- `interactions`: ([[index, action]]) Define interactive regions per line. See [Interactions](template.md#interaction-regions).
- `border`: [Border Settings](#border-settings).

### TUI Settings (`tui.`)

- `restore_fullscreen`: (bool) Restore fullscreen on exit.
- `redraw_on_resize`: (bool) Redraw the UI when the terminal is resized.
- `extended_keys`: (bool) Enable enhanced keyboard support, including modified keys such as `Shift-Space` on terminals that support the Kitty keyboard protocol.
- `sleep_ms`: (number) Delay in milliseconds before resizing.
- `clear_on_exit`: (bool) Clear the TUI screen after selection.
- `layout` *(flattened)*: Constraints for non-fullscreen mode.
  - `percentage`: Height of the terminal used.
  - `min`, `max`: Pixel constraints.
- `osc_52`: (bool). Execute the `Copy` action using the OSC52 protocol. If false, the `Copy` command pipes to `CLIPcmd` from `envs` (default: true).

### Style Settings

Several UI components have a `style` block (or similar, like `prompt_style`):

- `fg`: (color) Foreground color.
- `bg`: (color) Background color.
- `modifier`: (modifier) Style modifier (e.g., `BOLD`, `ITALIC`, `DIM`, joined by `|`).

### Border Settings

Most UI components have a `border` block:

- `type`: See https://docs.rs/matetui/latest/matetui/ratatui/widgets/enum.BorderType.html.
- `color`: CSS-style colors or named colors (e.g., `blue`, `red`, `#ff0000`).
- `bg`: Background color of the bordered area.
- `sides`: Which sides to show (e.g., `TOP | BOTTOM | LEFT | RIGHT`). Empty string for none.
- `padding`: Padding inside the border. Can be 1 value (all), 2 (vertical, horizontal), or 4 (top, right, bottom, left).
- `title`: Optional text to display on the border.
- `title_modifier`: Style modifier for the title.

### Key Binds (`binds.`, `b`)

See `wm --doc binds`.

### Frecency & Search Ranking (`matcher.` / `worker.`)

- `sort`: Sorting stability and performance settings.
  - `threshold` / `S`: (`smart`, `auto`, `stable`, `exact`, number) Sorting stability threshold.
  - `cap` / `sc`: (number) Maximum number of matched items to re-sort (default: 1000, 0 for unlimited).
- `frecency`: Frecency score boosting configuration.
  - `active` / `frecency`: (bool) Enable frecency (frequency + recency) score boosting for match ranking.
  - `weight`: (number) Multiplier for frecency bonus score (default: 1).
  - `half_life_days` / `hl`: (number) Half-life in days for continuous exponential frecency decay (default: 7). Set 0 to use legacy discrete time buckets.
- `location_bias` / `lb`: (number) Percentage bonus boost for items located inside or relative to current working directory (default: 30, i.e. +30%). Set 0 to disable.
- `depth_penalty` / `dp`: (number) Score penalty per path depth level ('/' or '\'). 0 disables.
- `typo_tolerance` / `tt`: (bool) Allow character substitutions/typo matching for queries >= 3 chars.
- `dir_first` / `df`: (bool) Prioritize direct child directories over files and deeper paths (Tier 0: direct dirs, Tier 1: direct files, Tier 2: deeper items).


### Frecency Subcommands

- `wm add <path>`: Record an access event for a file or directory path.
- `wm rank <path>`: Query current frecency score and access statistics for a path.
- `wm list [-d / --dirs / --dirs-only] [keyword1 keyword2 ...]` / `wm query`: List tracked paths sorted by frecency score descending, filtered to paths matching ALL keywords (e.g. `wm list --dirs dotfiles main`). Pass `-d` / `--dirs` to output directories only.
- `wm init <shell> [--cmd <alias>]`: Generate shell integration code (`zsh`, `bash`, `fish`, `nushell`, `powershell`) with optional custom command alias (e.g. `wm init zsh --cmd j`).
- `wm import zoxide`: Import historical directory records and scores from `zoxide`.
- `wm clean` / `wm prune`: Purge stale/deleted paths from the frecency database.
- `wm cache [path]`: Fast-index files in target directory into warm cache.
