//! Config Types.
//! See `src/bin/mm/config.rs` for an example

use std::{collections::HashMap, ffi::OsString};

use waymaker_partial_macros::partial;

pub use crate::config_types::*;
pub use crate::utils::{Percentage, serde::StringOrVec};

use crate::{
    action::{Action, Actions, NullActionExt, SortOrder},
    tui::IoStream,
    utils::serde::{escaped_opt_char, escaped_opt_string},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NavFocus {
    #[default]
    Filter,
    Picker,
}

use cba::serde::transform::{camelcase_normalized, camelcase_normalized_option};
use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{BorderType, Borders},
};

use serde::{Deserialize, Serialize};

/// Settings unrelated to event loop/picker_ui.
///
/// Does not deny unknown fields.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(recurse, path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct MatcherConfig {
    #[serde(flatten)]
    #[partial(skip)]
    pub matcher: NucleoMatcherConfig,
    #[serde(flatten)]
    pub worker: WorkerConfig,
}

/// Configuration for sorting behavior in worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct WorkerSortConfig {
    /// How "stable" the results are. Higher values prioritize the initial ordering.
    #[serde(alias = "sort_threshold", alias = "sort")]
    pub threshold: SortThreshold,
    /// Maximum number of top matched items to re-sort by frecency/depth penalty (0 = unlimited). Default is 1000.
    #[partial(alias = "sc")]
    #[serde(alias = "sort_cap")]
    pub cap: usize,
}

impl Default for WorkerSortConfig {
    fn default() -> Self {
        Self {
            threshold: SortThreshold::default(),
            cap: 1000,
        }
    }
}

pub fn deserialize_worker_sort<'de, D>(deserializer: D) -> Result<WorkerSortConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SortHelper {
        Threshold(SortThreshold),
        Config(WorkerSortConfig),
    }

    match SortHelper::deserialize(deserializer)? {
        SortHelper::Threshold(t) => Ok(WorkerSortConfig {
            threshold: t,
            ..WorkerSortConfig::default()
        }),
        SortHelper::Config(c) => Ok(c),
    }
}

/// Configuration for frecency scoring in worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct WorkerFrecencyConfig {
    /// Enable frecency (frequency + recency) score boosting for matched items.
    #[partial(alias = "frec")]
    #[serde(alias = "frecency")]
    pub active: bool,
    /// Multiplier for frecency bonus score added to matching items. Default is 1.
    #[serde(alias = "frecency_weight")]
    pub weight: u32,
    /// Half-life in days for continuous exponential frecency decay. Default is 7 days (0 switches to legacy discrete buckets).
    #[partial(alias = "hl")]
    #[serde(alias = "frecency_half_life_days")]
    pub half_life_days: u32,
}

impl Default for WorkerFrecencyConfig {
    fn default() -> Self {
        Self {
            active: false,
            weight: 1,
            half_life_days: 7,
        }
    }
}

pub fn deserialize_worker_frecency<'de, D>(deserializer: D) -> Result<WorkerFrecencyConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FrecencyHelper {
        Bool(bool),
        Config(WorkerFrecencyConfig),
    }

    match FrecencyHelper::deserialize(deserializer)? {
        FrecencyHelper::Bool(b) => Ok(WorkerFrecencyConfig {
            active: b,
            ..WorkerFrecencyConfig::default()
        }),
        FrecencyHelper::Config(c) => Ok(c),
    }
}

/// "Input/output specific". Configures the waymaker worker.
///
/// Does not deny unknown fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct WorkerConfig {
    /// Sort settings for worker.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_worker_sort", default)]
    pub sort: WorkerSortConfig,
    /// Score penalty subtracted per path depth level ('/' or '\'). 0 disables penalty.
    #[partial(alias = "dp")]
    pub depth_penalty: u32,
    /// Frecency scoring settings for worker.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_worker_frecency", default)]
    pub frecency: WorkerFrecencyConfig,
    /// Enable typo tolerance for search queries >= 3 characters.
    #[partial(alias = "tt")]
    pub typo_tolerance: bool,
    /// Prioritize direct child directories over files and deeper paths.
    #[partial(alias = "df")]
    pub dir_first: bool,
    /// TODO: Enable raw mode where non-matching items are also displayed in a dimmed color.
    #[partial(alias = "r")]
    pub raw: bool,
    /// TODO: Track the current selection when the result list is updated.
    pub track: bool,
    /// Reverse the order of the input
    pub reverse: bool, // TODO: test with sort_threshold
    /// Percentage bonus boost for items located inside or relative to current working directory (CWD). Default is 30 (i.e. +30%).
    #[partial(alias = "lb")]
    pub location_bias: u32,
    /// Fuzzy matcher engine backend ("nucleo" or "frizbee").
    #[partial(alias = "eng")]
    pub engine: MatcherEngineType,
}

/// Matcher engine backend used for fuzzy searching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatcherEngineType {
    #[default]
    Nucleo,
    Frizbee,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            sort: WorkerSortConfig::default(),
            depth_penalty: 0,
            frecency: WorkerFrecencyConfig::default(),
            typo_tolerance: false,
            dir_first: false,
            raw: false,
            track: false,
            reverse: false,
            location_bias: 30,
            engine: MatcherEngineType::default(),
        }
    }
}

/// Startup command execution configuration.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct StartCommandConfig {
    /// (cli only) Default command to execute when stdin is not being read.
    #[partial(alias = "cmd", alias = "x")]
    #[serde(alias = "command")]
    pub default: CommandSetting,
    /// (cli only) Additional command which can be cycled through using Action::ReloadNext
    #[partial(alias = "ax")]
    #[serde(alias = "additional_commands")]
    pub additional: Vec<String>,
}

pub fn deserialize_start_command<'de, D>(deserializer: D) -> Result<StartCommandConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CmdHelper {
        Setting(CommandSetting),
        Config(StartCommandConfig),
    }

    match CmdHelper::deserialize(deserializer)? {
        CmdHelper::Setting(s) => Ok(StartCommandConfig {
            default: s,
            ..StartCommandConfig::default()
        }),
        CmdHelper::Config(c) => Ok(c),
    }
}

/// Configures how input is fed to to the worker(s).
///
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct StartConfig {
    #[serde(deserialize_with = "escaped_opt_char")]
    #[partial(alias = "is")]
    pub input_separator: Option<char>,

    #[serde(deserialize_with = "escaped_opt_string")]
    #[partial(alias = "os")]
    pub output_separator: Option<String>,

    /// Format string to print accepted items as.
    #[partial(alias = "ot")]
    #[serde(alias = "output")]
    pub output_template: Option<String>,

    /// (cli only) Command and alternative commands configuration.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_start_command", default)]
    pub command: StartCommandConfig,

    #[partial(alias = "d")]
    pub directory: EnvValue,

    pub sync: bool,
    /// Whether to parse ansi sequences from input
    #[partial(alias = "a")]
    pub ansi: bool,
    /// Trim the input
    #[partial(alias = "t")]
    pub trim: bool,

    pub mode: Option<String>,

    /// Sort input lines alphabetically before injecting into the picker.
    /// Only applies when reading from stdin (not from a command).
    pub sort: bool,

    /// Reload interval in milliseconds. If set, triggers a reload action periodically.
    #[partial(alias = "ri")]
    pub reload_interval: Option<u64>,

    /// Shell executable and arguments to run start commands with.
    #[partial(alias = "sh")]
    #[serde(default)]
    pub shell: Option<Vec<String>>,

    /// (cli only) Native directory walker configuration.
    #[partial(recurse, alias = "w")]
    #[serde(default)]
    pub walker: Option<WalkerConfig>,
}

/// Configuration for native directory walker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct WalkerConfig {
    /// Directory/file names to ignore during directory traversal.
    #[serde(alias = "ignore_dirs", alias = "exclude")]
    #[partial(alias = "ignore", alias = "ignore_dirs")]
    pub ignore: Vec<String>,

    /// Additional directory/file names to ignore alongside `ignore`.
    #[serde(alias = "extra_ignore_dirs", alias = "add_ignore")]
    #[partial(alias = "extra_ignore", alias = "add_ignore")]
    pub extra_ignore: Vec<String>,

    /// Whether to walk hidden files/directories (except ignored directories). Default is true.
    #[partial(alias = "h")]
    pub hidden: bool,

    /// Whether to honor .gitignore rules. Default is true.
    #[partial(alias = "gi")]
    pub git_ignore: bool,

    /// Whether to honor git exclude rules (.git/info/exclude). Default is true.
    #[partial(alias = "ge")]
    pub git_exclude: bool,

    /// Whether to honor global gitignore rules. Default is true.
    #[partial(alias = "gg")]
    pub git_global: bool,

    /// Maximum directory depth to traverse. None = unlimited.
    #[partial(alias = "d", alias = "depth")]
    pub max_depth: Option<usize>,
}

impl Default for WalkerConfig {
    fn default() -> Self {
        Self {
            ignore: crate::walker::default_ignored_dirs(),
            extra_ignore: Vec::new(),
            hidden: true,
            git_ignore: true,
            git_exclude: true,
            git_global: true,
            max_depth: None,
        }
    }
}

impl WalkerConfig {
    pub fn effective_ignore(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        for item in &self.ignore {
            let trimmed = item.trim_end_matches('/');
            if !trimmed.is_empty() {
                set.insert(trimmed.to_string());
            }
        }
        for item in &self.extra_ignore {
            let trimmed = item.trim_end_matches('/');
            if !trimmed.is_empty() {
                set.insert(trimmed.to_string());
            }
        }
        set.into_iter().collect()
    }

    pub fn to_options(&self, root: impl AsRef<std::path::Path>) -> crate::walker::WalkerOptions {
        crate::walker::WalkerOptions {
            root: root.as_ref().to_path_buf(),
            hidden: self.hidden,
            ignore: self.git_ignore,
            git_exclude: self.git_exclude,
            git_global: self.git_global,
            max_depth: self.max_depth,
            threads: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4),
            entry_type: crate::walker::EntryType::Any,
            strip_cwd_prefix: true,
            ignore_dirs: self.effective_ignore(),
        }
    }

    pub fn merge(&mut self, other: &WalkerConfig) {
        if other.ignore != crate::walker::default_ignored_dirs() {
            self.ignore = other.ignore.clone();
        }
        self.extra_ignore.extend(other.extra_ignore.clone());
        if !other.hidden {
            self.hidden = false;
        }
        if !other.git_ignore {
            self.git_ignore = false;
        }
        if !other.git_exclude {
            self.git_exclude = false;
        }
        if !other.git_global {
            self.git_global = false;
        }
        if other.max_depth.is_some() {
            self.max_depth = other.max_depth;
        }
    }
}

