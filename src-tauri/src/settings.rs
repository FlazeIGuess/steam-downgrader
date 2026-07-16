//! Small persisted app settings (`%APPDATA%\steam-downgrader\settings.json`).
//!
//! Currently only the opt-in state for the community manifest archive. The app
//! works fully without it; nothing is shared unless the user opts in.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bump this when the archive consent text or privacy policy changes, so the app
/// asks again after an update. It does NOT re-ask on every update otherwise.
pub const CONSENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// `None` means the user has not decided yet.
    #[serde(default)]
    pub archive_opt_in: Option<bool>,
    /// The consent version the user last acted on.
    #[serde(default)]
    pub consent_version: u32,
}

/// What the frontend needs: the current choice plus whether to show the dialog.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub archive_opt_in: Option<bool>,
    pub needs_consent: bool,
    pub consent_version: u32,
}

fn store_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("steam-downgrader")
}

fn store_file() -> PathBuf {
    store_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(store_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(s: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(store_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(store_file(), json).map_err(|e| e.to_string())
}

/// Show the consent dialog when the user has not decided yet, or when the consent
/// version was bumped since they last acted (e.g. after a policy change).
pub fn view() -> SettingsView {
    let s = load();
    SettingsView {
        needs_consent: s.archive_opt_in.is_none() || s.consent_version < CONSENT_VERSION,
        archive_opt_in: s.archive_opt_in,
        consent_version: CONSENT_VERSION,
    }
}

pub fn set_opt_in(opt_in: bool) -> Result<SettingsView, String> {
    let mut s = load();
    s.archive_opt_in = Some(opt_in);
    s.consent_version = CONSENT_VERSION;
    save(&s)?;
    Ok(view())
}

/// Whether the community archive is enabled. Used to gate all network calls to it.
pub fn opt_in_enabled() -> bool {
    load().archive_opt_in == Some(true)
}
