use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};

pub const DIR_CACHE_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("dir_cache_v2");
pub const ZERO_COPY_MAGIC: u32 = 0x4D4D5A43; // 'MMZC'

/// Cache record for a directory listing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirCacheRecord {
    pub root: String,
    pub timestamp: u64, // Unix timestamp in seconds
    #[serde(default)]
    pub mtime_nanos: u64, // Root directory mtime in nanoseconds for mtime validation
    pub items: Vec<String>,
}

impl DirCacheRecord {
    pub fn new(root: String, items: Vec<String>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mtime_nanos = fs::metadata(&root)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Self {
            root,
            timestamp,
            mtime_nanos,
            items,
        }
    }

    pub fn age_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.timestamp)
    }

    /// Encodes the record into a packed, zero-copy binary layout for sub-millisecond retrieval.
    pub fn to_zero_copy_bytes(&self) -> Vec<u8> {
        let root_bytes = self.root.as_bytes();
        let root_len = root_bytes.len() as u32;

        let mut capacity = 4 + 8 + 8 + 4 + root_bytes.len();
        for item in &self.items {
            capacity += item.len() + 1;
        }

        let mut buf = Vec::with_capacity(capacity);
        buf.extend_from_slice(&ZERO_COPY_MAGIC.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&self.mtime_nanos.to_le_bytes());
        buf.extend_from_slice(&root_len.to_le_bytes());
        buf.extend_from_slice(root_bytes);

        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                buf.push(0);
            }
            buf.extend_from_slice(item.as_bytes());
        }
        buf
    }

    /// Decodes record from packed zero-copy binary buffer without intermediate AST allocations.
    pub fn from_zero_copy_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 24 {
            return None;
        }

        let magic = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
        if magic != ZERO_COPY_MAGIC {
            return None;
        }

        let timestamp = u64::from_le_bytes(bytes[4..12].try_into().ok()?);
        let mtime_nanos = u64::from_le_bytes(bytes[12..20].try_into().ok()?);
        let root_len = u32::from_le_bytes(bytes[20..24].try_into().ok()?) as usize;

        if bytes.len() < 24 + root_len {
            return None;
        }

        let root = std::str::from_utf8(&bytes[24..24 + root_len])
            .ok()?
            .to_string();
        let payload = &bytes[24 + root_len..];

        let items = if payload.is_empty() {
            Vec::new()
        } else {
            let payload_str = std::str::from_utf8(payload).ok()?;
            payload_str.split('\0').map(|s| s.to_string()).collect()
        };

        Some(Self {
            root,
            timestamp,
            mtime_nanos,
            items,
        })
    }
}

/// Directory Listing Cache Store powered by `redb`.
#[derive(Clone)]
pub struct DirCacheStore {
    db: Arc<Option<Database>>,
    pub db_path: Option<PathBuf>,
}

impl std::fmt::Debug for DirCacheStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirCacheStore")
            .field("db_path", &self.db_path)
            .field("active", &self.db.is_some())
            .finish()
    }
}

impl DirCacheStore {
    /// Default state path: `~/.local/state/matchmaker/dir_cache.redb`
    pub fn default_db_path() -> Option<PathBuf> {
        let dir = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .map(|d| d.join("matchmaker"))?;
        Some(dir.join("dir_cache.redb"))
    }