/// Exit conditions of the render loop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ExitConfig {
    /// Exit automatically if there is only one match.
    #[partial(alias = "first")]
    #[serde(alias = "first")]
    pub select_1: bool,
    /// Allow returning without any items selected.
    pub allow_empty: bool,
    /// Abort if no items.
    pub abort_empty: bool,
    /// Last processed key is written here.
    /// Set to an empty path to disable.
    pub last_key_path: Option<std::path::PathBuf>,
}

impl Default for ExitConfig {
    fn default() -> Self {
        Self {
            select_1: false,
            allow_empty: false,
            abort_empty: true,
            last_key_path: None,
        }
    }
}

/// The ui config.
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(recurse, path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct RenderConfig {
    /// The default overlay style
    pub ui: UiConfig,
    /// The input bar style
    #[partial(alias = "q")]
    pub query: QueryConfig,
    /// The results table style
    #[partial(alias = "r")]
    pub results: ResultsConfig,

    /// The results status style
    pub status: StatusConfig,
    /// The preview panel style
    #[partial(alias = "p")]
    pub preview: PreviewConfig,
    #[partial(alias = "f")]
    pub footer: DisplayConfig,
    #[partial(alias = "h")]
    pub header: DisplayConfig,
    /// Action dialog box above the filter input.
    pub action: ActionBoxConfig,
    /// Breadcrumb showing the current directory.
    pub breadcrumb: BreadcrumbConfig,
}

impl RenderConfig {
    pub fn tick_rate(&self) -> u8 {
        self.ui.tick_rate
    }
}

/// Terminal settings.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    pub stream: IoStream, // consumed
    pub restore_fullscreen: bool,
    pub redraw_on_resize: bool,
    // https://docs.rs/crossterm/latest/crossterm/event/struct.PushKeyboardEnhancementFlags.html
    pub extended_keys: bool,
    pub sleep_ms: u64, // necessary to give ratatui a small delay before resizing after entering and exiting
    #[serde(flatten)]
    #[partial(recurse)]
    pub layout: Option<TerminalLayoutSettings>, // None for fullscreen
    pub clear_on_exit: bool,

    // unimplemented: currently favoring Execute2
    pub clear_after_execute: bool,

    /// Whether to use OSC 52 for clipboard copying.
    pub osc52: bool,
    /// Whether to enable mouse capture.
    pub mouse: bool,
    /// Whether to drop the end of the output of the copy command if it is a new line
    pub copy_trailing_newline: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            stream: IoStream::default(),
            restore_fullscreen: true,
            redraw_on_resize: bool::default(),
            sleep_ms: 100,
            layout: Option::default(),
            extended_keys: true,
            clear_on_exit: true,
            clear_after_execute: true,
            osc52: true,
            mouse: true,
            copy_trailing_newline: false,
        }
    }
}

/// Navigation profile controlling which default bindings and hints are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NavProfile {
    /// Safe list navigation (cursor movements, selection, paging, filter toggle). No file mutations, no directory hopping.
    #[default]
    #[serde(alias = "picker", alias = "safe")]
    List,
    /// Minimal vertical navigation (j/k, gg/G, J/K, esc, q). Directory hopping silenced.
    #[serde(alias = "minimal")]
    Basic,
    /// Full file-manager navigation (directory hopping with h/l, frecency, bookmarks, and file creation/renaming/deletion actions).
    #[serde(alias = "filemanager", alias = "file_manager", alias = "jump")]
    Fm,
    /// No default bindings injected; strictly uses whatever is defined in `[ui.nav.binds]`.
    #[serde(alias = "strict", alias = "empty")]
    None,
}

///// Configuration for Navigation mode (`--nav`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct NavConfig {
    /// Enable navigation mode: pressing `ToggleFocus` switches keyboard focus
    /// between the input bar and the results list.
    #[partial(alias = "fm")]
    #[serde(alias = "nav_mode", alias = "focus_mode", alias = "mode")]
    pub active: bool,

    /// Navigation profile: "list" (default safe picker), "basic" (minimal vertical),
    /// "fm" (file manager with create/rename/etc.), or "none" (strict zero-defaults).
    #[serde(alias = "nav_profile")]
    #[serde(default)]
    pub profile: NavProfile,

    /// Opt-in flag to enable file manager actions and directory traversal.
    /// Equivalent to setting `profile = "fm"`.
    #[serde(alias = "nav_fm")]
    pub fm: bool,

    /// Navigation-mode indicator colour (set via `--color nav:` or `--nav color:`).
    #[serde(deserialize_with = "camelcase_normalized")]
    #[serde(alias = "nav_color", alias = "focus_color")]
    pub color: Color,

    /// Make the navigation indicator blink.
    #[partial(alias = "fm_blink")]
    #[serde(alias = "nav_blink", alias = "focus_blink")]
    pub blink: bool,

    /// Blink speed when `nav_blink = true`.
    #[partial(alias = "fm_blink_rate")]
    #[serde(alias = "nav_blink_rate", alias = "focus_blink_rate")]
    pub blink_rate: BlinkRate,

    /// Apply bold styling to navigation indicator.
    #[partial(alias = "fm_bold")]
    #[serde(alias = "nav_bold", alias = "focus_bold")]
    pub bold: bool,

    /// Left-bar border style for the navigation indicator.
    #[partial(alias = "fm_bar")]
    #[serde(alias = "nav_bar", alias = "focus_bar")]
    pub bar: Option<BorderType>,

    /// Marker rendered on the current result row when results pane is focused.
    #[partial(alias = "fm_marker")]
    #[serde(alias = "nav_marker", alias = "focus_marker")]
    pub marker: String,

    /// Prompt text shown while the results pane is focused.
    #[partial(alias = "fm_prompt")]
    #[serde(alias = "nav_prompt", alias = "focus_prompt")]
    pub prompt: String,

    /// Key bindings active while navigation mode is enabled and results pane has focus.
    #[serde(alias = "nav_binds", alias = "focus_binds")]
    #[partial(no_recurse, unwrap)]
    pub binds: HashMap<String, Actions<NullActionExt>>,

    /// Show notifications for file-manager clipboard actions.
    #[serde(alias = "nav_notify", alias = "focus_notify", alias = "fm_notify")]
    pub notify: bool,

    /// When true, characters typed while the results pane is focused bypass the
    /// nav-bind intercept and fall through to the query input.  Useful when you
    /// want the navigation-mode visual indicators but still want live filtering
    /// while scrolling results.  Enable with `--nav no-filter` or in TOML as
    /// `nav_passthrough = true`.
    #[serde(alias = "nav_passthrough")]
    pub passthrough: bool,

    /// When true only the basic j / k / J / K navigation binds are active;
    /// the h / l / gg / G / gb / gt binds are silenced and the file-manager
    /// semantic binds (d, a, r, …) are not injected.  Enable with
    /// `--nav basic` or in TOML as `nav_basic = true`.
    #[serde(alias = "nav_basic")]
    pub basic: bool,

    #[partial(alias = "fm_focus_on_start")]
    #[serde(alias = "nav_focus_on_start", alias = "focus_on_start")]
    #[serde(default)]
    pub focus_on_start: NavFocus,

    /// Show keybinding hints in footer/status when Results pane is focused in navigation mode.
    #[serde(alias = "nav_hints", alias = "focus_hints", alias = "fm_hints")]
    pub hints: bool,

    /// Number of columns to display in the navigation hints grid in the footer.
    /// If 0 or 1, renders as a single row.
    /// Default: 4.
    #[serde(alias = "nav_hints_columns", alias = "hints_columns")]
    pub hints_columns: usize,
}

impl Default for NavConfig {
    fn default() -> Self {
        Self {
            active: false,
            color: Color::Yellow,
            blink: false,
            blink_rate: BlinkRate::Normal,
            bold: false,
            bar: None,
            marker: "".to_string(),
            prompt: "".to_string(),
            binds: Self::default_binds_for_profile(NavProfile::List),
            notify: false,
            passthrough: false,
            basic: false,
            profile: NavProfile::List,
            fm: false,
            focus_on_start: NavFocus::Filter,
            hints: true,
            hints_columns: 4,
        }
    }
}

impl NavConfig {
    /// Resolves the effective navigation profile considering `profile`, `fm`, and `basic`.
    pub fn effective_profile(&self) -> NavProfile {
        if self.fm {
            NavProfile::Fm
        } else if self.basic && self.profile == NavProfile::List {
            NavProfile::Basic
        } else {
            self.profile
        }
    }

    /// Returns the default keybindings corresponding to the specified navigation profile.
    pub fn default_binds_for_profile(
        profile: NavProfile,
    ) -> HashMap<String, Actions<NullActionExt>> {
        let mut binds = HashMap::new();
        if profile == NavProfile::None {
            return binds;
        }

        // Shared navigation binds (Basic, List, Fm)
        binds.insert("j".to_string(), Actions::from([Action::Down(1)]));
        binds.insert("k".to_string(), Actions::from([Action::Up(1)]));
        binds.insert("J".to_string(), Actions::from([Action::PreviewDown(1)]));
        binds.insert("K".to_string(), Actions::from([Action::PreviewUp(1)]));
        binds.insert("gg".to_string(), Actions::from([Action::PreviewUp(0)]));
        binds.insert("G".to_string(), Actions::from([Action::PreviewDown(0)]));
        binds.insert("/".to_string(), Actions::from([Action::FocusFilter]));
        binds.insert("esc".to_string(), Actions::from([Action::Quit(130)]));
        binds.insert("q".to_string(), Actions::from([Action::Quit(130)]));

        if profile == NavProfile::Basic {
            return binds;
        }

        // List and Fm navigation binds
        binds.insert("gb".to_string(), Actions::from([Action::Pos(-1)]));
        binds.insert("gt".to_string(), Actions::from([Action::Pos(0)]));
        binds.insert(" ".to_string(), Actions::from([Action::Toggle]));
        binds.insert("\\".to_string(), Actions::from([Action::ToggleParentPeek]));
        binds.insert("|".to_string(), Actions::from([Action::ToggleParentPeek]));
        binds.insert(",".to_string(), Actions::from([Action::SortMenu]));
        binds.insert(".".to_string(), Actions::from([Action::NextColumn]));
        binds.insert(">".to_string(), Actions::from([Action::PrevColumn]));

        if profile == NavProfile::Fm {
            binds.insert(
                "l".to_string(),
                Actions::from([
                    Action::ChDir("{=}".to_string()),
                    Action::Reload("".to_string()),
                    Action::Pos(0),
                ]),
            );
            binds.insert(
                "h".to_string(),
                Actions::from([
                    Action::ChDir("..".to_string()),
                    Action::Reload("".to_string()),
                    Action::Pos(0),
                ]),
            );
            binds.insert(
                "f".to_string(),
                Actions::from([Action::Semantic("frecency".to_string())]),
            );
            binds.insert(
                "b".to_string(),
                Actions::from([Action::Semantic("bookmarks".to_string())]),
            );
            binds.insert(
                "*".to_string(),
                Actions::from([Action::Semantic("bookmark".to_string())]),
            );
        }

        binds
    }
    /// Calculate the required height in rows for the navigation hints footer.
    pub fn hints_height(&self, item_count: usize) -> u16 {
        if !self.hints {
            return 0;
        }
        let cols = if self.hints_columns == 0 {
            4
        } else {
            self.hints_columns
        };
        if cols <= 1 {
            1
        } else {
            let rows = (item_count + cols - 1) / cols;
            rows.max(1) as u16
        }
    }
}

