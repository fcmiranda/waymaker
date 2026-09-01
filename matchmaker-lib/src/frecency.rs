use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{Database, ReadableTable, TableDefinition};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

pub const FRECENCY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("frecency_v2");

#[inline]
fn decode_record(bytes: &[u8]) -> Option<FrecencyRecord> {
    if let Ok(record) = postcard::from_bytes::<FrecencyRecord>(bytes) {
        Some(record)
    } else if let Ok(json_str) = std::str::from_utf8(bytes) {
        serde_json::from_str::<FrecencyRecord>(json_str).ok()
    } else {
        None
    }
}

/// Access record for a given path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FrecencyRecord {
    pub path: String,
    pub count: u64,
    pub last_accessed: u64,   // Unix timestamp in seconds
    pub timestamps: Vec<u64>, // Recent access timestamps
}

impl FrecencyRecord {
    pub fn new(path: String) -> Self {
        Self {
            path,
            count: 0,
            last_accessed: 0,
            timestamps: Vec::new(),
        }
    }

    pub fn record_access(&mut self, now: u64) {
        self.count += 1;
        self.last_accessed = now;
        self.timestamps.push(now);

        // Keep at most 50 recent timestamps
        if self.timestamps.len() > 50 {
            let cutoff = now.saturating_sub(7_776_000); // 90 days
            self.timestamps.retain(|&ts| ts >= cutoff);
            if self.timestamps.len() > 50 {
                let start_idx = self.timestamps.len() - 50;
                self.timestamps.drain(0..start_idx);
            }
        }
    }

    /// Calculate Frecency Score based on continuous exponential half-life decay (default 7 days).
    pub fn calculate_score(&self, now: u64) -> u32 {
        self.calculate_score_with_half_life(now, 7)
    }

    /// Calculate Frecency Score based on continuous exponential half-life decay.
    /// If half_life_days == 0, falls back to legacy discrete time buckets (<1h, <1d, <1w, <1mo, >1mo).
    pub fn calculate_score_with_half_life(&self, now: u64, half_life_days: u32) -> u32 {
        if half_life_days == 0 {
            let mut score: u32 = 0;
            for &ts in &self.timestamps {
                let age = now.saturating_sub(ts);
                let weight = if age < 3600 {
                    100
                } else if age < 86400 {
                    80
                } else if age < 604800 {
                    40
                } else if age < 2592000 {
                    20
                } else {
                    10
                };
                score = score.saturating_add(weight);
            }
            score
        } else {
            let half_life_secs = (half_life_days as f64) * 86_400.0;
            let mut total_score: f64 = 0.0;
            for &ts in &self.timestamps {
                let age = now.saturating_sub(ts) as f64;
                let decay = (-age / half_life_secs).exp2();
                total_score += 100.0 * decay;
            }
            total_score.round() as u32
        }
    }
}

/// In-memory snapshot for zero-allocation fast lookup during active fuzzy search.
#[derive(Debug, Clone, Default)]
pub struct FrecencySnapshot {
    pub scores: FxHashMap<String, u32>,
    pub cwd: String,
    pub home: String,
}

impl FrecencySnapshot {
    #[inline]
    pub fn get_bonus(&self, path: &str) -> u32 {
        self.get_bonus_with_bias(path, 0)
    }

