use serde::{Deserialize, Serialize};

use indexmap::IndexMap;
use waymaker::SortOrder;
use waymaker::action::Actions;
use waymaker::binds::Trigger;
use waymaker::config::*;
use waymaker_partial_macros::partial;
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
    #[partial(attr, alias = "m", alias = "worker")]
    #[serde(default, alias = "worker")]
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

    // configure native directory walker
    #[partial(attr, alias = "w")]
    #[serde(default)]
    pub walker: WalkerConfig,

    /// imports: only supported on overrides and with one nesting level
    #[serde(default)]
    #[partial(no_recurse)]
    pub source: Option<std::path::PathBuf>,
}

impl Config {
    pub fn resolve(&mut self) {
        if let Some(ref sw) = self.start.walker {
            self.walker.merge(sw);
        }
    }
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
            matcher.sort.threshold = "smart"
            matcher.frecency.active = true
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
        use waymaker_partial::Apply;
        let p = std::path::Path::new("/home/fecavmi/.config/matchmaker/presets/awt-type.toml");
        if !p.exists() {
            return;
        }
        let toml_str = std::fs::read_to_string(p).unwrap();
        let partial: PartialConfig = toml::from_str(&toml_str).unwrap();
        let mut config = Config::default();
        config.apply(partial);

        println!("Config worker: {:#?}", config.matcher.worker);
        assert_eq!(
            config.matcher.worker.sort.threshold,
            waymaker::config::SortThreshold::SMART
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

        let dotfiles_path = std::path::Path::new(
            "/home/fecavmi/.dotfiles/main/matchmaker/.config/matchmaker/presets/jump.toml",
        );
        if dotfiles_path.exists() {
            let dotfiles_str = std::fs::read_to_string(dotfiles_path).unwrap();
            let res_dotfiles: Result<PartialConfig, _> = toml::from_str(&dotfiles_str);
            if let Err(e) = &res_dotfiles {
                eprintln!(
                    "=== TOML ERROR ON DOTFILES JUMP ===\n{}\n================",
                    e
                );
                panic!("Error parsing dotfiles jump.toml: {e}");
            }
        }
    }

    #[test]
    fn test_worker_and_matcher_table_equivalence() {
        use waymaker_partial::Apply;

        let toml_matcher = r#"
            [matcher.sort]
            threshold = "smart"
            cap = 500

            [matcher.frecency]
            active = true
            weight = 3
        "#;
        let p_matcher: PartialConfig = toml::from_str(toml_matcher).unwrap();
        let mut cfg_matcher = Config::default();
        cfg_matcher.apply(p_matcher);

        let toml_worker = r#"
            [worker.sort]
            threshold = "smart"
            cap = 500

            [worker.frecency]
            active = true
            weight = 3
        "#;
        let p_worker: PartialConfig = toml::from_str(toml_worker).unwrap();
        let mut cfg_worker = Config::default();
        cfg_worker.apply(p_worker);

        assert_eq!(
            cfg_matcher.matcher.worker.sort.threshold,
            cfg_worker.matcher.worker.sort.threshold
        );
        assert_eq!(
            cfg_matcher.matcher.worker.sort.cap,
            cfg_worker.matcher.worker.sort.cap
        );
        assert_eq!(
            cfg_matcher.matcher.worker.frecency.active,
            cfg_worker.matcher.worker.frecency.active
        );
        assert_eq!(
            cfg_matcher.matcher.worker.frecency.weight,
            cfg_worker.matcher.worker.frecency.weight
        );
    }

