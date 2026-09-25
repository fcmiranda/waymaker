#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MatchResult {
    pub score: u32,
    pub index: u32,
}

/// Abstract fuzzy matcher engine interface.
pub trait MatcherEngine: Send + Sync {
    /// Friendly name of the engine backend.
    fn name(&self) -> &'static str;

    /// Perform a search on a slice of items using a single thread.
    fn search(&mut self, query: &str, items: &[String]) -> Vec<MatchResult>;

    /// Perform a search in parallel across multiple threads.
    fn search_parallel(&mut self, query: &str, items: &[String], threads: usize) -> Vec<MatchResult>;

    /// Extract matching character offsets for highlight rendering in TUI.
    fn highlight_indices(&mut self, query: &str, haystack: &str) -> Vec<u32>;
}

/// Pure algorithm engine powered by Helix/Nucleo matcher.
pub struct NucleoEngine {
    matcher: nucleo::Matcher,
    case_matching: nucleo::pattern::CaseMatching,
    normalization: nucleo::pattern::Normalization,
}

impl Default for NucleoEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl NucleoEngine {
    pub fn new() -> Self {
        Self {
            matcher: nucleo::Matcher::new(nucleo::Config::DEFAULT),
            case_matching: nucleo::pattern::CaseMatching::Smart,
            normalization: nucleo::pattern::Normalization::Smart,
        }
    }
}

impl MatcherEngine for NucleoEngine {
    fn name(&self) -> &'static str {
        "nucleo"
    }

    fn search(&mut self, query: &str, items: &[String]) -> Vec<MatchResult> {
        if query.is_empty() {
            return items
                .iter()
                .enumerate()
                .map(|(i, _)| MatchResult {
                    score: 0,
                    index: i as u32,
                })
                .collect();
        }

        let pattern = nucleo::pattern::Pattern::new(
            query,
            self.case_matching,
            self.normalization,
            nucleo::pattern::AtomKind::Fuzzy,
        );

        let mut buf = Vec::new();
        let mut results = Vec::with_capacity(items.len());

        for (i, item) in items.iter().enumerate() {
            buf.clear();
            let utf32 = nucleo::Utf32Str::new(item, &mut buf);
            if let Some(score) = pattern.score(utf32, &mut self.matcher) {
                results.push(MatchResult {
                    score: score as u32,
                    index: i as u32,
                });
            }
        }

        results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));
        results
    }

    fn search_parallel(&mut self, query: &str, items: &[String], _threads: usize) -> Vec<MatchResult> {
        // Fallback to sequential for single Nucleo Matcher (or use nucleo worker in full TUI)
        self.search(query, items)
    }

    fn highlight_indices(&mut self, query: &str, haystack: &str) -> Vec<u32> {
        if query.is_empty() || haystack.is_empty() {
            return Vec::new();
        }

        let pattern = nucleo::pattern::Pattern::new(
            query,
            self.case_matching,
            self.normalization,
            nucleo::pattern::AtomKind::Fuzzy,
        );

        let mut buf = Vec::new();
        let utf32 = nucleo::Utf32Str::new(haystack, &mut buf);
        let mut indices = Vec::new();
        pattern.indices(utf32, &mut self.matcher, &mut indices);
        indices.sort_unstable();
        indices.dedup();
        indices
    }
}

#[cfg(feature = "frizbee")]
pub struct FrizbeeEngine {
    config: frizbee::Config,
}

#[cfg(feature = "frizbee")]
impl Default for FrizbeeEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "frizbee")]
impl FrizbeeEngine {
    pub fn new() -> Self {
        Self {
            config: frizbee::Config::default(),
        }
    }

    pub fn with_typo_tolerance(mut self, enabled: bool) -> Self {
        self.config.max_typos = if enabled { Some(1) } else { Some(0) };
        self
    }