pub fn deserialize_nav<'de, D>(deserializer: D) -> Result<NavConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NavHelper {
        Bool(bool),
        Config(NavConfig),
    }

    match NavHelper::deserialize(deserializer)? {
        NavHelper::Bool(b) => Ok(NavConfig {
            active: b,
            ..NavConfig::default()
        }),
        NavHelper::Config(c) => Ok(c),
    }
}

/// The container ui.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct UiConfig {
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,
    pub tick_rate: u8, // separate from render, but best place ig

    /// Navigation mode (`--nav`) settings.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_nav", default)]
    pub nav: NavConfig,

    /// Configuration for the 3-pane Parent Peek left directory pane.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_parent_peek", default)]
    pub parent_peek: ParentPeekConfig,

    /// Configuration for the sort options menu in the footer.
    #[partial(recurse)]
    #[serde(default)]
    pub sort_menu: SortMenuConfig,

    /// Folder-specific rules for automatic sorting by directory.
    #[serde(default)]
    #[serde(alias = "sort_rules")]
    #[partial(no_recurse, unwrap)]
    pub folder_rules: Vec<FolderRule>,

    /// Global default sort order.
    #[serde(default)]
    pub default_sort: Option<SortOrder>,
}

impl UiConfig {
    /// Resolve the sort order for a given directory based on folder_rules or default_sort.
    pub fn resolve_sort_for_dir(&self, dir: &std::path::Path) -> Option<SortOrder> {
        for rule in &self.folder_rules {
            if rule.matches(dir) {
                return Some(rule.sort);
            }
        }
        self.default_sort
    }

    /// Calculate the required height in rows for the navigation hints footer.
    pub fn nav_hints_height(&self, item_count: usize) -> u16 {
        self.nav.hints_height(item_count)
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            border: BorderSetting::default(),
            tick_rate: 15,
            nav: NavConfig::default(),
            parent_peek: ParentPeekConfig::default(),
            sort_menu: SortMenuConfig::default(),
            folder_rules: Vec::new(),
            default_sort: None,
        }
    }
}

/// Configuration for query filtering mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct QueryFilterConfig {
    /// Style for the query prompt when in filter mode (focused).
    #[partial(recurse)]
    #[serde(alias = "filter_prompt_style")]
    pub prompt_style: StyleSetting,

    /// Style for the query text when in filter mode (focused).
    #[partial(recurse)]
    #[serde(alias = "filter_style")]
    pub style: StyleSetting,

    /// Prompt prefix when in filter mode (focused).
    #[serde(alias = "filter_prompt")]
    pub prompt: Option<String>,

    /// Whether the underline separator is shown when in filter mode (focused).
    /// If None, inherits from `underline`.
    #[serde(alias = "filter_underline")]
    pub underline: Option<bool>,

    /// Style and color of the query underline separator when in filter mode (focused).
    #[partial(recurse)]
    #[serde(alias = "filter_underline_style")]
    pub underline_style: StyleSetting,
}

impl Default for QueryFilterConfig {
    fn default() -> Self {
        Self {
            prompt_style: StyleSetting {
                modifier: Modifier::empty(),
                ..Default::default()
            },
            style: StyleSetting {
                modifier: Modifier::empty(),
                ..Default::default()
            },
            prompt: None,
            underline: None,
            underline_style: StyleSetting {
                modifier: Modifier::empty(),
                ..Default::default()
            },
        }
    }
}

/// Prompt and style configuration for query modes (local, frecency, bookmarks).
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct QueryModeConfig {
    /// Mode-specific prompt text.
    pub prompt: Option<String>,
    /// Style for the mode-specific prompt.
    #[partial(recurse)]
    pub prompt_style: StyleSetting,
    /// Style for the underline separator in this mode.
    #[partial(recurse)]
    pub underline_style: StyleSetting,
}

/// The query (input) bar ui.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct QueryConfig {
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,

    // text styles
    #[partial(recurse)]
    pub style: StyleSetting,

    #[partial(recurse)]
    pub prompt_style: StyleSetting,

    /// The prompt prefix.
    #[serde(deserialize_with = "deserialize_string_or_char_as_double_width")]
    pub prompt: String,

    /// Cursor style.
    pub cursor: CursorSetting,

    /// Initial text in the input bar.
    #[partial(alias = "i")]
    pub initial: String,

    /// Maintain padding when moving the cursor in the bar.
    pub scroll_padding: bool,

    /// When true, display the match status (e.g. "3/42") right-aligned
    /// inside the input bar instead of in the separate status row.
    pub status_inline: bool,

    /// Whether to show the filter input bar.
    pub show: bool,

    /// Underline separator drawn directly across the query filter row (0 extra vertical lines).
    pub underline: bool,

    /// Style and color of the query underline separator.
    #[partial(recurse)]
    pub underline_style: StyleSetting,

    /// Filter-mode configuration.
    #[partial(recurse)]
    pub filter: QueryFilterConfig,

    /// Prompt and styling for local mode (index 0).
    #[partial(recurse)]
    pub local: QueryModeConfig,

    /// Prompt and styling for frecency mode (index 1).
    #[partial(recurse)]
    pub frecency: QueryModeConfig,

    /// Prompt and styling for bookmarks mode (index 2).
    #[partial(recurse)]
    pub bookmarks: QueryModeConfig,
}

impl Default for QueryConfig {
    fn default() -> Self {
        Self {
            border: Default::default(),
            style: Default::default(),
            prompt_style: StyleSetting {
                modifier: Modifier::empty(),
                ..Default::default()
            },
            prompt: "> ".to_string(),
            cursor: CursorSetting::default(),
            initial: Default::default(),

            scroll_padding: true,
            status_inline: false,
            show: true,
            underline: false,
            underline_style: StyleSetting {
                modifier: Modifier::empty(),
                ..Default::default()
            },

            filter: QueryFilterConfig::default(),
            local: QueryModeConfig {
                prompt: None,
                prompt_style: Default::default(),
                underline_style: Default::default(),
            },
            frecency: QueryModeConfig {
                prompt: Some("󱅤 ".to_string()),
                prompt_style: StyleSetting {
                    fg: Some(Color::Blue),
                    ..Default::default()
                },
                underline_style: Default::default(),
            },
            bookmarks: QueryModeConfig {
                prompt: Some(" ".to_string()),
                prompt_style: StyleSetting {
                    fg: Some(Color::Yellow),
                    ..Default::default()
                },
                underline_style: Default::default(),
            },
        }
    }
}

impl QueryConfig {}

/// Configuration for the action dialog box shown above the filter input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ActionBoxConfig {
    /// Width of the action box as a percentage of the terminal width (0–100). Default: 100.
    pub width_pct: Percentage,
    /// Number of extra lines reserved below the input for a preview area. Default: 0 (no preview).
    pub preview_height: u16,
    /// Border drawn at the bottom of the action box, acting as a visual separator.
    /// Defaults to a plain bottom line. Set `color` to colorize it.
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,
}

impl Default for ActionBoxConfig {
    fn default() -> Self {
        Self {
            width_pct: Percentage::new(100),
            preview_height: 0,
            border: BorderSetting::default(),
        }
    }
}

/// Configuration for the breadcrumb showing current path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct BreadcrumbConfig {
    /// Show a breadcrumb on top.
    pub show: bool,
    /// Separator used in the breadcrumb.
    pub separator: String,
    /// Style of the breadcrumb text.
    #[partial(recurse)]
    pub style: StyleSetting,
    /// Style of the breadcrumb separator.
    #[partial(recurse)]
    pub separator_style: StyleSetting,
    /// Maximum length for intermediate folder names. 0 means no truncation.
    pub truncate_length: usize,
    /// If true, only show the current folder name.
    pub current_folder_only: bool,
}

impl Default for BreadcrumbConfig {
    fn default() -> Self {
        Self {
            show: false,
            separator: "  ".to_string(),
            style: StyleSetting {
                fg: Some(Color::Cyan),
                modifier: Modifier::BOLD,
                ..Default::default()
            },
            separator_style: StyleSetting {
                fg: Some(Color::DarkGray),
                ..Default::default()
            },
            truncate_length: 0,
            current_folder_only: false,
        }
    }
}

/// Border configuration for the Parent Peek left pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ParentPeekBorderConfig {
    /// Show or hide the right border line (default: true).
    pub show: bool,
    /// Border color (default: DarkGray).
    #[serde(deserialize_with = "camelcase_normalized_option", default)]
    pub color: Option<Color>,
    /// Border style type (Plain, Rounded, Double, Thick).
    #[serde(deserialize_with = "camelcase_normalized_option", default)]
    pub r#type: Option<BorderType>,
}

impl Default for ParentPeekBorderConfig {
    fn default() -> Self {
        Self {
            show: true,
            color: Some(Color::DarkGray),
            r#type: Some(BorderType::Plain),
        }
    }
}

/// Configuration for the 3-pane Parent Peek left directory pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ParentPeekConfig {
    /// Enable 3-pane Parent Peek layout (default: false).
    pub enabled: bool,
    /// Width percentage of the parent peek pane (default: 15%).
    pub pct: Percentage,
    /// Maximum row height for the parent peek pane (default: None).
    #[serde(alias = "max_rows")]
    pub max: Option<u16>,
    /// Parent folder title color (default: Cyan).
    #[serde(deserialize_with = "camelcase_normalized_option", default)]
    pub parent_color: Option<Color>,
    /// Whether to highlight the current directory in the parent list (default: true).
    pub highlight: bool,
    /// Highlight color for the current directory (default: Yellow).
    #[serde(deserialize_with = "camelcase_normalized_option", default)]
    pub highlight_color: Option<Color>,
    /// Border settings for the parent peek pane.
    #[partial(recurse)]
    pub border: ParentPeekBorderConfig,
}