    #[inline]
    pub fn get_bonus_with_bias(&self, path: &str, location_bias: u32) -> u32 {
        if self.scores.is_empty() {
            return 0;
        }

        let clean = clean_path(path);
        // 1. Direct exact match in scores table
        if let Some(&score) = self.scores.get(clean) {
            let is_cwd_child =
                !self.cwd.is_empty() && (clean.starts_with(&self.cwd) || !clean.starts_with('/'));
            if is_cwd_child && location_bias > 0 {
                return score.saturating_add((score as u64 * location_bias as u64 / 100) as u32);
            }
            return score;
        }

        let mut buf = [0u8; 1024];
        // 2. Expand ~/ with self.home and check exact path
        if (clean.starts_with("~/") || clean.starts_with("~\\")) && !self.home.is_empty() {
            let rest = &clean[2..];
            let needed = self.home.len() + 1 + rest.len();
            if needed <= buf.len() {
                buf[..self.home.len()].copy_from_slice(self.home.as_bytes());
                buf[self.home.len()] = b'/';
                buf[self.home.len() + 1..needed].copy_from_slice(rest.as_bytes());
                if let Ok(full_str) = std::str::from_utf8(&buf[..needed]) {
                    if let Some(&score) = self.scores.get(clean_path(full_str)) {
                        let is_cwd_child = !self.cwd.is_empty() && full_str.starts_with(&self.cwd);
                        if is_cwd_child && location_bias > 0 {
                            return score.saturating_add(
                                (score as u64 * location_bias as u64 / 100) as u32,
                            );
                        }
                        return score;
                    }
                }
            }
        // 3. Resolve relative path against self.cwd and check exact path
        } else if !clean.starts_with('/') && !clean.starts_with('\\') && !self.cwd.is_empty() {
            let needed = self.cwd.len() + 1 + clean.len();
            if needed <= buf.len() {
                buf[..self.cwd.len()].copy_from_slice(self.cwd.as_bytes());
                buf[self.cwd.len()] = b'/';
                buf[self.cwd.len() + 1..needed].copy_from_slice(clean.as_bytes());
                if let Ok(full_str) = std::str::from_utf8(&buf[..needed]) {
                    if let Some(&score) = self.scores.get(clean_path(full_str)) {
                        if location_bias > 0 {
                            return score.saturating_add(
                                (score as u64 * location_bias as u64 / 100) as u32,
                            );
                        }
                        return score;
                    }
                }
            }
        }

        0
    }

    /// Fast zero-allocation check if path has any frecency bonus.
    #[inline]
    pub fn has_bonus_fast(&self, path: &str) -> bool {
        if self.scores.is_empty() {
            return false;
        }
        let trimmed = path.trim_end_matches('/').trim_end_matches('\\');
        if self.scores.contains_key(trimmed) {
            return true;
        }
        let mut buf = [0u8; 1024];
        if !trimmed.starts_with('/') && !trimmed.starts_with('\\') && !self.cwd.is_empty() {
            let needed = self.cwd.len() + 1 + trimmed.len();
            if needed <= buf.len() {
                buf[..self.cwd.len()].copy_from_slice(self.cwd.as_bytes());
                buf[self.cwd.len()] = b'/';
                buf[self.cwd.len() + 1..needed].copy_from_slice(trimmed.as_bytes());
                if let Ok(full_str) = std::str::from_utf8(&buf[..needed]) {
                    return self.scores.contains_key(full_str);
                }
            }
        }
        false
    }
}

/// Helper function to normalize path strings (expand tilde, convert relative paths to absolute, trim trailing slashes).
pub fn normalize_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let p = PathBuf::from(trimmed);
    let abs = if p.is_absolute() {
        p
    } else if trimmed == "~" || trimmed.starts_with("~/") || trimmed.starts_with("~\\") {
        if let Some(home) = dirs::home_dir() {
            if trimmed == "~" {
                home
            } else {
                home.join(&trimmed[2..])
            }
        } else {
            p
        }
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(&p)
    } else {
        p
    };

    let resolved = abs.canonicalize().unwrap_or(abs);
    let mut s = resolved.to_string_lossy().to_string();
    if s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        s.pop();
    }
    s
}

pub fn clean_path(path: &str) -> &str {
    let p = path.trim();
    if p.len() > 1 {
        p.strip_suffix('/')
            .or_else(|| p.strip_suffix('\\'))
            .unwrap_or(p)
    } else {
        p
    }
}

