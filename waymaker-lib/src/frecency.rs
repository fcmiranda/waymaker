use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use redb::{Database, ReadableTable, TableDefinition};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

pub const FRECENCY_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("frecency_v2");
pub const PINS_TABLE: TableDefinition<&str, u64> = TableDefinition::new("pins_v1");

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

#[derive(Debug, Clone)]
struct ShadowFileGuard {
    temp_path: PathBuf,
}

impl Drop for ShadowFileGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp_path);
    }
}

static FRECENCY_DB_INSTANCES: std::sync::Mutex<
    Option<rustc_hash::FxHashMap<PathBuf, Weak<Database>>>,
> = std::sync::Mutex::new(None);

/// Main Frecency Store wrapping `redb::Database` with thread safety and resilient fallback.
#[derive(Clone)]
pub struct FrecencyStore {
    db: Option<Arc<Database>>,
    pub db_path: Option<PathBuf>,
    _shadow_guard: Option<Arc<ShadowFileGuard>>,
}

impl std::fmt::Debug for FrecencyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrecencyStore")
            .field("db_path", &self.db_path)
            .field("active", &self.db.is_some())
            .field("is_shadow", &self._shadow_guard.is_some())
            .finish()
    }
}

impl FrecencyStore {
    /// Default state directory for waymaker frecency DB:
    /// `~/.local/state/waymaker/frecency.redb` (with fallback to `matchmaker/frecency.redb` if existing)
    pub fn default_db_path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("WM_FRECENCY_DB").or_else(|_| std::env::var("MM_FRECENCY_DB")) {
            return Some(PathBuf::from(p));
        }
        let base = dirs::state_dir().or_else(dirs::data_local_dir)?;
        let wm_path = base.join("waymaker").join("frecency.redb");
        if wm_path.exists() {
            return Some(wm_path);
        }
        let legacy_path = base.join("matchmaker").join("frecency.redb");
        if legacy_path.exists() {
            return Some(legacy_path);
        }
        Some(wm_path)
    }

    /// Opens or creates the frecency database at default location with resilient error handling.
    pub fn open() -> Self {
        if let Some(path) = Self::default_db_path() {
            Self::open_at(&path).unwrap_or_else(|err| {
                log::warn!("Failed to open frecency store at {path:?}: {err}. Falling back to in-memory mode.");
                Self {
                    db: None,
                    db_path: Some(path),
                    _shadow_guard: None,
                }
            })
        } else {
            Self {
                db: None,
                db_path: None,
                _shadow_guard: None,
            }
        }
    }

    /// Returns true if this store is operating on a read-only shadow copy due to lock contention.
    pub fn is_shadow(&self) -> bool {
        self._shadow_guard.is_some()
    }

    fn get_write_db(&self) -> Option<Arc<Database>> {
        if self._shadow_guard.is_none() {
            return self.db.clone();
        }
        // If we are currently holding a shadow copy (because another process holds flock),
        // attempt to open the real database now for writing.
        if let Some(path) = &self.db_path {
            if let Ok(d) = Database::create(path) {
                return Some(Arc::new(d));
            }
        }
        None
    }

    /// Opens or creates the frecency database at a specific path.
    pub fn open_at(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let canonical_path = path.canonicalize().unwrap_or_else(|_| {
            if let Some(parent) = path.parent() {
                let p = parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf());
                if let Some(name) = path.file_name() {
                    p.join(name)
                } else {
                    path.to_path_buf()
                }
            } else {
                path.to_path_buf()
            }
        });

        // 1. Check if another thread in this process already has an active Database open
        {
            let mut lock = FRECENCY_DB_INSTANCES.lock().unwrap();
            let map = lock.get_or_insert_with(rustc_hash::FxHashMap::default);
            if let Some(weak) = map.get(&canonical_path) {
                if let Some(existing) = weak.upgrade() {
                    return Ok(Self {
                        db: Some(existing),
                        db_path: Some(canonical_path),
                        _shadow_guard: None,
                    });
                }
            }
        }

        // 2. Attempt to open primary database with retry on lock contention
        let mut db = match Database::create(path) {
            Ok(database) => Some(Arc::new(database)),
            Err(err) => {
                log::warn!("redb error opening {path:?}: {err}.");
                match &err {
                    redb::DatabaseError::DatabaseAlreadyOpen => {
                        let mut retried = None;
                        for _ in 0..10 {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                            if let Ok(d) = Database::create(path) {
                                retried = Some(Arc::new(d));
                                break;
                            }
                        }
                        retried
                    }
                    _ => {
                        if path.exists() {
                            let backup_path =
                                path.with_extension(format!("corrupt.{}.bak", current_unix_secs()));
                            let _ = fs::rename(path, &backup_path);
                            Database::create(path).ok().map(Arc::new)
                        } else {
                            None
                        }
                    }
                }
            }
        };

        let mut shadow_guard = None;

        // 3. Fallback: If primary database is locked by another process, create an ephemeral shadow copy for reading
        if db.is_none() && canonical_path.exists() {
            let temp_file = std::env::temp_dir().join(format!(
                "mm_frecency_shadow_{}_{}.redb",
                std::process::id(),
                current_unix_nanos()
            ));
            if fs::copy(&canonical_path, &temp_file).is_ok() {
                if let Ok(shadow_db) = Database::create(&temp_file) {
                    db = Some(Arc::new(shadow_db));
                    shadow_guard = Some(Arc::new(ShadowFileGuard {
                        temp_path: temp_file,
                    }));
                } else {
                    let _ = fs::remove_file(&temp_file);
                }
            }
        }

        // 4. If primary DB opened, cache it as a Weak reference
        if let Some(ref d) = db {
            if shadow_guard.is_none() {
                let actual_canonical = path
                    .canonicalize()
                    .unwrap_or_else(|_| canonical_path.clone());
                let mut lock = FRECENCY_DB_INSTANCES.lock().unwrap();
                let map = lock.get_or_insert_with(rustc_hash::FxHashMap::default);
                map.insert(actual_canonical, Arc::downgrade(d));
                map.insert(canonical_path.clone(), Arc::downgrade(d));
            }
        }

        Ok(Self {
            db,
            db_path: Some(canonical_path),
            _shadow_guard: shadow_guard,
        })
    }

    /// Record access event for a file or directory path. Returns updated score.
    pub fn add(&self, raw_path: &str) -> anyhow::Result<u32> {
        let Some(db) = self.get_write_db() else {
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
        let Some(db) = self.get_write_db() else {
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

    /// Check if the frecency table has 0 entries.
    pub fn is_empty(&self) -> bool {
        let Some(db) = self.db.as_ref() else {
            return true;
        };
        if let Ok(read_txn) = db.begin_read() {
            if let Ok(table) = read_txn.open_table(FRECENCY_TABLE) {
                if let Ok(mut iter) = table.iter() {
                    return iter.next().is_none();
                }
            }
        }
        true
    }

    /// Automatically import directory history from zoxide if the frecency database is currently empty.
    pub fn auto_import_from_zoxide_if_empty(&self) {
        if self.is_empty() {
            let _ = self.import_from_zoxide();
        }
    }

    /// Import directory rankings from zoxide CLI (`zoxide query -l`) or `~/.local/share/zoxide/db.zo`.
    pub fn import_from_zoxide(&self) -> usize {
        let mut count = 0;
        // 1. Try running `zoxide query -l`
        if let Ok(output) = std::process::Command::new("zoxide")
            .arg("query")
            .arg("-l")
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = text.lines().collect();
                let total = lines.len();
                for (i, line) in lines.iter().enumerate() {
                    let path = line.trim();
                    if !path.is_empty() && Path::new(path).is_dir() {
                        let weight = (total.saturating_sub(i) as u64).clamp(1, 20);
                        if self.import_entry(path, weight).is_ok() {
                            count += 1;
                        }
                    }
                }
                if count > 0 {
                    return count;
                }
            }
        }

        // 2. Fallback: Parse ~/.local/share/zoxide/db.zo directly
        if let Some(data_dir) = dirs::data_dir().or_else(dirs::data_local_dir) {
            let db_zo = data_dir.join("zoxide").join("db.zo");
            if db_zo.exists() {
                if let Ok(content) = std::fs::read_to_string(&db_zo) {
                    for line in content.lines() {
                        if let Some((path, _)) = line.split_once('|') {
                            let path = path.trim();
                            if !path.is_empty() && Path::new(path).is_dir() {
                                if self.import_entry(path, 5).is_ok() {
                                    count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }

        count
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

        let Some(write_db) = self.get_write_db() else {
            return Ok(0);
        };

        let write_txn = write_db.begin_write()?;
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
        let Some(db) = self.get_write_db() else {
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

    /// Pin / bookmark a path.
    pub fn pin(&self, raw_path: &str) -> anyhow::Result<()> {
        let Some(db) = self.get_write_db() else {
            return Ok(());
        };

        let key_str = normalize_path(raw_path);
        if key_str.is_empty() {
            return Ok(());
        }
        let now = current_unix_secs();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(PINS_TABLE)?;
            table.insert(key_str.as_str(), now)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Unpin / remove bookmark for a path. Returns true if key was present.
    pub fn unpin(&self, raw_path: &str) -> anyhow::Result<bool> {
        let Some(db) = self.get_write_db() else {
            return Ok(false);
        };

        let key_str = normalize_path(raw_path);
        if key_str.is_empty() {
            return Ok(false);
        }
        let clean = clean_path(raw_path);
        let write_txn = db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(PINS_TABLE)?;
            let r1 = table.remove(key_str.as_str())?.is_some();
            let r2 = if clean != key_str.as_str() {
                table.remove(clean)?.is_some()
            } else {
                false
            };
            r1 || r2
        };
        write_txn.commit()?;
        Ok(removed)
    }

    /// Toggle pin / bookmark for a path. Returns `true` if now pinned, `false` if unpinned.
    pub fn toggle_pin(&self, raw_path: &str) -> anyhow::Result<bool> {
        let key_str = normalize_path(raw_path);
        if key_str.is_empty() {
            return Ok(false);
        }

        if self.is_pinned(&key_str) {
            self.unpin(&key_str)?;
            Ok(false)
        } else {
            self.pin(&key_str)?;
            Ok(true)
        }
    }

    /// Check if a path is pinned / bookmarked.
    pub fn is_pinned(&self, raw_path: &str) -> bool {
        let Some(db) = self.db.as_ref() else {
            return false;
        };

        let key_str = normalize_path(raw_path);
        if key_str.is_empty() {
            return false;
        }

        let read_txn = match db.begin_read() {
            Ok(t) => t,
            Err(_) => return false,
        };

        let table = match read_txn.open_table(PINS_TABLE) {
            Ok(t) => t,
            Err(_) => return false,
        };

        match table.get(key_str.as_str()) {
            Ok(Some(_)) => true,
            _ => {
                let clean = clean_path(raw_path);
                if clean != key_str.as_str() {
                    table.get(clean).map(|g| g.is_some()).unwrap_or(false)
                } else {
                    false
                }
            }
        }
    }

    /// Return all pinned paths.
    pub fn list_pins(&self) -> Vec<String> {
        let Some(db) = self.db.as_ref() else {
            return Vec::new();
        };

        let read_txn = match db.begin_read() {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        let table = match read_txn.open_table(PINS_TABLE) {
            Ok(t) => t,
            Err(_) => return Vec::new(),
        };

        let mut pins: Vec<(String, u64)> = Vec::new();
        if let Ok(iter) = table.iter() {
            for item in iter.flatten() {
                let path = item.0.value().to_string();
                let timestamp = item.1.value();
                pins.push((path, timestamp));
            }
        }

        // Sort by timestamp ascending
        pins.sort_by_key(|p| p.1);
        pins.into_iter().map(|p| p.0).collect()
    }

    /// Get all pinned paths as a `std::collections::HashSet<String>`.
    pub fn get_pins_set(&self) -> std::collections::HashSet<String> {
        self.list_pins().into_iter().collect()
    }
}

fn current_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn current_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
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

    #[test]
    fn test_pins_crud_and_toggle() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_pins_crud");
        let _ = fs::remove_dir_all(&temp_dir);
        let db_path = temp_dir.join("test_pins.redb");

        let store = FrecencyStore::open_at(&db_path)?;
        let path1 = "/home/user/projects/alpha";
        let path2 = "/home/user/projects/beta";

        assert!(!store.is_pinned(path1));
        assert!(store.list_pins().is_empty());

        // Pin path1
        store.pin(path1)?;
        assert!(store.is_pinned(path1));
        assert_eq!(store.list_pins().len(), 1);

        // Pin path2
        store.pin(path2)?;
        assert!(store.is_pinned(path2));
        assert_eq!(store.list_pins().len(), 2);

        // Toggle path1 (should unpin)
        let pinned = store.toggle_pin(path1)?;
        assert!(!pinned);
        assert!(!store.is_pinned(path1));
        assert_eq!(store.list_pins().len(), 1);

        // Toggle path1 again (should pin)
        let pinned = store.toggle_pin(path1)?;
        assert!(pinned);
        assert!(store.is_pinned(path1));
        assert_eq!(store.list_pins().len(), 2);

        // Unpin path2
        let removed = store.unpin(path2)?;
        assert!(removed);
        assert!(!store.is_pinned(path2));
        assert_eq!(store.list_pins().len(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_shadow_copy_fallback_on_locked_database() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_shadow_fallback");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir)?;
        let db_path = temp_dir.join("primary.redb");

        let test_file = temp_dir.join("locked_item.rs");
        fs::write(&test_file, "")?;
        let test_path = test_file.to_str().unwrap();

        // Open primary database and populate with test data
        let primary_store = FrecencyStore::open_at(&db_path)?;
        primary_store.add(test_path)?;
        primary_store.pin(test_path)?;

        let temp_shadow =
            std::env::temp_dir().join(format!("test_shadow_{}.redb", current_unix_nanos()));
        fs::copy(&db_path, &temp_shadow)?;
        let shadow_db = Database::create(&temp_shadow)?;
        let shadow_store = FrecencyStore {
            db: Some(Arc::new(shadow_db)),
            db_path: Some(db_path.clone()),
            _shadow_guard: Some(Arc::new(ShadowFileGuard {
                temp_path: temp_shadow.clone(),
            })),
        };

        assert!(shadow_store.is_shadow());
        assert!(shadow_store.is_pinned(test_path));
        assert!(shadow_store.get_bonus(test_path) > 0);
        assert_eq!(shadow_store.list_pins(), vec![test_path.to_string()]);

        drop(shadow_store);
        assert!(
            !temp_shadow.exists(),
            "Shadow temp file must be cleaned up on drop"
        );

        drop(primary_store);
        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }
}