impl Default for ParentPeekConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            pct: Percentage::new(15),
            max: None,
            parent_color: Some(Color::Cyan),
            highlight: true,
            highlight_color: Some(Color::Yellow),
            border: ParentPeekBorderConfig::default(),
        }
    }
}

pub fn deserialize_parent_peek<'de, D>(deserializer: D) -> Result<ParentPeekConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ParentPeekHelper {
        Bool(bool),
        Config(ParentPeekConfig),
    }

    match ParentPeekHelper::deserialize(deserializer)? {
        ParentPeekHelper::Bool(b) => Ok(ParentPeekConfig {
            enabled: b,
            ..ParentPeekConfig::default()
        }),
        ParentPeekHelper::Config(c) => Ok(c),
    }
}

pub fn deserialize_optional_parent_peek<'de, D>(
    deserializer: D,
) -> Result<Option<ParentPeekConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Helper {
        Bool(bool),
        Config(ParentPeekConfig),
    }

    match Option::<Helper>::deserialize(deserializer)? {
        Some(Helper::Bool(b)) => Ok(Some(ParentPeekConfig {
            enabled: b,
            ..Default::default()
        })),
        Some(Helper::Config(c)) => Ok(Some(c)),
        None => Ok(None),
    }
}

/// Configuration for the interactive sort options menu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct SortMenuConfig {
    /// Number of columns to display in the sort menu grid (e.g. 2, 3, 4).
    /// If set to 0 or 1, it renders as a single horizontal line.
    /// Default: 4.
    pub columns: usize,
    /// Whether to display the "Sort:" title header.
    pub show_title: bool,
}

impl Default for SortMenuConfig {
    fn default() -> Self {
        Self {
            columns: 4,
            show_title: true,
        }
    }
}

impl SortMenuConfig {
    /// Calculate the required height in rows for the sort menu.
    pub fn height(&self, item_count: usize) -> u16 {
        if self.columns <= 1 {
            1
        } else {
            let rows = (item_count + self.columns - 1) / self.columns;
            rows.max(1) as u16
        }
    }
}

/// Rule for automatically applying a sort order to matching folders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct FolderRule {
    /// Directory path or glob pattern (supports `~`, absolute paths, and globs like `*/screenshots`).
    pub path: String,
    /// Sort order to apply when entering this directory.
    pub sort: crate::action::SortOrder,
}

impl Default for FolderRule {
    fn default() -> Self {
        Self {
            path: "".to_string(),
            sort: crate::action::SortOrder::Natural,
        }
    }
}

impl FolderRule {
    pub fn new(path: impl Into<String>, sort: crate::action::SortOrder) -> Self {
        Self {
            path: path.into(),
            sort,
        }
    }

    /// Check if the rule matches the given directory path.
    pub fn matches(&self, dir: &std::path::Path) -> bool {
        if self.path.is_empty() {
            return false;
        }

        let expanded_pattern = if self.path.starts_with("~/") || self.path == "~" {
            if let Some(home) = dirs::home_dir() {
                if self.path == "~" {
                    home.to_string_lossy().to_string()
                } else {
                    format!("{}{}", home.to_string_lossy(), &self.path[1..])
                }
            } else {
                self.path.clone()
            }
        } else {
            self.path.clone()
        };

        let rule_path_str = expanded_pattern.trim_end_matches('/');
        let dir_str = dir.to_string_lossy();
        let dir_path_str = dir_str.trim_end_matches('/');

        // 1. Direct path equality
        if rule_path_str == dir_path_str {
            return true;
        }

        // 2. Canonicalized path equality
        if let (Ok(c_rule), Ok(c_dir)) = (
            std::fs::canonicalize(&expanded_pattern),
            std::fs::canonicalize(dir),
        ) {
            if c_rule == c_dir {
                return true;
            }
        }

        // 3. Glob matching against full expanded path
        if let Ok(pattern) = glob::Pattern::new(rule_path_str) {
            if pattern.matches(dir_path_str) {
                return true;
            }
        }

        // 4. Glob matching against folder name (basename)
        if let Ok(pattern) = glob::Pattern::new(&self.path) {
            if let Some(file_name) = dir.file_name().and_then(|f| f.to_str()) {
                if pattern.matches(file_name) {
                    return true;
                }
            }
        }

        false
    }
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct OverlayConfig {
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,
    pub outer_dim: bool,
    pub layout: OverlayLayoutSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct OverlayLayoutSettings {
    /// w, h
    #[partial(alias = "p")]
    pub percentage: [Percentage; 2],
    /// w, h
    pub min: [u16; 2],
    /// w, h
    pub max: [u16; 2],

    /// y_offset as a percentage of total height: 50 for neutral, (default: 55)
    pub y_offset: Percentage,
}

impl Default for OverlayLayoutSettings {
    fn default() -> Self {
        Self {
            percentage: [Percentage::new(60), Percentage::new(30)],
            min: [10, 10],
            max: [200, 30],
            y_offset: Percentage::new(55),
        }
    }
}

// pub struct OverlaySize

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AutoscrollSettings {
    /// Number of characters at the start of the line to always keep visible.
    #[partial(alias = "i")]
    pub initial_preserved: usize,
    /// Enable/disable horizontal autoscroll.
    #[partial(alias = "a")]
    pub enabled: bool,
    /// Number of characters to show around the match.
    #[partial(alias = "c")]
    pub context: usize,
    /// Whether to autoscroll to the end of the line.
    #[partial(alias = "e")]
    pub end: bool,
    /// Enable autoscroll even when wrap = true. Ignored if enable = false.
    pub always: bool,
    /// The string to show when text is horizontally scrolled.
    pub indicator: String,
    /// Style of the hscroll indicator.
    pub indicator_style: StyleSetting,
}

impl Default for AutoscrollSettings {
    fn default() -> Self {
        Self {
            initial_preserved: 0,
            enabled: true,
            context: 4,
            end: false,
            always: false,
            indicator: "..".to_string(),
            indicator_style: crate::config_types::StyleSetting {
                fg: Some(ratatui::style::Color::DarkGray),
                ..Default::default()
            },
        }
    }
}

/// Symlink target display configuration.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultsSymlinkConfig {
    /// Show symlink targets appended to the first column text.
    /// Defaults to false.
    #[serde(alias = "symlink_target", alias = "target")]
    pub active: bool,
    /// Style for the appended symlink target text.
    #[partial(recurse)]
    #[serde(alias = "symlink_target_style")]
    pub style: StyleSetting,
}

impl Default for ResultsSymlinkConfig {
    fn default() -> Self {
        Self {
            active: false,
            style: StyleSetting {
                fg: Some(Color::DarkGray),
                ..Default::default()
            },
        }
    }
}

pub fn deserialize_results_symlink<'de, D>(deserializer: D) -> Result<ResultsSymlinkConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SymlinkHelper {
        Bool(bool),
        Config(ResultsSymlinkConfig),
    }

    match SymlinkHelper::deserialize(deserializer)? {
        SymlinkHelper::Bool(b) => Ok(ResultsSymlinkConfig {
            active: b,
            ..ResultsSymlinkConfig::default()
        }),
        SymlinkHelper::Config(c) => Ok(c),
    }
}

/// Tier separator line configuration for 3-tiered sort when dir_first is enabled.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultsTierConfig {
    /// Horizontal separator style drawn between the 3 tiers (direct dirs, direct files, deep items)
    /// when dir_first is enabled. Defaults to Top ("▔").
    #[serde(deserialize_with = "camelcase_normalized")]
    #[serde(alias = "tier_separator")]
    pub separator: HorizontalSeparator,
    /// Style override for the tier separator line.
    #[partial(recurse)]
    #[serde(alias = "tier_separator_style")]
    pub style: StyleSetting,
}

impl Default for ResultsTierConfig {
    fn default() -> Self {
        Self {
            separator: HorizontalSeparator::Top,
            style: StyleSetting {
                fg: Some(Color::DarkGray),
                ..Default::default()
            },
        }
    }
}

/// Icon configuration for pinned / bookmarked items.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultsBookmarkConfig {
    /// Icon for pinned / bookmarked items.
    #[serde(alias = "bookmark_icon")]
    pub icon: Option<String>,
    /// Icon for pinned / bookmarked file items. Defaults to Some("󱀻".to_string()).
    #[serde(alias = "bookmark_file_icon")]
    pub file_icon: Option<String>,
    /// Icon for pinned / bookmarked folder items. Defaults to Some("󰮟".to_string()).
    #[serde(alias = "bookmark_folder_icon")]
    pub folder_icon: Option<String>,
    /// Style / color for the bookmark icon. Defaults to Yellow.
    #[partial(recurse)]
    #[serde(alias = "bookmark_icon_style")]
    pub icon_style: StyleSetting,
    /// Style / color for the bookmark file icon. Defaults to Yellow.
    #[partial(recurse)]
    #[serde(alias = "bookmark_file_icon_style")]
    pub file_icon_style: StyleSetting,
    /// Style / color for the bookmark folder icon. Defaults to Yellow.
    #[partial(recurse)]
    #[serde(alias = "bookmark_folder_icon_style")]
    pub folder_icon_style: StyleSetting,
}

impl Default for ResultsBookmarkConfig {
    fn default() -> Self {
        Self {
            icon: None,
            file_icon: Some("󱀻".to_string()),
            folder_icon: Some("󰮟".to_string()),
            icon_style: StyleSetting {
                fg: Some(Color::Yellow),
                ..Default::default()
            },
            file_icon_style: Default::default(),
            folder_icon_style: Default::default(),
        }
    }
}

/// Icon configuration for frecency-scored items.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultsFrecencyConfig {
    /// Icon for frecency folder items. Defaults to Some("󰪻".to_string()).
    #[serde(alias = "frecency_folder_icon")]
    pub folder_icon: Option<String>,
    /// Style / color for the frecency folder icon. Defaults to Blue.
    #[partial(recurse)]
    #[serde(alias = "frecency_folder_icon_style")]
    pub folder_icon_style: StyleSetting,
    /// Icon for frecency general / file items. Defaults to Some("󱋢".to_string()).
    #[serde(alias = "frecency_icon")]
    pub icon: Option<String>,
    /// Style / color for the frecency item icon. Defaults to Blue.
    #[partial(recurse)]
    #[serde(alias = "frecency_icon_style")]
    pub icon_style: StyleSetting,
}