/// Main Frecency Store wrapping `redb::Database` with thread safety and resilient fallback.
#[derive(Clone)]
pub struct FrecencyStore {
    db: Arc<Option<Database>>,
    pub db_path: Option<PathBuf>,
}

impl std::fmt::Debug for FrecencyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrecencyStore")
            .field("db_path", &self.db_path)
            .field("active", &self.db.is_some())
            .finish()
    }
}

impl FrecencyStore {
    /// Default state directory for matchmaker frecency DB:
    /// `~/.local/state/matchmaker/frecency.redb`
    pub fn default_db_path() -> Option<PathBuf> {
        let dir = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .map(|d| d.join("matchmaker"))?;
        Some(dir.join("frecency.redb"))
    }

    /// Opens or creates the frecency database at default location with resilient error handling.
    pub fn open() -> Self {
        if let Some(path) = Self::default_db_path() {
            Self::open_at(&path).unwrap_or_else(|err| {
                log::warn!("Failed to open frecency store at {path:?}: {err}. Falling back to in-memory mode.");
                Self {
                    db: Arc::new(None),
                    db_path: Some(path),
                }
            })
        } else {
            Self {
                db: Arc::new(None),
                db_path: None,
            }
        }
    }

    /// Opens or creates the frecency database at a specific path.
    pub fn open_at(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let db_res = Database::create(path);
        let db = match db_res {
            Ok(database) => Some(database),
            Err(err) => {
                log::error!("redb error opening {path:?}: {err}. Attempting recovery...");
                // If database corrupt, attempt backup and recreate clean
                let backup_path =
                    path.with_extension(format!("corrupt.{}.bak", current_unix_secs()));
                let _ = fs::rename(path, &backup_path);
                Database::create(path).ok()
            }
        };

        Ok(Self {
            db: Arc::new(db),
            db_path: Some(path.to_path_buf()),
        })
    }

    /// Record access event for a file or directory path. Returns updated score.
    pub fn add(&self, raw_path: &str) -> anyhow::Result<u32> {
        let Some(db) = self.db.as_ref() else {
            return Ok(0);
        };

        let key_str = normalize_path(raw_path);
        if key_str.is_empty() || !Path::new(&key_str).exists() {
            return Ok(0);
        }
        let key = key_str.as_str();
        let now = current_unix_secs();

        let write_txn = db.begin_write()?;
        let score = {
            let mut table = write_txn.open_table(FRECENCY_TABLE)?;
            let mut record = if let Some(guard) = table.get(key)? {
                decode_record(guard.value()).unwrap_or_else(|| FrecencyRecord::new(key.to_string()))
            } else {
                FrecencyRecord::new(key.to_string())
            };

            record.record_access(now);
            let updated_score = record.calculate_score(now);
            let bytes = postcard::to_allocvec(&record)?;
            table.insert(key, bytes.as_slice())?;
            updated_score
        };

        write_txn.commit()?;
        Ok(score)
    }

    /// Query current calculated frecency score for a path.
    pub fn get_bonus(&self, raw_path: &str) -> u32 {
        let Some(db) = self.db.as_ref() else {
            return 0;
        };

        let key_str = normalize_path(raw_path);
        let key = key_str.as_str();
        let now = current_unix_secs();

        let read_txn = match db.begin_read() {
            Ok(t) => t,
            Err(_) => return 0,
        };

        let table = match read_txn.open_table(FRECENCY_TABLE) {
            Ok(t) => t,
            Err(_) => return 0,
        };

        match table.get(key) {
            Ok(Some(guard)) => {
                if let Some(record) = decode_record(guard.value()) {
                    record.calculate_score(now)
                } else {
                    0
                }
            }
            _ => {
                let clean = clean_path(raw_path);
                if clean != key {
                    if let Ok(Some(guard)) = table.get(clean) {
                        if let Some(record) = decode_record(guard.value()) {
                            return record.calculate_score(now);
                        }
                    }
                }
                0
            }
        }
    }

