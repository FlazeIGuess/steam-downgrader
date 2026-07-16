//! Community manifest archive client (opt-in).
//!
//! Talks to the Cloudflare Worker backend (see `steam-downgrader-archive/`). Only
//! public manifest metadata is ever sent, and only when the user has opted in
//! (the caller gates on `crate::settings::opt_in_enabled()`). Everything here is
//! best effort: any error just yields an empty result, so the app keeps working
//! offline.

use serde::{Deserialize, Serialize};

/// Deployed Worker URL. Set this after deploying `steam-downgrader-archive`
/// (for example `https://steam-downgrader-archive.<you>.workers.dev`). An empty
/// string disables the archive entirely.
const ARCHIVE_BASE_URL: &str = "https://steam-downgrader-archive.benow.workers.dev";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveManifest {
    pub depot_id: u32,
    pub manifest_id: String,
    #[serde(default)]
    pub build_id: Option<u64>,
    #[serde(default)]
    pub timestamp: Option<i64>,
    /// Objective, patch-derived description ("the build before X"). Never a
    /// user's custom version name.
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestsResponse {
    #[serde(default)]
    manifests: Vec<ArchiveManifest>,
}

pub fn is_configured() -> bool {
    !ARCHIVE_BASE_URL.is_empty()
}

/// Fetch all manifests the archive knows for an app. Empty on any error.
pub async fn fetch(app_id: u32) -> Vec<ArchiveManifest> {
    if !is_configured() {
        return Vec::new();
    }
    let url = format!("{ARCHIVE_BASE_URL}/v1/manifests/{app_id}");
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => resp
            .json::<ManifestsResponse>()
            .await
            .map(|r| r.manifests)
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Send manifests to the archive. Returns whether the POST succeeded. Low level;
/// prefer [`contribute_new`], which skips manifests already sent from this machine.
pub async fn contribute(app_id: u32, app_version: &str, depots: &[ArchiveManifest]) -> bool {
    if !is_configured() || depots.is_empty() {
        return false;
    }
    let url = format!("{ARCHIVE_BASE_URL}/v1/contribute");
    let body = serde_json::json!({
        "app_id": app_id,
        "app_version": app_version,
        "depots": depots,
    });
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.post(&url).json(&body).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Contribute only the manifests this machine has not sent before, then remember
/// them so they are not re-sent on every game open. Best effort.
pub async fn contribute_new(app_id: u32, app_version: &str, depots: &[ArchiveManifest]) {
    if !is_configured() || depots.is_empty() {
        return;
    }
    let sent = cache::load();
    let fresh: Vec<ArchiveManifest> = depots
        .iter()
        .filter(|d| !sent.contains(&key(app_id, d.depot_id, &d.manifest_id)))
        .cloned()
        .collect();
    if fresh.is_empty() {
        return; // nothing new; skip the request entirely
    }
    if contribute(app_id, app_version, &fresh).await {
        let keys: Vec<String> = fresh
            .iter()
            .map(|d| key(app_id, d.depot_id, &d.manifest_id))
            .collect();
        cache::mark(&keys);
    }
}

fn key(app_id: u32, depot_id: u32, manifest_id: &str) -> String {
    format!("{app_id}:{depot_id}:{manifest_id}")
}

/// Local record of manifests already contributed from this machine, so they are
/// not re-sent on every game open. Stored at
/// `%APPDATA%\steam-downgrader\contributed.json` as a plain array of keys.
mod cache {
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn file() -> PathBuf {
        let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
        PathBuf::from(base)
            .join("steam-downgrader")
            .join("contributed.json")
    }

    pub fn load() -> HashSet<String> {
        std::fs::read_to_string(file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn mark(keys: &[String]) {
        let mut set = load(); // re-load so concurrent marks merge rather than clobber
        let mut changed = false;
        for k in keys {
            changed |= set.insert(k.clone());
        }
        if !changed {
            return;
        }
        if let Some(dir) = file().parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(&set) {
            let _ = std::fs::write(file(), json);
        }
    }
}