impl Default for ResultsFrecencyConfig {
    fn default() -> Self {
        Self {
            folder_icon: Some("󰪻".to_string()),
            folder_icon_style: StyleSetting {
                fg: Some(Color::Blue),
                ..Default::default()
            },
            icon: Some("󱋢".to_string()),
            icon_style: StyleSetting {
                fg: Some(Color::Blue),
                ..Default::default()
            },
        }
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResultsConfig {
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,

    // prefixes
    #[serde(deserialize_with = "deserialize_string_or_char_as_double_width")]
    pub multi_prefix: String,
    pub unselected_prefix: String,
    pub default_prefix: String,

    pub spinner_prefix: String,
    pub spinner: String,
    pub spinner_inline: bool,

    #[serde(alias = "prefix")]
    #[partial(recurse)]
    pub prefix_style: StyleSetting,

    #[serde(alias = "prefix_inactive")]
    #[partial(recurse)]
    pub prefix_inactive_style: StyleSetting,

    #[partial(recurse)]
    pub unselected_prefix_style: StyleSetting,

    #[partial(recurse)]
    pub spinner_style: StyleSetting,

    /// Enable selections
    pub multi: bool,

    // text styles
    #[partial(recurse)]
    pub style: StyleSetting,

    // inactive_col styles
    #[serde(alias = "inactive")]
    #[partial(recurse)]
    pub inactive_style: StyleSetting,

    // inactive_col styles on the current item
    #[serde(alias = "inactive_current")]
    #[partial(recurse)]
    pub inactive_current_style: StyleSetting,

    #[serde(alias = "match")]
    #[partial(recurse)]
    pub match_style: StyleSetting,

    /// current item style
    #[serde(alias = "current")]
    #[partial(recurse)]
    pub current_style: StyleSetting,

    /// How the styles are applied across the row:
    /// Disjoint: Styles are applied per column.
    /// Capped: The inactive styles are applied per row, and the active styles applied on the active column.
    /// Full: Inactive column styles are ignored, the current style is applied on the current row.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub row_connection: RowConnectionStyle,

    // scroll
    #[partial(alias = "c")]
    #[serde(alias = "cycle")]
    pub scroll_wrap: bool,
    #[partial(alias = "sp")]
    pub scroll_padding: u16,
    #[partial(alias = "r")]
    pub reverse: Option<bool>,

    // wrap
    #[partial(alias = "w")]
    pub wrap: bool,
    pub min_width: u16,

    // autoscroll
    #[partial(recurse, alias = "a")]
    pub autoscroll: AutoscrollSettings,

    // ------------
    // experimental
    // ------------
    pub column_spacing: Count,
    pub current_prefix: String,

    /// Maximum row height.
    /// VScroll/Preview can still be used to view the whole result.
    pub max_height: usize,
    #[serde(alias = "max")]
    pub max_rows: Option<u16>,
    pub show_skipped: bool,
    /// Always false if max_height is set
    pub vscroll_current_only: bool,

    // lowpri: maybe space-around/space-between instead?
    #[partial(alias = "ra")]
    pub right_align_last: bool,
    #[partial(alias = "v")]
    #[serde(alias = "vertical")]
    pub stacked_columns: bool,

    #[serde(alias = "hr")]
    #[serde(deserialize_with = "camelcase_normalized")]
    pub separator: HorizontalSeparator,
    pub separator_style: StyleSetting,

    /// Prepend a file-type icon (Nerd Font) before the first column text.
    /// Requires a Nerd Font in the terminal. Defaults to false.
    pub icons: bool,
    /// Whether file icons on the focused/cursor row should lose their individual color
    /// to match the cursor line highlight (similar to Yazi). Defaults to false.
    pub uncolor_current_icon: bool,
    /// Whether file icons on the focused/cursor row should invert their color
    /// (e.g. Blue <-> Yellow, Red <-> Cyan, Green <-> Magenta). Defaults to false.
    pub invert_current_icon: bool,
    /// Optional custom style override for the icon on the focused/cursor row.
    #[partial(recurse)]
    pub current_icon_style: StyleSetting,

    /// Border type / thickness override for the navigation bar on the focused/cursor row
    /// (e.g. "Thick", "Plain", "Double", "Rounded", "QuadrantOutside"). Defaults to None.
    pub current_nav_bar: Option<BorderType>,
    /// Optional custom style override for the navigation bar cell on the focused/cursor row.
    #[partial(recurse)]
    pub current_nav_bar_style: StyleSetting,

    /// Symlink display settings.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_results_symlink", default)]
    pub symlink: ResultsSymlinkConfig,

    /// Style for multi-selected rows (not the cursor row).
    #[partial(recurse)]
    pub selected_style: StyleSetting,
    /// Style for the prefix marker of multi-selected rows.
    #[partial(recurse)]
    pub selected_prefix_style: StyleSetting,
    /// Style for the prefix marker of yanked rows (FM mode).
    #[partial(recurse)]
    pub yank_prefix_style: StyleSetting,
    /// Style for the prefix marker of cut rows (FM mode).
    #[partial(recurse)]
    pub cut_prefix_style: StyleSetting,

    /// Style for group header rows (set via --color group-header:…)
    #[partial(recurse)]
    pub group_header_style: StyleSetting,

    /// Tier separator settings.
    #[partial(recurse)]
    pub tier: ResultsTierConfig,

    /// Initial cursor position (0-based index or negative for from-the-end).
    pub pos: Option<i32>,

    /// Bookmark icons and styles.
    #[partial(recurse)]
    pub bookmark: ResultsBookmarkConfig,

    /// Frecency icons and styles.
    #[partial(recurse)]
    pub frecency: ResultsFrecencyConfig,
}

impl Default for ResultsConfig {
    fn default() -> Self {
        ResultsConfig {
            pos: None,
            border: Default::default(),

            multi_prefix: "▌ ".to_string(),
            unselected_prefix: "  ".to_string(),
            default_prefix: Default::default(),
            spinner_prefix: "".to_string(),
            spinner: "dot".to_string(),
            spinner_inline: false,
            prefix_style: Default::default(),
            prefix_inactive_style: Default::default(),
            unselected_prefix_style: Default::default(),
            spinner_style: Default::default(),
            multi: true,

            style: Default::default(),
            inactive_style: Default::default(),

            inactive_current_style: StyleSetting {
                // fg: Some(Color::DarkGray),
                // bg: Some(Color::Black),
                ..Default::default()
            },

            match_style: StyleSetting {
                fg: Some(Color::Green),
                modifier: Modifier::ITALIC,
                ..Default::default()
            },

            current_style: StyleSetting {
                bg: Some(Color::Black),
                modifier: Modifier::BOLD,
                ..Default::default()
            },

            row_connection: RowConnectionStyle::Capped,

            scroll_wrap: false,
            scroll_padding: 2,
            reverse: None,

            wrap: false,
            min_width: 2,
            max_height: 0,
            max_rows: None,

            autoscroll: Default::default(),

            column_spacing: Default::default(),
            current_prefix: Default::default(),
            right_align_last: false,
            stacked_columns: false,
            separator: Default::default(),
            separator_style: Default::default(),
            show_skipped: true,
            vscroll_current_only: true,
            icons: false,
            uncolor_current_icon: false,
            invert_current_icon: false,
            current_icon_style: Default::default(),
            current_nav_bar: None,
            current_nav_bar_style: Default::default(),
            symlink: ResultsSymlinkConfig::default(),
            selected_style: StyleSetting {
                modifier: Modifier::BOLD,
                ..Default::default()
            },
            selected_prefix_style: StyleSetting {
                fg: Some(Color::Cyan),
                modifier: Modifier::BOLD,
                ..Default::default()
            },
            yank_prefix_style: StyleSetting {
                fg: Some(Color::Yellow),
                modifier: Modifier::BOLD,
                ..Default::default()
            },
            cut_prefix_style: StyleSetting {
                fg: Some(Color::Red),
                modifier: Modifier::BOLD,
                ..Default::default()
            },
            group_header_style: StyleSetting {
                fg: Some(Color::Cyan),
                modifier: Modifier::BOLD,
                ..Default::default()
            },
            tier: ResultsTierConfig::default(),
            bookmark: ResultsBookmarkConfig::default(),
            frecency: ResultsFrecencyConfig::default(),
        }
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatusConfig {
    #[partial(recurse)]
    pub style: StyleSetting,

    /// Whether the status is visible.
    pub show: bool,
    /// Indent the status to match the results.
    pub match_indent: bool,

    /// Supports replacements:
    /// - `\r` -> cursor index
    /// - `\m` -> match count
    /// - `\t` -> total count
    /// - `\s` -> available whitespace / # appearances
    /// - `\S` -> Increment # appearances for `\s`
    ///
    /// For example: `r#"\m/\t"#.to_string()`
    #[partial(alias = "t")]
    pub template: String,

    /// - Full: available whitespace is computed using the full ui width when replacing `\s` in the template.
    /// - Disjoint: no effect.
    /// - Capped: no effect. (Since, unlike [`DisplayConfig`], status line can not display over the preview).
    pub row_connection: RowConnectionStyle,

    pub interactions: InteractionRegionSetting,
}
impl Default for StatusConfig {
    fn default() -> Self {
        Self {
            style: StyleSetting {
                fg: Some(Color::Green),
                modifier: Modifier::ITALIC,
                ..Default::default()
            },
            show: true,
            match_indent: true,
            template: String::new(),
            row_connection: RowConnectionStyle::Full,

            interactions: Default::default(),
        }
    }
}

impl StatusConfig {}

pub fn deserialize_horizontal_separator<'de, D>(
    deserializer: D,
) -> Result<HorizontalSeparator, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SeparatorHelper {
        Bool(bool),
        String(String),
    }

    match SeparatorHelper::deserialize(deserializer)? {
        SeparatorHelper::Bool(b) => Ok(if b {
            HorizontalSeparator::Light
        } else {
            HorizontalSeparator::None
        }),
        SeparatorHelper::String(s) => match s.to_lowercase().replace(['-', '_', ' '], "").as_str() {
            "none" | "false" | "" => Ok(HorizontalSeparator::None),
            "empty" => Ok(HorizontalSeparator::Empty),
            "light" | "normal" | "true" => Ok(HorizontalSeparator::Light),
            "heavy" | "thick" | "bold" => Ok(HorizontalSeparator::Heavy),
            "dashed" => Ok(HorizontalSeparator::Dashed),
            "top" | "upper" | "upperblock" => Ok(HorizontalSeparator::Top),
            "bottom" | "lower" | "lowerblock" => Ok(HorizontalSeparator::Bottom),
            "underline" | "underlined" => Ok(HorizontalSeparator::Underline),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "none",
                    "light",
                    "normal",
                    "heavy",
                    "dashed",
                    "top",
                    "bottom",
                    "underline",
                    "true",
                    "false",
                ],
            )),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct DisplayConfig {
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,

    #[partial(recurse)]
    pub style: StyleSetting,

    /// Indent content to match the results table.
    pub match_indent: bool,
    /// Enable line wrapping.
    pub wrap: bool,

    /// Static content to display.
    pub content: Option<StringOrVec>,

    /// This setting controls the effective width of the displayed content.
    /// - Full: Effective width is the full ui width.
    /// - Capped: Effective width is the full ui width, but
    ///   any width exceeding the width of the Results UI is occluded by the preview pane.
    /// - Disjoint: Same as capped. Additionally, the (bg) style is applied to individual
    /// columns instead of uniformly on the row.
    ///
    /// # Note
    /// The width effect only applies on the footer, and when the content is singular.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub row_connection: RowConnectionStyle,

    /// (cli only) This setting controls how many lines are read from the input for display with the header.
    /// Note: Incoming lines are partitioned into columns the same way regular lines are.
    #[partial(alias = "h")]
    pub header_lines: usize,

    pub interactions: Vec<InteractionRegionSetting>,

    /// Horizontal separator line drawn above the footer (e.g. separating list and preview from footer).
    /// Can be "light", "normal", "heavy", "dashed", "top", "bottom", "none", or a boolean (true = "light").
    #[serde(alias = "hr")]
    #[serde(alias = "divider")]
    #[serde(alias = "separator_line")]
    #[serde(deserialize_with = "deserialize_horizontal_separator", default)]
    pub separator: HorizontalSeparator,

    /// Custom style override for the footer horizontal separator line and junction.
    #[partial(recurse)]
    #[serde(alias = "separator_style")]
    #[serde(default)]
    pub separator_style: StyleSetting,
}

pub type InteractionRegionSetting = Vec<(u8, String)>;

impl Default for DisplayConfig {
    fn default() -> Self {
        DisplayConfig {
            border: Default::default(),
            match_indent: true,
            style: StyleSetting {
                fg: Some(Color::Cyan),
                ..Default::default()
            },
            wrap: false,
            row_connection: Default::default(),
            content: None,
            header_lines: 0,

            interactions: Default::default(),
            separator: HorizontalSeparator::None,
            separator_style: StyleSetting::default(),
        }
    }
}

/// # Example
/// ```rust
/// use waymaker::config::{PreviewConfig, PreviewSetting, PreviewLayout};
///
/// let _ = PreviewConfig {
///     layout: vec![
///         PreviewSetting {
///             layout: PreviewLayout::default(),
///             command: String::new(),
///             ..Default::default()
///         }
///     ],
///     ..Default::default()
/// };
/// ```
/// Native media preview settings.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewMediaConfig {
    /// Whether to enable native media previews using ratatui-image
    #[partial(alias = "m")]
    #[serde(alias = "media")]
    pub active: bool,
    /// Overriding graphics protocol for media previews (e.g. "kitty", "sixel", "halfblocks", "iterm2")
    #[serde(alias = "media_protocol")]
    pub protocol: Option<String>,
    /// Pixel resolution for media previews (images, videos, PDFs). Default: 1280. 0 = original
    #[serde(alias = "media_size")]
    pub size: Option<u32>,
    /// Initial zoom level for image previews. Default: 1.0
    pub zoom: Option<f32>,
    /// Resize/fit mode for media previews: "crop", "fit", or "scale" (default: "crop")
    #[serde(alias = "media_fit")]
    pub fit: Option<String>,
}

impl Default for PreviewMediaConfig {
    fn default() -> Self {
        Self {
            active: false,
            protocol: None,
            size: None,
            zoom: None,
            fit: None,
        }
    }
}

pub fn deserialize_preview_media<'de, D>(deserializer: D) -> Result<PreviewMediaConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MediaHelper {
        Bool(bool),
        Config(PreviewMediaConfig),
    }

    match MediaHelper::deserialize(deserializer)? {
        MediaHelper::Bool(b) => Ok(PreviewMediaConfig {
            active: b,
            ..PreviewMediaConfig::default()
        }),
        MediaHelper::Config(c) => Ok(c),
    }
}