    /// Retrieve full FrecencyRecord details for a path.
    pub fn rank(&self, raw_path: &str) -> Option<FrecencyRecord> {
        let Some(db) = self.db.as_ref() else {
            return None;
        };

        let key_str = normalize_path(raw_path);
        let key = key_str.as_str();
        let read_txn = db.begin_read().ok()?;
        let table = read_txn.open_table(FRECENCY_TABLE).ok()?;
        if let Some(guard) = table.get(key).ok()? {
            decode_record(guard.value())
        } else {
            let clean = clean_path(raw_path);
            let guard = table.get(clean).ok()??;
            decode_record(guard.value())
        }
    }

    /// Load all tracked entries into an in-memory snapshot for sub-millisecond lookup (default 7 days half-life).
    pub fn get_snapshot(&self) -> FrecencySnapshot {
        self.get_snapshot_with_half_life(7)
    }

    /// Load all tracked entries into an in-memory snapshot with a configurable decay half-life in days.
    pub fn get_snapshot_with_half_life(&self, half_life_days: u32) -> FrecencySnapshot {
        let mut snapshot = FrecencySnapshot {
            scores: FxHashMap::default(),
            cwd: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
            home: dirs::home_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        };
        let Some(db) = self.db.as_ref() else {
            return snapshot;
        };

        let now = current_unix_secs();
        if let Ok(read_txn) = db.begin_read() {
            if let Ok(table) = read_txn.open_table(FRECENCY_TABLE) {
                if let Ok(iter) = table.iter() {
                    for entry in iter.flatten() {
                        let key = entry.0.value();
                        let bytes = entry.1.value();
                        if let Some(record) = decode_record(bytes) {
                            let score = record.calculate_score_with_half_life(now, half_life_days);
                            if score > 0 {
                                snapshot.scores.insert(key.to_string(), score);
                            }
                        }
                    }
                }
            }
        }

        snapshot
    }

    /// Retrieve all records stored in database.
    pub fn all_records(&self) -> Vec<FrecencyRecord> {
        let Some(db) = self.db.as_ref() else {
            return Vec::new();
        };

        let mut records = Vec::new();
        if let Ok(read_txn) = db.begin_read() {
            if let Ok(table) = read_txn.open_table(FRECENCY_TABLE) {
                if let Ok(iter) = table.iter() {
                    for entry in iter.flatten() {
                        if let Some(record) = decode_record(entry.1.value()) {
                            records.push(record);
                        }
                    }
                }
            }
        }
        records
    }

    /// Import an entry with a specified access count/weight into the database.
    pub fn import_entry(&self, raw_path: &str, count: u64) -> anyhow::Result<()> {
        let Some(db) = self.db.as_ref() else {
            return Ok(());
        };

        let key_str = normalize_path(raw_path);
        if key_str.is_empty() {
            return Ok(());
        }
        let key = key_str.as_str();
        let now = current_unix_secs();

        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(FRECENCY_TABLE)?;
            let mut record = if let Some(guard) = table.get(key)? {
                decode_record(guard.value()).unwrap_or_else(|| FrecencyRecord::new(key.to_string()))
            } else {
                FrecencyRecord::new(key.to_string())
            };

            let iterations = count.clamp(1, 50);
            for _ in 0..iterations {
                record.record_access(now);
            }

            let bytes = postcard::to_allocvec(&record)?;
            table.insert(key, bytes.as_slice())?;
        }