    #[test]
    fn test_all_asset_presets_deserialization() {
        let presets_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/presets");
        let mut failures = Vec::new();
        fn check_dir(dir: &std::path::Path, failures: &mut Vec<(std::path::PathBuf, String)>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    check_dir(&path, failures);
                } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                    let content = std::fs::read_to_string(&path).unwrap();
                    let res: Result<PartialConfig, _> = toml::from_str(&content);
                    if let Err(e) = res {
                        failures.push((path, e.to_string()));
                    }
                }
            }
        }
        check_dir(&presets_dir, &mut failures);
        if !failures.is_empty() {
            eprintln!("Failed to deserialize {} presets:", failures.len());
            for (p, err) in &failures {
                eprintln!("  {:?}: {}", p, err);
            }
            panic!("{} presets failed deserialization", failures.len());
        }
    }

    #[test]
    fn test_jump_preset_layering() {
        use waymaker::action::Action;
        use waymaker::binds::BindMapExt;
        use waymaker_partial::Apply;

        let jump_str = include_str!("../assets/presets/jump.toml");
        let jump_partial: PartialConfig = toml::from_str(jump_str).unwrap();

        let mut config = Config::default();
        if let Ok(user_str) =
            std::fs::read_to_string("/home/fecavmi/.config/matchmaker/config.toml")
        {
            if let Ok(user_partial) = toml::from_str::<PartialConfig>(&user_str) {
                config.apply(user_partial);
            }
        }
        config.apply(jump_partial);

        // Check that 'tab' is present in nav_binds and binds
        assert!(
            config.render.ui.nav.binds.contains_key("tab"),
            "nav_binds should have 'tab'"
        );
        let tab_trigger: waymaker::binds::Trigger = "tab".parse().unwrap();
        assert!(
            config.binds.contains_key(&tab_trigger),
            "binds should have 'tab'"
        );

        let sem_trigger: waymaker::binds::Trigger = "@reloadnext".parse().unwrap();
        config
            .binds
            .entry(sem_trigger)
            .or_insert(waymaker::acs![Action::Custom(
                crate::action::MMAction::ReloadNext(None)
            )]);
        config.binds.resolve_semantics();

        let tab_bind_after = &config.binds[&tab_trigger];
        // After resolution, the actions should contain ReloadNext
        let has_reloadnext = tab_bind_after
            .iter()
            .any(|a| matches!(a, Action::Custom(crate::action::MMAction::ReloadNext(None))));
        assert!(has_reloadnext, "tab binding should resolve to ReloadNext");
    }

    #[test]
    fn test_walker_and_start_walker_table_equivalence() {
        use waymaker_partial::Apply;

        let toml_walker = r#"
            [walker]
            ignore = [".git", ".cache", "custom"]
            hidden = false
        "#;
        let p_walker: PartialConfig = toml::from_str(toml_walker).unwrap();
        let mut cfg_walker = Config::default();
        cfg_walker.apply(p_walker);
        cfg_walker.resolve();

        let toml_start_walker = r#"
            [start.walker]
            ignore = [".git", ".cache", "custom"]
            hidden = false
        "#;
        let p_start_walker: PartialConfig = toml::from_str(toml_start_walker).unwrap();
        let mut cfg_start_walker = Config::default();
        cfg_start_walker.apply(p_start_walker);
        cfg_start_walker.resolve();

        assert_eq!(cfg_walker.walker.ignore, cfg_start_walker.walker.ignore);
        assert_eq!(cfg_walker.walker.hidden, cfg_start_walker.walker.hidden);
        assert_eq!(
            cfg_walker.walker.effective_ignore(),
            cfg_start_walker.walker.effective_ignore()
        );
    }

    #[test]
    fn test_walker_native_defaults_and_override() {
        use waymaker_partial::Apply;

        // Base config has native ignored directories by default
        let cfg = Config::default();
        let eff = cfg.walker.effective_ignore();
        assert!(eff.contains(&".git".to_string()));
        assert!(eff.contains(&".cache".to_string()));
        assert!(eff.contains(&".local".to_string()));
        assert!(eff.contains(&".cargo".to_string()));
        assert!(eff.contains(&".rustup".to_string()));
        assert!(eff.contains(&".npm".to_string()));
        assert!(eff.contains(&"node_modules".to_string()));
        assert!(eff.contains(&"target".to_string()));
        assert!(eff.contains(&".venv".to_string()));

        // Override via TOML
        let toml_override = r#"
            [walker]
            extra_ignore = ["my_cache", "build/"]
        "#;
        let partial: PartialConfig = toml::from_str(toml_override).unwrap();
        let mut cfg2 = Config::default();
        cfg2.apply(partial);
        cfg2.resolve();

        let eff2 = cfg2.walker.effective_ignore();
        assert!(eff2.contains(&"my_cache".to_string()));
        assert!(eff2.contains(&"build".to_string()));
        assert!(eff2.contains(&".cache".to_string()));
    }
}