    /// Opens or creates the directory cache store at default location.
    pub fn open() -> Self {
        if let Some(path) = Self::default_db_path() {
            Self::open_at(&path).unwrap_or_else(|err| {
                log::warn!("Failed to open dir cache store at {path:?}: {err}. Falling back to in-memory mode.");
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

    /// Opens or creates the directory cache store at a specific path.
    pub fn open_at(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let db = match Database::create(path) {
            Ok(database) => Some(database),
            Err(err) => {
                log::warn!("redb error opening dir cache at {path:?}: {err}.");
                match &err {
                    redb::DatabaseError::DatabaseAlreadyOpen => {
                        let mut retried = None;
                        for _ in 0..10 {
                            std::thread::sleep(std::time::Duration::from_millis(25));
                            if let Ok(d) = Database::create(path) {
                                retried = Some(d);
                                break;
                            }
                        }
                        retried
                    }
                    _ => {
                        if path.exists() {
                            let backup_path = path.with_extension("corrupt.bak");
                            let _ = fs::rename(path, &backup_path);
                            Database::create(path).ok()
                        } else {
                            None
                        }
                    }
                }
            }
        };

        Ok(Self {
            db: Arc::new(db),
            db_path: Some(path.to_path_buf()),
        })
    }

    /// Retrieves cached directory listing record for a root directory path.
    pub fn get(&self, raw_root: &str) -> Option<DirCacheRecord> {
        let Some(db) = self.db.as_ref() else {
            return None;
        };

        let key_str = crate::frecency::normalize_path(raw_root);
        if key_str.is_empty() {
            return None;
        }

        let read_txn = db.begin_read().ok()?;
        let table = read_txn.open_table(DIR_CACHE_TABLE).ok()?;
        let guard = table.get(key_str.as_str()).ok()??;
        let bytes = guard.value();
        if let Some(rec) = DirCacheRecord::from_zero_copy_bytes(bytes) {
            Some(rec)
        } else if let Ok(rec) = postcard::from_bytes::<DirCacheRecord>(bytes) {
            Some(rec)
        } else if let Ok(json_str) = std::str::from_utf8(bytes) {
            serde_json::from_str::<DirCacheRecord>(json_str).ok()
        } else {
            None
        }
    }

    /// Retrieves cached directory listing record only if root directory mtime matches,
    /// invalidating automatically if files/folders were created, deleted, or renamed externally.
    pub fn get_valid(&self, raw_root: &str) -> Option<DirCacheRecord> {
        let rec = self.get(raw_root)?;
        let current_mtime_nanos = fs::metadata(raw_root)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);

        if current_mtime_nanos != 0
            && rec.mtime_nanos != 0
            && current_mtime_nanos != rec.mtime_nanos
        {
            log::debug!(
                "DirCache invalidated for {raw_root}: directory mtime changed ({current_mtime_nanos} != {})",
                rec.mtime_nanos
            );
            return None;
        }
        Some(rec)
    }

    /// Stores/updates cached directory listing record for a root directory path.
    pub fn put(&self, raw_root: &str, items: Vec<String>) -> anyhow::Result<()> {
        let Some(db) = self.db.as_ref() else {
            return Ok(());
        };

        let key_str = crate::frecency::normalize_path(raw_root);
        if key_str.is_empty() {
            return Ok(());
        }

        let record = DirCacheRecord::new(key_str.clone(), items);
        let bytes = record.to_zero_copy_bytes();

        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(DIR_CACHE_TABLE)?;
            table.insert(key_str.as_str(), bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// Removes a root path entry from the directory cache database.
    pub fn remove(&self, raw_root: &str) -> anyhow::Result<bool> {
        let Some(db) = self.db.as_ref() else {
            return Ok(false);
        };

        let key_str = crate::frecency::normalize_path(raw_root);
        if key_str.is_empty() {
            return Ok(false);
        }

        let write_txn = db.begin_write()?;
        let removed = {
            let mut table = write_txn.open_table(DIR_CACHE_TABLE)?;
            table.remove(key_str.as_str())?.is_some()
        };
        write_txn.commit()?;
        Ok(removed)
    }

    /// Purges entries whose root path no longer exists on disk.
    pub fn clean_stale(&self) -> anyhow::Result<usize> {
        let Some(db) = self.db.as_ref() else {
            return Ok(0);
        };

        let mut keys_to_remove = Vec::new();
        if let Ok(read_txn) = db.begin_read() {
            if let Ok(table) = read_txn.open_table(DIR_CACHE_TABLE) {
                if let Ok(iter) = table.iter() {
                    for entry in iter.flatten() {
                        let path_str = entry.0.value();
                        let p = Path::new(path_str);
                        if !p.exists() {
                            keys_to_remove.push(path_str.to_string());
                        }
                    }
                }
            }
        }

        if keys_to_remove.is_empty() {
            return Ok(0);
        }

        let write_txn = db.begin_write()?;
        let removed_count = {
            let mut table = write_txn.open_table(DIR_CACHE_TABLE)?;
            let mut count = 0;
            for key in &keys_to_remove {
                if table.remove(key.as_str())?.is_some() {
                    count += 1;
                }
            }
            count
        };

        write_txn.commit()?;
        Ok(removed_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_cache_store_put_get_remove() -> anyhow::Result<()> {
        let temp_dir = std::env::temp_dir().join("mm_test_dir_cache");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir)?;
        let db_path = temp_dir.join("cache.redb");

        let store = DirCacheStore::open_at(&db_path)?;
        let root_str = temp_dir.to_str().unwrap();

        assert!(store.get(root_str).is_none());

        let items = vec!["src/main.rs".to_string(), "README.md".to_string()];
        store.put(root_str, items.clone())?;

        let cached = store.get(root_str);
        assert!(cached.is_some());
        let rec = cached.unwrap();
        assert_eq!(rec.items, items);

        let removed = store.remove(root_str)?;
        assert!(removed);
        assert!(store.get(root_str).is_none());

        let _ = fs::remove_dir_all(&temp_dir);
        Ok(())
    }

    #[test]
    fn test_zero_copy_encoding_decoding() {
        let rec = DirCacheRecord {
            root: "/home/user/project".to_string(),
            timestamp: 1234567890,
            mtime_nanos: 987654321,
            items: vec![
                "src/lib.rs".to_string(),
                "Cargo.toml".to_string(),
                "docs/README.md".to_string(),
            ],
        };

        let encoded = rec.to_zero_copy_bytes();
        assert!(!encoded.is_empty());
        assert_eq!(&encoded[0..4], &ZERO_COPY_MAGIC.to_le_bytes());

        let decoded = DirCacheRecord::from_zero_copy_bytes(&encoded).unwrap();
        assert_eq!(decoded.root, rec.root);
        assert_eq!(decoded.timestamp, rec.timestamp);
        assert_eq!(decoded.mtime_nanos, rec.mtime_nanos);
        assert_eq!(decoded.items, rec.items);
    }
}
