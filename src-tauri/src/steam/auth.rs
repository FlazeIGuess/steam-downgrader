//! Steam login through the .NET helper (SteamKit2). 2FA runs over the
//! `steam-event` / `need_code` event plus [`provide_code`].

use serde::Serialize;
use tauri::AppHandle;

use super::{sidecar, SteamError};

#[derive(Debug, Clone, Serialize)]
pub struct Session {
    pub steam_id: u64,
    pub account_name: String,
}

/// Start a username/password login. If Steam Guard is active the helper emits a
/// `need_code` event; the UI supplies the code via [`provide_code`].
pub async fn login(app: &AppHandle, account: &str, password: &str) -> Result<Session, SteamError> {
    let v = sidecar::request(
        app,
        "login",
        serde_json::json!({ "username": account, "password": password }),
    )
    .await?;
    Ok(Session {
        steam_id: v.get("steam_id").and_then(|x| x.as_u64()).unwrap_or(0),
        account_name: v
            .get("account")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// Start a QR login. The helper emits `qr_url` events (the challenge URL, which
/// rotates about every 30s); the UI renders them as a QR code. Resolves once the
/// login is approved in the Steam mobile app.
pub async fn login_qr(app: &AppHandle) -> Result<Session, SteamError> {
    let v = sidecar::request(app, "login_qr", serde_json::json!({})).await?;
    Ok(Session {
        steam_id: v.get("steam_id").and_then(|x| x.as_u64()).unwrap_or(0),
        account_name: v
            .get("account")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// Supply a 2FA code previously requested through a `need_code` event.
pub async fn provide_code(app: &AppHandle, code: &str) -> Result<(), SteamError> {
    sidecar::request(app, "provide_code", serde_json::json!({ "code": code })).await?;
    Ok(())
}
