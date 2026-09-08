use serde::{Deserialize, Serialize};

use indexmap::IndexMap;
use matchmaker::SortOrder;
use matchmaker::action::Actions;
use matchmaker::binds::Trigger;
use matchmaker::config::*;
use matchmaker_partial_macros::partial;
use std::collections::HashMap;

use crate::action::MMAction;

#[derive(Clone, PartialEq, Serialize)]
#[partial(recurse, path, derive(Debug, Clone, PartialEq, Deserialize, Serialize))]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    // configure the ui
    #[partial(attr)]
    #[serde(default)]
    #[serde(flatten)]
    pub render: RenderConfig,

    #[serde(default)]
    #[serde(alias = "env")]
    #[partial(no_recurse, unwrap)]
    pub envs: HashMap<String, EnvValue>,

    // configure binds ( keypress/mouseevent/event => Actions )
    #[partial(attr)]
    #[serde(default)]
    #[partial(alias = "b")]
    #[partial(no_recurse, unwrap)]
    pub binds: IndexMap<Trigger, Actions<MMAction>>,

    // configure the tui
    #[partial(attr)]
    #[serde(default)]
    pub tui: TerminalConfig,

    // configure the preview command runner
    #[serde(default)]
    pub previewer: PreviewerConfig,

    // configure the matcher (columns + matching settings)
    #[partial(attr, alias = "m")]
    #[serde(default)]
    pub matcher: MatcherConfig,

    // configure startup settings (options for how input/output is processed)
    #[partial(attr, alias = "s")]
    #[serde(default)]
    pub start: StartConfig,

    #[partial(attr, alias = "c")]
    #[serde(default)]
    /// How columns are parsed from input lines
    pub columns: ColumnsConfig,

    // configure exit conditions
    #[partial(attr, alias = "e")]
    #[serde(default)]
    pub exit: ExitConfig,

    #[partial(no_recurse)]
    #[serde(default)]
    #[serde(alias = "rules")]
    pub rule: Vec<PathRule>,

    /// Folder-specific rules (e.g. automatic sort order by directory).
    #[serde(default)]
    #[serde(alias = "folder_rules")]
    #[serde(alias = "folder_rule")]
    #[partial(no_recurse, unwrap)]
    pub folder_rules: Vec<FolderRule>,

    /// Global default sort order.
    #[serde(default)]
    #[partial(no_recurse)]
    pub default_sort: Option<SortOrder>,

    /// imports: only supported on overrides and with one nesting level
    #[serde(default)]
    #[partial(no_recurse)]
    pub source: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PathMatchPattern {
    Single(String),
    Multiple(Vec<String>),
}

impl PathMatchPattern {
    pub fn matches(&self, current_dir: &std::path::Path) -> bool {
        let cwd_str = current_dir.to_string_lossy();
        let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string());

        let check_single = |pattern: &str| -> bool {
            let mut expanded = pattern.trim().to_string();
            if let Some(ref h) = home {
                if expanded == "$HOME" || expanded == "~" {
                    expanded = h.clone();
                } else if expanded.starts_with("~/") {
                    expanded = format!("{}/{}", h, &expanded[2..]);
                } else if expanded.starts_with("$HOME/") {
                    expanded = format!("{}/{}", h, &expanded[6..]);
                }
            }

            let exp_path = std::path::Path::new(&expanded);
            if current_dir == exp_path {
                return true;
            }

            if expanded.contains('*') || expanded.contains('?') || expanded.contains('[') {
                if let Ok(glob_pattern) = glob::Pattern::new(&expanded) {
                    if glob_pattern.matches(&cwd_str) {
                        return true;
                    }
                }
            }
            false
        };

        match self {
            PathMatchPattern::Single(p) => check_single(p),
            PathMatchPattern::Multiple(list) => list.iter().any(|p| check_single(p)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathRule {
    #[serde(alias = "paths")]
    pub path: PathMatchPattern,

    pub preset: Option<std::path::PathBuf>,

    #[serde(flatten)]
    pub override_config: PartialConfig,
}

// -----------------------

#[cfg(not(windows))]
pub const DEFAULT_CONFIG: &str = include_str!("../assets/config.toml");
#[cfg(windows)]
pub const DEFAULT_CONFIG: &str = include_str!("../assets/win.config.toml");

impl Default for Config {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let default_toml = include_str!("../assets/dev.toml");
        let config: Config = toml::from_str(default_toml).expect("failed to parse default TOML");
        let serialized = toml::to_string_pretty(&config).expect("failed to serialize to TOML");
        let deserialized: Config = toml::from_str(&serialized)
            .unwrap_or_else(|e| panic!("failed to parse serialized TOML:\n{}\n{e}", serialized));

        // Assert the round-trip produces the same data
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_path_rule_deserialization_and_matching() {
        let toml_str = r#"
            [[rule]]
            path = ["$HOME", "~"]
            preset = "jump"

            [[rule]]
            path = "~/dev/**"
            matcher.sort = "smart"
            matcher.frecency = true
        "#;

        let partial: PartialConfig =
            toml::from_str(toml_str).expect("failed to parse path rules TOML");
        let rules = partial.rule.expect("rules should be present");
        assert_eq!(rules.len(), 2);

        let home_dir = dirs::home_dir().expect("home dir");
        assert!(rules[0].path.matches(&home_dir));
        assert_eq!(rules[0].preset, Some(std::path::PathBuf::from("jump")));

        let dev_subdir = home_dir.join("dev").join("github").join("project");
        assert!(!rules[0].path.matches(&dev_subdir));
        assert!(rules[1].path.matches(&dev_subdir));
    }

    #[test]
    fn test_awt_type_deserialization() {
        use matchmaker_partial::Apply;
        let toml_str =
            std::fs::read_to_string("/home/fecavmi/.config/matchmaker/presets/awt-type.toml")
                .unwrap();
        let partial: PartialConfig = toml::from_str(&toml_str).unwrap();
        let mut config = Config::default();
        config.apply(partial);

        println!("Config worker: {:#?}", config.matcher.worker);
        assert_eq!(
            config.matcher.worker.sort_threshold,
            matchmaker::config::SortThreshold::SMART
        );
    }

    #[test]
    fn test_jump_preset_deserialization() {
        let toml_str = include_str!("../assets/presets/jump.toml");
        let res: Result<PartialConfig, _> = toml::from_str(toml_str);
        if let Err(e) = &res {
            eprintln!("=== TOML ERROR ON ASSET JUMP ===\n{}\n================", e);
            panic!("Error parsing asset jump.toml: {e}");
        }

        let dotfiles_str =
            std::fs::read_to_string("/home/fecavmi/.dotfiles/main/matchmaker/.config/matchmaker/presets/jump.toml")
                .unwrap();
        let res_dotfiles: Result<PartialConfig, _> = toml::from_str(&dotfiles_str);
        if let Err(e) = &res_dotfiles {
            eprintln!("=== TOML ERROR ON DOTFILES JUMP ===\n{}\n================", e);
            panic!("Error parsing dotfiles jump.toml: {e}");
        }
    }
}
