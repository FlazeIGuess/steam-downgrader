//! Library of the signed-in account, via the helper's `owned` command. Only
//! owned games may be downloaded: a hard boundary of this tool.

use serde::Serialize;
use tauri::AppHandle;

use super::{sidecar, SteamError};

#[derive(Debug, Clone, Serialize)]
pub struct OwnedGame {
    pub app_id: u32,
    pub name: Option<String>,
}

/// List the games owned by the signed-in account.
pub async fn list_owned(app: &AppHandle) -> Result<Vec<OwnedGame>, SteamError> {
    let v = sidecar::request(app, "owned", serde_json::json!({})).await?;
    let games = v
        .get("games")
        .and_then(|g| g.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<OwnedGame> = games
        .into_iter()
        .map(|g| OwnedGame {
            app_id: g.get("app_id").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
            name: g
                .get("name")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
        })
        .filter(|g| g.app_id != 0)
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