        write_txn.commit()?;
        Ok(())
    }

    /// Purges all entries whose file/directory path is not absolute or no longer exists on disk.
    pub fn clean_stale(&self) -> anyhow::Result<usize> {
        let Some(db) = self.db.as_ref() else {
            return Ok(0);
        };

        let mut stale_keys = Vec::new();
        if let Ok(read_txn) = db.begin_read() {
            if let Ok(table) = read_txn.open_table(FRECENCY_TABLE) {
                if let Ok(iter) = table.iter() {
                    for entry in iter.flatten() {
                        let key = entry.0.value();
                        let p = Path::new(key);
                        if !p.is_absolute() || !p.exists() {
                            stale_keys.push(key.to_string());
                        }
                    }
                }
            }
        }

        if stale_keys.is_empty() {
            return Ok(0);
        }

        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(FRECENCY_TABLE)?;
            for key in &stale_keys {
                let _ = table.remove(key.as_str())?;
            }
        }
        write_txn.commit()?;
        Ok(stale_keys.len())
    }

    /// Removes a specific path entry from the frecency database. Returns true if key was present.
    pub fn remove(&self, raw_path: &str) -> anyhow::Result<bool> {
        let Some(db) = self.db.as_ref() else {
            return Ok(false);
        };

        let key_str = normalize_path(raw_path);
        let key = key_str.as_str();
        let clean = clean_path(raw_path);

        let write_txn = db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(FRECENCY_TABLE)?;
            let r1 = table.remove(key)?.is_some();
            let r2 = if clean != key {
                table.remove(clean)?.is_some()
            } else {
                false
            };
            r1 || r2
        };

        write_txn.commit()?;
        Ok(removed)
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_path() {
        assert_eq!(clean_path("/home/user/project/"), "/home/user/project");
        assert_eq!(clean_path("src/lib.rs"), "src/lib.rs");
        assert_eq!(clean_path("/"), "/");
    }

    #[test]
    fn test_frecency_score_decay() {
        let now = 10_000_000;
        let mut rec = FrecencyRecord::new("foo.txt".into());
        rec.record_access(now);
        assert_eq!(rec.calculate_score(now), 100);

        // 7 days ago (604800s) -> decayed by exactly 50% = 50 pts (total: 150)
        rec.timestamps.push(now - 604_800);
        assert_eq!(rec.calculate_score(now), 150);

        // 14 days ago -> decayed by 75% = 25 pts (total: 175)
        rec.timestamps.push(now - 2 * 604_800);
        assert_eq!(rec.calculate_score(now), 175);
    }

    #[test]
    fn test_store_open_add_rank() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_frecency");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir)?;
        let test_file = temp_dir.join("file.rs");
        fs::write(&test_file, "")?;
        let db_path = temp_dir.join("test.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let path = test_file.to_str().unwrap();

        let score1 = store.add(path)?;
        assert!(score1 >= 100);

        let rank_res = store.rank(path);
        assert!(rank_res.is_some());
        let record = rank_res.unwrap();
        assert_eq!(record.count, 1);
        assert_eq!(record.path, path);

        let score2 = store.add(path)?;
        assert!(score2 > score1);

        let snapshot = store.get_snapshot();
        assert_eq!(snapshot.get_bonus(path), score2);

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_store_import_and_clean_stale() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_frecency_clean");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir)?;
        let db_path = temp_dir.join("test.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let existing_path = temp_dir.to_str().unwrap();
        let non_existing_path = "/non/existent/path/for/mm/test.rs";

        store.import_entry(existing_path, 3)?;
        store.import_entry(non_existing_path, 5)?;

        assert!(store.get_bonus(existing_path) > 0);
        assert!(store.get_bonus(non_existing_path) > 0);

        let cleaned = store.clean_stale()?;
        assert_eq!(cleaned, 1);

        assert!(store.get_bonus(existing_path) > 0);
        assert_eq!(store.get_bonus(non_existing_path), 0);

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_store_remove() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_frecency_remove");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir)?;
        let test_file = temp_dir.join("remove_target");
        fs::write(&test_file, "")?;
        let db_path = temp_dir.join("test.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let path = test_file.to_str().unwrap();

        store.add(path)?;
        assert!(store.get_bonus(path) > 0);

        let removed = store.remove(path)?;
        assert!(removed);
        assert_eq!(store.get_bonus(path), 0);

        let removed_again = store.remove(path)?;
        assert!(!removed_again);

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_snapshot_relative_path_lookup() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_frecency_relative");
        let _ = fs::remove_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let abs_path = temp_dir
            .join(".agents")
            .join("skills")
            .join("skill-creator")
            .join("scripts")
            .join("run_eval.py");
        fs::create_dir_all(abs_path.parent().unwrap())?;
        fs::write(&abs_path, "")?;
        let abs_str = abs_path.to_str().unwrap();

        store.add(abs_str)?;
        let mut snapshot = store.get_snapshot();
        snapshot.cwd = temp_dir.to_str().unwrap().to_string();

        // Exact match
        assert!(snapshot.get_bonus(abs_str) > 0);

        // Relative path resolved against cwd
        let rel_path = ".agents/skills/skill-creator/scripts/run_eval.py";
        assert!(snapshot.get_bonus(rel_path) > 0);

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_exact_path_outranks_generic_basename() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_frecency_ranking");
        let _ = fs::remove_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let accessed_path = temp_dir
            .join("github")
            .join("matchmaker")
            .join("fecavmi")
            .join(".agents")
            .join("skills")
            .join("skill-creator")
            .join("scripts")
            .join("run_eval.py");
        fs::create_dir_all(accessed_path.parent().unwrap())?;
        fs::write(&accessed_path, "")?;

        let unaccessed_path = temp_dir
            .join("github")
            .join("acpd")
            .join(".agents")
            .join("skills")
            .join("skill-creator")
            .join("scripts")
            .join("run_eval.py");
        fs::create_dir_all(unaccessed_path.parent().unwrap())?;
        fs::write(&unaccessed_path, "")?;

        store.add(accessed_path.to_str().unwrap())?;
        let mut snapshot = store.get_snapshot();
        snapshot.cwd = temp_dir.to_str().unwrap().to_string();

        let rel_accessed =
            "github/matchmaker/fecavmi/.agents/skills/skill-creator/scripts/run_eval.py";
        let rel_unaccessed = "github/acpd/.agents/skills/skill-creator/scripts/run_eval.py";

        let accessed_bonus = snapshot.get_bonus(rel_accessed);
        let unaccessed_bonus = snapshot.get_bonus(rel_unaccessed);

        assert!(
            accessed_bonus > 0,
            "Accessed path should have a positive bonus"
        );
        assert_eq!(
            unaccessed_bonus, 0,
            "Unaccessed path must have 0 bonus (no basename pollution)"
        );

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_location_bias_boost() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_location_bias");
        let _ = fs::remove_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let local_file = temp_dir.join("local_file.rs");
        fs::create_dir_all(&temp_dir)?;
        fs::write(&local_file, "")?;

        store.add(local_file.to_str().unwrap())?;
        let mut snapshot = store.get_snapshot();
        snapshot.cwd = temp_dir.to_str().unwrap().to_string();

        let base_bonus = snapshot.get_bonus_with_bias("local_file.rs", 0);
        let biased_bonus = snapshot.get_bonus_with_bias("local_file.rs", 30);

        assert!(base_bonus > 0);
        assert_eq!(
            biased_bonus,
            base_bonus + (base_bonus * 30 / 100),
            "Location bias +30% should apply to CWD local paths"
        );

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_legacy_discrete_half_life_0() {
        let now = 1_000_000;
        let mut rec = FrecencyRecord::new("legacy.txt".into());
        rec.record_access(now);
        // Discrete bucket < 1h -> weight 100
        assert_eq!(rec.calculate_score_with_half_life(now, 0), 100);

        // 2 hours ago -> weight 80 in legacy mode
        rec.timestamps.push(now - 7200);
        assert_eq!(rec.calculate_score_with_half_life(now, 0), 180);
    }
}