    pub fn set_typo_tolerance(&mut self, enabled: bool) {
        self.config.max_typos = if enabled { Some(1) } else { Some(0) };
    }
}

#[cfg(feature = "frizbee")]
impl MatcherEngine for FrizbeeEngine {
    fn name(&self) -> &'static str {
        "frizbee"
    }

    fn search(&mut self, query: &str, items: &[String]) -> Vec<MatchResult> {
        if query.is_empty() {
            return items
                .iter()
                .enumerate()
                .map(|(i, _)| MatchResult {
                    score: 0,
                    index: i as u32,
                })
                .collect();
        }

        let mut matcher = frizbee::Matcher::new(query, &self.config);
        let matches = matcher.match_list(items);

        matches
            .into_iter()
            .map(|m| MatchResult {
                score: m.score as u32,
                index: m.index,
            })
            .collect()
    }

    fn search_parallel(&mut self, query: &str, items: &[String], threads: usize) -> Vec<MatchResult> {
        if query.is_empty() {
            return items
                .iter()
                .enumerate()
                .map(|(i, _)| MatchResult {
                    score: 0,
                    index: i as u32,
                })
                .collect();
        }

        let mut matcher = frizbee::Matcher::new(query, &self.config);
        let matches = matcher.match_list_parallel(items, threads);

        matches
            .into_iter()
            .map(|m| MatchResult {
                score: m.score as u32,
                index: m.index,
            })
            .collect()
    }

    fn highlight_indices(&mut self, query: &str, haystack: &str) -> Vec<u32> {
        if query.is_empty() || haystack.is_empty() {
            return Vec::new();
        }

        let mut matcher = frizbee::Matcher::new(query, &self.config);
        if let Some(m) = matcher.match_one_indices(haystack, 0) {
            let mut indices = m.indices;
            indices.reverse();
            indices
        } else {
            Vec::new()
        }
    }
}

/// Selector enum to pick the active backend dynamically.
pub enum MatcherBackend {
    Nucleo(NucleoEngine),
    #[cfg(feature = "frizbee")]
    Frizbee(FrizbeeEngine),
}

impl MatcherBackend {
    pub fn nucleo() -> Self {
        Self::Nucleo(NucleoEngine::new())
    }

    #[cfg(feature = "frizbee")]
    pub fn frizbee() -> Self {
        Self::Frizbee(FrizbeeEngine::new())
    }

    #[cfg(feature = "frizbee")]
    pub fn frizbee_with_typo(typo_tolerance: bool) -> Self {
        Self::Frizbee(FrizbeeEngine::new().with_typo_tolerance(typo_tolerance))
    }
}