/// Diagram rendering settings (Mermaid).
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewDiagramsConfig {
    /// Whether to render embedded Mermaid diagrams inside markdown files. Default: true
    #[partial(alias = "md_diag", alias = "diag")]
    #[serde(alias = "markdown_diagrams", alias = "diagrams", alias = "mermaid")]
    pub active: bool,
    /// Whether to render embedded Mermaid diagrams inline using Kitty Unicode Placeholders. Default: true
    #[partial(alias = "inline_diag", alias = "inline_diagram", alias = "inline_mermaid")]
    #[serde(alias = "inline_diagrams", alias = "inline_diag", alias = "inline_diagram", alias = "inline_mermaid", alias = "kitty_diagrams")]
    pub inline: bool,
    /// Mermaid diagram theme: "auto" (detects system/terminal theme), "dark", or "light". Default: "auto"
    #[partial(alias = "diagram_theme", alias = "diag_theme", alias = "theme_diag")]
    #[serde(alias = "diagram_theme", alias = "diagrams_theme", alias = "mermaid_theme", alias = "theme_diagram")]
    pub theme: DiagramTheme,
    /// Mermaid diagram background mode: "transparent" (inherits terminal background) or "solid". Default: "transparent"
    #[partial(alias = "diagram_background", alias = "diag_bg", alias = "diag_background")]
    #[serde(alias = "diagram_background", alias = "diagrams_background", alias = "mermaid_background", alias = "diagram_bg", alias = "diagrams_bg")]
    pub background: DiagramBackground,
}

impl Default for PreviewDiagramsConfig {
    fn default() -> Self {
        Self {
            active: true,
            inline: true,
            theme: DiagramTheme::default(),
            background: DiagramBackground::default(),
        }
    }
}

