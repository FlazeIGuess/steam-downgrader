//! Downloading an old build through the DepotDownloader engine that is compiled
//! into the helper (git submodule). Login reuses the helper's QR token, so there
//! is no separate process, no password prompt, and no console window. Progress
//! arrives as `download_progress` / `download_log` events.

use std::path::PathBuf;

use serde::Serialize;
use tauri::AppHandle;

use super::{resolver::ResolvedVersion, sidecar, SteamError};

#[derive(Debug, Clone, Serialize)]
pub struct DownloadResult {
    pub output_dir: PathBuf,
    pub bytes_total: u64,
}

/// The engine is built in; nothing to provision.
pub fn is_available() -> bool {
    true
}

/// Kept for frontend command compatibility; no-op.
pub async fn provision(_app: &AppHandle) -> Result<String, SteamError> {
    Ok("built-in".into())
}

/// Download the resolved version into `output_dir` via the helper's `download` command.
pub async fn download(
    app: &AppHandle,
    version: &ResolvedVersion,
    output_dir: PathBuf,
) -> Result<DownloadResult, SteamError> {
    let depots: Vec<serde_json::Value> = version
        .depots
        .iter()
        .map(|d| {
            serde_json::json!({
                "depot_id": d.depot_id,
                // Manifest id as a string (64-bit, beyond JS number precision).
                "manifest_id": d.manifest_id.to_string(),
            })
        })
        .collect();

    let out = output_dir.to_string_lossy().into_owned();
    // Downloads can take a long time, so use a 4h timeout.
    let v = sidecar::request_timeout(
        app,
        "download",
        serde_json::json!({
            "app_id": version.app_id,
            "depots": depots,
            "output_dir": out,
        }),
        4 * 3600,
    )
    .await?;

    let bytes_total = v.get("bytes_total").and_then(|x| x.as_u64()).unwrap_or(0);
    Ok(DownloadResult {
        output_dir,
        bytes_total,
    })
}