impl MatcherEngine for MatcherBackend {
    fn name(&self) -> &'static str {
        match self {
            Self::Nucleo(e) => e.name(),
            #[cfg(feature = "frizbee")]
            Self::Frizbee(e) => e.name(),
        }
    }

    fn search(&mut self, query: &str, items: &[String]) -> Vec<MatchResult> {
        match self {
            Self::Nucleo(e) => e.search(query, items),
            #[cfg(feature = "frizbee")]
            Self::Frizbee(e) => e.search(query, items),
        }
    }

    fn search_parallel(&mut self, query: &str, items: &[String], threads: usize) -> Vec<MatchResult> {
        match self {
            Self::Nucleo(e) => e.search_parallel(query, items, threads),
            #[cfg(feature = "frizbee")]
            Self::Frizbee(e) => e.search_parallel(query, items, threads),
        }
    }

    fn highlight_indices(&mut self, query: &str, haystack: &str) -> Vec<u32> {
        match self {
            Self::Nucleo(e) => e.highlight_indices(query, haystack),
            #[cfg(feature = "frizbee")]
            Self::Frizbee(e) => e.highlight_indices(query, haystack),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nucleo_engine_basic() {
        let mut engine = NucleoEngine::new();
        let items = vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "README.md".to_string(),
        ];
        let res = engine.search("lib", &items);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].index, 1);
    }

    #[cfg(feature = "frizbee")]
    #[test]
    fn test_frizbee_engine_basic() {
        let mut engine = FrizbeeEngine::new();
        let items = vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "README.md".to_string(),
        ];
        let res = engine.search("lib", &items);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].index, 1);
    }

    #[cfg(feature = "frizbee")]
    #[test]
    fn test_frizbee_typo_tolerance() {
        let mut engine = FrizbeeEngine::new().with_typo_tolerance(true);
        let items = vec![
            "config.toml".to_string(),
            "cargo.lock".to_string(),
        ];
        // Typo: "cnfig" missing 'o'
        let res = engine.search("cnfig", &items);
        assert!(!res.is_empty(), "Frizbee should match with typo tolerance");
        assert_eq!(res[0].index, 0);
    }

    #[cfg(feature = "frizbee")]
    #[test]
    fn test_frizbee_highlights() {
        let mut engine = FrizbeeEngine::new();
        let indices = engine.highlight_indices("main", "src/main.rs");
        assert_eq!(indices, vec![4, 5, 6, 7]);
    }

    #[test]
    fn test_nucleo_engine_edge_cases() {
        let mut engine = NucleoEngine::default();
        assert_eq!(engine.name(), "nucleo");

        let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let empty_search = engine.search("", &items);
        assert_eq!(empty_search.len(), 3);
        assert_eq!(empty_search[0].score, 0);

        let parallel_res = engine.search_parallel("bet", &items, 4);
        assert_eq!(parallel_res.len(), 1);
        assert_eq!(parallel_res[0].index, 1);

        assert!(engine.highlight_indices("", "sample").is_empty());
        assert!(engine.highlight_indices("test", "").is_empty());
    }

    #[cfg(feature = "frizbee")]
    #[test]
    fn test_frizbee_engine_edge_cases() {
        let mut engine = FrizbeeEngine::default();
        assert_eq!(engine.name(), "frizbee");

        let items = vec!["alpha".into(), "beta".into(), "gamma".into()];
        let empty_search = engine.search("", &items);
        assert_eq!(empty_search.len(), 3);

        let empty_parallel = engine.search_parallel("", &items, 2);
        assert_eq!(empty_parallel.len(), 3);

        let parallel_res = engine.search_parallel("bet", &items, 2);
        assert_eq!(parallel_res.len(), 1);
        assert_eq!(parallel_res[0].index, 1);

        assert!(engine.highlight_indices("", "sample").is_empty());
        assert!(engine.highlight_indices("test", "").is_empty());

        engine.set_typo_tolerance(false);
        assert!(engine.search("alpz", &items).is_empty());
        engine.set_typo_tolerance(true);
        assert!(!engine.search("alpz", &items).is_empty());
    }

    #[test]
    fn test_matcher_backend_dispatch() {
        let mut backend = MatcherBackend::nucleo();
        assert_eq!(backend.name(), "nucleo");
        let items = vec!["foo.rs".into(), "bar.rs".into()];
        let res = backend.search("foo", &items);
        assert_eq!(res.len(), 1);
        let res_par = backend.search_parallel("bar", &items, 2);
        assert_eq!(res_par.len(), 1);
        let hl = backend.highlight_indices("bar", "bar.rs");
        assert_eq!(hl, vec![0, 1, 2]);

        #[cfg(feature = "frizbee")]
        {
            let mut friz = MatcherBackend::frizbee();
            assert_eq!(friz.name(), "frizbee");
            assert_eq!(friz.search("foo", &items).len(), 1);
            assert_eq!(friz.search_parallel("bar", &items, 2).len(), 1);
            assert_eq!(friz.highlight_indices("bar", "bar.rs"), vec![0, 1, 2]);

            let friz_typo = MatcherBackend::frizbee_with_typo(true);
            assert_eq!(friz_typo.name(), "frizbee");
        }
    }
}