pub fn deserialize_preview_diagrams<'de, D>(deserializer: D) -> Result<PreviewDiagramsConfig, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DiagramsHelper {
        Bool(bool),
        Config(PreviewDiagramsConfig),
    }

    match DiagramsHelper::deserialize(deserializer)? {
        DiagramsHelper::Bool(b) => Ok(PreviewDiagramsConfig {
            active: b,
            ..PreviewDiagramsConfig::default()
        }),
        DiagramsHelper::Config(c) => Ok(c),
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewConfig {
    #[partial(recurse)]
    #[partial(alias = "b")]
    pub border: BorderSetting,
    #[partial(recurse, set = "recurse")]
    #[partial(alias = "l")]
    pub layout: Vec<PreviewSetting>,
    #[serde(alias = "scroll")]
    #[partial(recurse)]
    #[partial(alias = "i")]
    pub initial: PreviewInitialSetting,
    /// Whether to cycle to top after scrolling to the bottom and vice versa.
    #[partial(alias = "c")]
    #[serde(alias = "cycle")]
    pub scroll_wrap: bool,
    pub wrap: bool,
    /// Whether to show the preview pane initially.
    /// Can either be a boolean or a number which the relevant dimension of the available ui area must exceed.
    pub show: ShowCondition,

    pub reevaluate_show_on_resize: bool,

    /// Width of the drag area for resizing the preview pane.
    /// If `None`, it defaults to the width of the preview border.
    /// If `0`, drag resizing is disabled.
    pub drag_width: Option<u16>,
    /// Whether to show a scrollbar in the preview pane.
    #[partial(alias = "sb")]
    pub scrollbar: bool,

    /// Media preview configuration.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_preview_media", default)]
    pub media: PreviewMediaConfig,

    /// Whether to enable native markdown rendering with embedded Mermaid diagrams
    #[partial(alias = "md")]
    pub markdown: bool,

    /// Diagram rendering configuration.
    #[partial(recurse)]
    #[serde(deserialize_with = "deserialize_preview_diagrams", default)]
    pub diagrams: PreviewDiagramsConfig,

    /// Whether to render embedded local images inline using Kitty Unicode Placeholders. Default: true
    #[partial(alias = "inline_img", alias = "inline_image")]
    #[serde(alias = "inline_img", alias = "inline_image", alias = "kitty_images")]
    pub inline_images: bool,

    #[serde(default)]
    pub initial_layout: u8,
}

impl PreviewConfig {
    pub fn trim_commands(&mut self) {
        for setting in &mut self.layout {
            setting.command = setting.command.trim().to_string();
        }
    }
}

impl Default for PreviewConfig {
    fn default() -> Self {
        PreviewConfig {
            border: BorderSetting {
                padding: Padding(ratatui::widgets::Padding::left(2)),
                ..Default::default()
            },
            initial: Default::default(),
            initial_layout: 0,
            layout: Default::default(),
            scroll_wrap: false,
            wrap: false,
            show: Default::default(),
            reevaluate_show_on_resize: false,
            drag_width: None,
            scrollbar: false,
            media: PreviewMediaConfig::default(),
            markdown: true,
            diagrams: PreviewDiagramsConfig::default(),
            inline_images: true,
        }
    }
}

/// Determines the initial scroll offset of the preview window.
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewInitialSetting {
    /// Extract the initial display index `n` of the preview window from this column.
    /// `n` lines are skipped after the header lines are consumed.
    pub index: Option<StringValue>,
    /// For adjusting the initial scroll index.
    #[partial(alias = "o")]
    pub offset: isize,
    /// How far from the bottom of the preview window the scroll offset should appear.
    #[partial(alias = "p")]
    pub percentage: Percentage,
    /// Keep the top N lines as the fixed header so that they are always visible.
    #[partial(alias = "h")]
    pub header_lines: usize,

    #[partial(alias = "t")]
    pub tail: bool,

    #[serde(default)]
    pub hidden: bool,
}

impl Default for PreviewInitialSetting {
    fn default() -> Self {
        Self {
            index: Default::default(),
            offset: -1,
            percentage: Default::default(),
            header_lines: Default::default(),
            tail: false,
            hidden: false,
        }
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PreviewerConfig {
    pub try_lossy: bool,
    pub delay_clear: bool,

    // todo
    pub cache: u8,

    pub debounce_ms: u64,
    pub max_procs: usize,
    pub always_trigger: bool,

    pub help: HelpDisplayConfig,
    pub shell: Option<Vec<OsString>>,
    pub trim_commands: bool,
    pub hide_semantic_help: bool,

    /// See [`StartConfig`]
    pub command_args: Vec<OsString>,

    /// Pixel resolution for media previews (images, videos, PDFs). Default: 1280. 0 = original
    pub media_size: u32,

    /// Whether native terminal image/media previewing is enabled. Default: false
    pub media: bool,

    /// Whether embedded Mermaid diagrams are rendered in markdown files. Default: true
    pub markdown_diagrams: bool,

    /// Whether embedded Mermaid diagrams are rendered inline using Kitty Unicode Placeholders. Default: true
    pub inline_diagrams: bool,

    /// Whether embedded images are rendered inline using Kitty Unicode Placeholders. Default: true
    pub inline_images: bool,

    /// Theme for Mermaid diagrams: "auto", "dark", or "light". Default: "auto"
    pub diagram_theme: DiagramTheme,

    /// Background for Mermaid diagrams: "transparent" or "solid". Default: "transparent"
    pub diagram_background: DiagramBackground,
}

impl Default for PreviewerConfig {
    fn default() -> Self {
        Self {
            try_lossy: false,
            delay_clear: true,
            cache: 0,
            debounce_ms: 25,
            max_procs: 4,
            always_trigger: true,
            help: Default::default(),
            shell: None,
            trim_commands: false,
            hide_semantic_help: true,

            command_args: Default::default(),
            media_size: 1280,
            media: false,
            markdown_diagrams: true,
            inline_diagrams: true,
            inline_images: true,
            diagram_theme: DiagramTheme::default(),
            diagram_background: DiagramBackground::default(),
        }
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HelpDisplayConfig {
    pub colors: Option<HelpColorConfig>,
    pub hide_semantic: bool,
    pub seq_brackets: Option<[char; 2]>,
    pub quote_traces: bool,
    pub max_len: usize,
    pub ellipsize_center: bool,
}

impl Default for HelpDisplayConfig {
    fn default() -> Self {
        Self {
            colors: Some(Default::default()),
            hide_semantic: true,
            seq_brackets: Some(['[', ']']),
            quote_traces: true,
            max_len: 25,
            ellipsize_center: false,
        }
    }
}

/// Help coloring
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelpColorConfig {
    #[serde(deserialize_with = "camelcase_normalized")]
    pub section: Color,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub key: Color,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub value: Color,
}

impl Default for HelpColorConfig {
    fn default() -> Self {
        Self {
            section: Color::Blue,
            key: Color::Green,
            value: Color::White,
        }
    }
}

// ----------- SETTING TYPES -------------------------

#[derive(Default, Debug, Clone, PartialEq, Deserialize, Serialize)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[serde(default, deny_unknown_fields)]
pub struct BorderSetting {
    #[serde(deserialize_with = "camelcase_normalized_option")]
    pub r#type: Option<BorderType>,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub color: Color,
    /// Given as sides joined by `|`. i.e.:
    /// `sides = "TOP | BOTTOM"``
    /// `sides = "ALL"`
    /// When omitted, this either ALL or the side that sits between results and the corresponding layout if either padding or type are specified, otherwise NONE.
    ///
    /// An empty string enforces no sides:
    /// `sides = ""`
    // #[serde(deserialize_with = "uppercase_normalized_option")] // need ratatui bitflags to use transparent
    pub sides: Option<Borders>,
    /// Supply as either 1, 2, or 4 numbers for:
    ///
    /// - Same padding on all sides
    /// - Vertical and horizontal padding values
    /// - Top, Right, Bottom, Left padding values
    ///
    /// respectively.
    pub padding: Padding,
    pub title: String,
    // #[serde(deserialize_with = "transform_uppercase")]
    pub title_modifier: Modifier,
    pub modifier: Modifier,
    #[serde(deserialize_with = "camelcase_normalized")]
    pub bg: Color,
    /// Foreground color for the dynamic item title shown in the preview border.
    /// When `Color::Reset` (the default) the title inherits the border color.
    #[serde(deserialize_with = "camelcase_normalized")]
    pub title_fg: Color,
}

impl BorderSetting {
    pub fn as_block(&self) -> ratatui::widgets::Block<'_> {
        let mut ret = ratatui::widgets::Block::default()
            .padding(self.padding.0)
            .style(Style::default().bg(self.bg).add_modifier(self.modifier));

        if !self.title.is_empty() {
            let title = Span::styled(
                &self.title,
                Style::default().add_modifier(self.title_modifier),
            );

            ret = ret.title(title)
        };

        if !self.is_empty() {
            ret = ret
                .borders(self.sides())
                .border_type(self.r#type.unwrap_or_default())
                .border_style(ratatui::style::Style::default().fg(self.color))
        }

        ret
    }

    /// Like `as_block` but uses `title_override` (the dynamic item text) when
    /// provided, falling back to the static `self.title` when `None`.
    /// The title is styled with `title_fg` (or the border color when `title_fg`
    /// is `Color::Reset`).
    pub fn block_with_title<'a>(
        &'a self,
        title_override: Option<&'a str>,
    ) -> ratatui::widgets::Block<'a> {
        let mut ret = ratatui::widgets::Block::default()
            .padding(self.padding.0)
            .style(Style::default().bg(self.bg).add_modifier(self.modifier));

        let title_text: Option<&'a str> = title_override.or_else(|| {
            if self.title.is_empty() {
                None
            } else {
                Some(&self.title)
            }
        });

        if let Some(t) = title_text {
            let fg = if self.title_fg == Color::Reset {
                self.color
            } else {
                self.title_fg
            };
            let title = Span::styled(t, Style::default().fg(fg).add_modifier(self.title_modifier));
            ret = ret.title(title);
        }

        if !self.is_empty() {
            ret = ret
                .borders(self.sides())
                .border_type(self.r#type.unwrap_or_default())
                .border_style(ratatui::style::Style::default().fg(self.color))
        }

        ret
    }

    pub fn sides(&self) -> Borders {
        if let Some(s) = self.sides {
            s
        } else if self.r#type.is_some() {
            Borders::ALL
        } else {
            Borders::NONE
        }
    }

    pub fn as_static_block(&self) -> ratatui::widgets::Block<'static> {
        let mut ret = ratatui::widgets::Block::default()
            .padding(self.padding.0)
            .style(Style::default().bg(self.bg).add_modifier(self.modifier));

        if !self.title.is_empty() {
            let title: Span<'static> = Span::styled(
                self.title.clone(),
                Style::default().add_modifier(self.title_modifier),
            );

            ret = ret.title(title)
        };

        if !self.is_empty() {
            ret = ret
                .borders(self.sides())
                .border_type(self.r#type.unwrap_or_default())
                .border_style(ratatui::style::Style::default().fg(self.color))
        }

        ret
    }

    pub fn is_empty(&self) -> bool {
        self.sides() == Borders::NONE
    }

    pub fn height(&self) -> u16 {
        let mut height = 0;
        height += self.sides().contains(Borders::TOP) as u16
            + self.sides().contains(Borders::BOTTOM) as u16;
        height += self.padding.top + self.padding.bottom;
        height += (!self.title.is_empty() as u16).saturating_sub(!self.is_empty() as u16);

        height
    }

    pub fn width(&self) -> u16 {
        let mut width = 0;
        width += self.sides().contains(Borders::LEFT) as u16
            + self.sides().contains(Borders::RIGHT) as u16;

        width += self.padding.left + self.padding.right;

        width
    }

    pub fn left(&self) -> u16 {
        let mut width = 0;
        width += !self.is_empty() as u16;
        width += self.padding.left;

        width
    }

    pub fn top(&self) -> u16 {
        let mut height = 0;
        height += !self.is_empty() as u16;
        height += self.padding.top;
        height += (!self.title.is_empty() as u16).saturating_sub(!self.is_empty() as u16);

        height
    }
}

// how to determine how many rows to allocate?
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct TerminalLayoutSettings {
    /// Percentage of total rows to occupy.
    #[partial(alias = "p")]
    pub percentage: Percentage,
    pub min: u16,
    pub max: u16, // 0 for terminal height cap
}

impl Default for TerminalLayoutSettings {
    fn default() -> Self {
        Self {
            percentage: Percentage::new(50),
            min: 10,
            max: 120,
        }
    }
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewSetting {
    #[serde(flatten)]
    #[partial(recurse)]
    pub layout: PreviewLayout,
    #[partial(recurse)]
    pub border: Option<BorderSetting>,
    /// Optional preview title for this layout.
    ///
    /// - Omit for default dynamic title (current item name)
    /// - Set to `{item}` for explicit dynamic title
    /// - Set to any other string for a static title
    /// - `{item}` inside a longer string is interpolated
    pub title: Option<String>,
    #[serde(default, alias = "cmd", alias = "x")]
    pub command: String,

    #[cfg(feature = "partial")]
    #[partial(unwrap)]
    #[serde(alias = "scroll")]
    #[serde(default)]
    pub initial: PartialPreviewInitialSetting,
}

#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PreviewLayout {
    pub side: Side,
    /// Percentage of total rows/columns to occupy.
    #[serde(alias = "p")]
    // we need serde here since its specified inside the value but i don't think there's another case for it.
    pub percentage: Percentage,
    pub min: i16,
    pub max: i16,
    /// Width (or height) in cells of the draggable gap between the preview and
    /// the picker.  When `0` the gap is not rendered but the drag-zone still
    /// falls back to the border edge (existing behaviour).
    pub gap: u16,
    /// Row offset from the top of the gap at which the selection counter is
    /// drawn.  Defaults to `5`.
    #[partial(alias = "gco")]
    pub gap_counter_offset: u16,
}

impl Default for PreviewLayout {
    fn default() -> Self {
        Self {
            side: Side::Right,
            percentage: Percentage::new(60),
            min: 0,
            max: i16::MAX,
            gap: 1,
            gap_counter_offset: 0,
        }
    }
}

