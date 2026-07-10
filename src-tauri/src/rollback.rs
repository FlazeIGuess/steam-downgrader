//! Persistent rollback library: every finished download is recorded on disk
//! (`%APPDATA%\steam-downgrader\rollbacks.json`) so downloads survive across
//! sessions and can be applied again without re-downloading.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackDepot {
    pub depot_id: u32,
    pub manifest_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedInfo {
    pub model: String,
    pub path: String,
    pub at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackEntry {
    /// Stable key: app_id plus the sorted depot:manifest list.
    pub id: String,
    pub app_id: u32,
    pub game_name: String,
    pub target_label: String,
    pub target_date: String,
    pub depots: Vec<RollbackDepot>,
    pub download_dir: String,
    pub bytes: u64,
    pub downloaded_at: i64,
    #[serde(default)]
    pub applied: Option<AppliedInfo>,
}

fn store_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("steam-downgrader")
}

fn store_file() -> PathBuf {
    store_dir().join("rollbacks.json")
}

pub fn load() -> Vec<RollbackEntry> {
    std::fs::read_to_string(store_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(list: &[RollbackEntry]) -> Result<(), String> {
    std::fs::create_dir_all(store_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(store_file(), json).map_err(|e| e.to_string())
}

/// Add an entry, replacing any existing one with the same id.
pub fn add(entry: RollbackEntry) -> Result<Vec<RollbackEntry>, String> {
    let mut list = load();
    list.retain(|e| e.id != entry.id);
    list.insert(0, entry);
    save(&list)?;
    Ok(list)
}

/// Remove an entry, optionally deleting its downloaded files.
pub fn remove(id: &str, delete_files: bool) -> Result<Vec<RollbackEntry>, String> {
    let mut list = load();
    if delete_files {
        if let Some(e) = list.iter().find(|e| e.id == id) {
            let _ = std::fs::remove_dir_all(&e.download_dir);
        }
    }
    list.retain(|e| e.id != id);
    save(&list)?;
    Ok(list)
}

pub fn set_applied(id: &str, applied: Option<AppliedInfo>) -> Result<Vec<RollbackEntry>, String> {
    let mut list = load();
    if let Some(e) = list.iter_mut().find(|e| e.id == id) {
        e.applied = applied;
    }
    save(&list)?;
    Ok(list)
}