use crate::utils::serde::bounded_usize;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[partial(path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
pub struct ColumnsConfig {
    /// The strategy of how columns are parsed from input lines
    #[partial(alias = "s")]
    pub split: Split,
    /// Column names
    #[partial(alias = "n")]
    // #[partial(recurse, set = "recurse")] // partial application is better on the command line but we don't want it for overrides
    pub names: Vec<ColumnSetting>,
    /// Maximum number of columns to autogenerate when names is unspecified. Minimum of 1, maximum of 16.
    #[serde(deserialize_with = "bounded_usize::<_, 1, 16>")]
    #[serde(alias = "max")]
    max_columns: usize,
    #[partial(alias = "i")]
    pub default: Option<StringValue>,
    /// When autogenerating column names, start from 0 instead of 1.
    pub names_from_zero: bool,
}

impl ColumnsConfig {
    pub fn max_cols(&self) -> usize {
        self.max_columns.min(16).max(1)
    }
}

impl Default for ColumnsConfig {
    fn default() -> Self {
        Self {
            split: Default::default(),
            names: Default::default(),
            max_columns: 6,
            default: None,
            names_from_zero: false,
        }
    }
}

// ----------- Nucleo config helper
#[derive(Debug, Clone, PartialEq)]
pub struct NucleoMatcherConfig(pub nucleo::Config);

impl Default for NucleoMatcherConfig {
    fn default() -> Self {
        Self(nucleo::Config::DEFAULT)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
struct MatcherConfigHelper {
    pub normalize: Option<bool>,
    pub ignore_case: Option<bool>,
    pub prefer_prefix: Option<bool>,
    pub match_paths: bool,
}

impl serde::Serialize for NucleoMatcherConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let helper = MatcherConfigHelper {
            normalize: Some(self.0.normalize),
            ignore_case: Some(self.0.ignore_case),
            prefer_prefix: Some(self.0.prefer_prefix),
            match_paths: false,
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NucleoMatcherConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = MatcherConfigHelper::deserialize(deserializer)?;
        let mut config = nucleo::Config::DEFAULT;

        if helper.match_paths {
            config.set_match_paths();
        }

        if let Some(norm) = helper.normalize {
            config.normalize = norm;
        }
        if let Some(ic) = helper.ignore_case {
            config.ignore_case = ic;
        }
        if let Some(pp) = helper.prefer_prefix {
            config.prefer_prefix = pp;
        }

        Ok(NucleoMatcherConfig(config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_preview_config_trim_commands() {
        let mut config = PreviewConfig {
            layout: vec![
                PreviewSetting {
                    command: "  echo hello  ".to_string(),
                    ..Default::default()
                },
                PreviewSetting {
                    command: "\nls -la\n".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        config.trim_commands();

        assert_eq!(config.layout[0].command, "echo hello");
        assert_eq!(config.layout[1].command, "ls -la");
    }

    #[test]
    fn test_preview_config_diagram_options_toml() {
        let default_cfg: PreviewConfig = toml::from_str("").unwrap();
        assert_eq!(default_cfg.diagrams.theme, DiagramTheme::Auto);
        assert_eq!(default_cfg.diagrams.background, DiagramBackground::Transparent);

        let toml_str = r#"
            [diagrams]
            theme = "dark"
            background = "solid"
        "#;
        let config: PreviewConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.diagrams.theme, DiagramTheme::Dark);
        assert_eq!(config.diagrams.background, DiagramBackground::Solid);

        // Test aliases
        let toml_alias = r#"
            [diagrams]
            diagrams_theme = "light"
            diagram_bg = "transparent"
        "#;
        let config_alias: PreviewConfig = toml::from_str(toml_alias).unwrap();
        assert_eq!(config_alias.diagrams.theme, DiagramTheme::Light);
        assert_eq!(config_alias.diagrams.background, DiagramBackground::Transparent);
    }

    #[test]
    fn test_results_config_tier_separator_toml() {
        let toml_str = r#"
            [tier]
            separator = "dashed"
            [tier.style]
            fg = "Cyan"
        "#;
        let config: ResultsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tier.separator, HorizontalSeparator::Dashed);
        assert_eq!(
            config.tier.style.fg,
            Some(ratatui::style::Color::Cyan)
        );

        let toml_top = r#"
            [tier]
            separator = "top"
        "#;
        let config_top: ResultsConfig = toml::from_str(toml_top).unwrap();
        assert_eq!(config_top.tier.separator, HorizontalSeparator::Top);

        let toml_bottom = r#"
            [tier]
            separator = "bottom"
        "#;
        let config_bottom: ResultsConfig = toml::from_str(toml_bottom).unwrap();
        assert_eq!(config_bottom.tier.separator, HorizontalSeparator::Bottom);
    }

    #[test]
    fn test_folder_rule_matching() {
        let rule_downloads = FolderRule::new("~/Downloads", SortOrder::ModifiedReverse);
        if let Some(home) = dirs::home_dir() {
            let downloads = home.join("Downloads");
            assert!(rule_downloads.matches(&downloads));
            let other = home.join("Documents");
            assert!(!rule_downloads.matches(&other));
        }

        let rule_glob = FolderRule::new("*/screenshots", SortOrder::CreatedReverse);
        assert!(rule_glob.matches(Path::new("/home/user/pictures/screenshots")));
        assert!(rule_glob.matches(Path::new("/var/data/screenshots")));
        assert!(!rule_glob.matches(Path::new("/home/user/pictures/wallpapers")));

        let rule_basename = FolderRule::new("Screenshots", SortOrder::CreatedReverse);
        assert!(rule_basename.matches(Path::new("/home/user/Pictures/Screenshots")));
    }

    #[test]
    fn test_ui_config_resolve_sort_for_dir() {
        let mut ui = UiConfig::default();
        ui.default_sort = Some(SortOrder::Natural);
        ui.folder_rules = vec![
            FolderRule::new("~/Downloads", SortOrder::ModifiedReverse),
            FolderRule::new("*/videos", SortOrder::SizeReverse),
        ];

        if let Some(home) = dirs::home_dir() {
            assert_eq!(
                ui.resolve_sort_for_dir(&home.join("Downloads")),
                Some(SortOrder::ModifiedReverse)
            );
            assert_eq!(
                ui.resolve_sort_for_dir(&home.join("Projects")),
                Some(SortOrder::Natural)
            );
        }
        assert_eq!(
            ui.resolve_sort_for_dir(Path::new("/media/storage/videos")),
            Some(SortOrder::SizeReverse)
        );
    }

    #[test]
    fn test_nav_profile_deserialization_and_effective_profile() {
        // Default
        let default_nav: NavConfig = toml::from_str("").unwrap();
        assert_eq!(default_nav.profile, NavProfile::List);
        assert_eq!(default_nav.effective_profile(), NavProfile::List);

        // Explicit list
        let nav_list: NavConfig = toml::from_str(r#"profile = "list""#).unwrap();
        assert_eq!(nav_list.effective_profile(), NavProfile::List);

        // Alias picker
        let nav_picker: NavConfig = toml::from_str(r#"profile = "picker""#).unwrap();
        assert_eq!(nav_picker.effective_profile(), NavProfile::List);

        // Basic via profile
        let nav_basic: NavConfig = toml::from_str(r#"profile = "basic""#).unwrap();
        assert_eq!(nav_basic.effective_profile(), NavProfile::Basic);

        // Basic via legacy flag
        let nav_legacy_basic: NavConfig = toml::from_str(r#"basic = true"#).unwrap();
        assert_eq!(nav_legacy_basic.effective_profile(), NavProfile::Basic);

        // Fm profile
        let nav_fm: NavConfig = toml::from_str(r#"profile = "fm""#).unwrap();
        assert_eq!(nav_fm.effective_profile(), NavProfile::Fm);

        // Fm via fm flag
        let nav_fm_flag: NavConfig = toml::from_str(r#"fm = true"#).unwrap();
        assert_eq!(nav_fm_flag.effective_profile(), NavProfile::Fm);

        // None / strict profile
        let nav_none: NavConfig = toml::from_str(r#"profile = "none""#).unwrap();
        assert_eq!(nav_none.effective_profile(), NavProfile::None);

        let nav_strict: NavConfig = toml::from_str(r#"profile = "strict""#).unwrap();
        assert_eq!(nav_strict.effective_profile(), NavProfile::None);
    }

    #[test]
    fn test_default_binds_for_profile_isolation() {
        let list_binds = NavConfig::default_binds_for_profile(NavProfile::List);
        assert!(list_binds.contains_key("j"));
        assert!(list_binds.contains_key("k"));
        assert!(list_binds.contains_key(" "));
        assert!(list_binds.contains_key("/"));
        // Must NOT contain file-manager binds
        assert!(!list_binds.contains_key("h"));
        assert!(!list_binds.contains_key("l"));
        assert!(!list_binds.contains_key("f"));
        assert!(!list_binds.contains_key("b"));
        assert!(!list_binds.contains_key("*"));

        let basic_binds = NavConfig::default_binds_for_profile(NavProfile::Basic);
        assert!(basic_binds.contains_key("j"));
        assert!(basic_binds.contains_key("k"));
        assert!(basic_binds.contains_key("/"));
        // Basic does not have extra navigation
        assert!(!basic_binds.contains_key("gb"));
        assert!(!basic_binds.contains_key("gt"));
        assert!(!basic_binds.contains_key("h"));
        assert!(!basic_binds.contains_key("l"));

        let fm_binds = NavConfig::default_binds_for_profile(NavProfile::Fm);
        assert!(fm_binds.contains_key("j"));
        assert!(fm_binds.contains_key("k"));
        assert!(fm_binds.contains_key("h"));
        assert!(fm_binds.contains_key("l"));
        assert!(fm_binds.contains_key("f"));
        assert!(fm_binds.contains_key("b"));
        assert!(fm_binds.contains_key("*"));

        let none_binds = NavConfig::default_binds_for_profile(NavProfile::None);
        assert!(none_binds.is_empty());
    }

    #[test]
    fn test_display_config_separator_deserialization() {
        // String variant
        let toml_str = r#"
            separator = "light"
        "#;
        let cfg: DisplayConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.separator, HorizontalSeparator::Light);

        // Boolean variant (true)
        let toml_bool: DisplayConfig = toml::from_str("separator = true").unwrap();
        assert_eq!(toml_bool.separator, HorizontalSeparator::Light);

        // Boolean variant (false)
        let toml_false: DisplayConfig = toml::from_str("separator = false").unwrap();
        assert_eq!(toml_false.separator, HorizontalSeparator::None);

        // Alias hr
        let toml_hr: DisplayConfig = toml::from_str(r#"hr = "heavy""#).unwrap();
        assert_eq!(toml_hr.separator, HorizontalSeparator::Heavy);

        // Default
        let toml_def: DisplayConfig = toml::from_str("").unwrap();
        assert_eq!(toml_def.separator, HorizontalSeparator::None);
    }
}
