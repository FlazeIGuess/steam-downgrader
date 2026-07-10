//! Steam integration: changelogs, version resolution, downloading and applying
//! downgrades.
//!
//! - [`changelog`]  Public Steam news API to a readable update history.
//! - [`resolver`]   Changelog date to depot + manifest id (timestamp alignment).
//! - [`library`]    List games the account owns (ownership boundary).
//! - [`downloader`] Wrap the embedded DepotDownloader engine to fetch an old build.
//! - [`applier`]    Model A (in-place freeze) and model B (separate copy + shortcut).
//! - [`auth`]       Steam login via the SteamKit helper (2FA / Guard).

// The changelog/resolver path is kept but not wired to the current UI.
#![allow(dead_code)]

pub mod applier;
pub mod auth;
pub mod changelog;
pub mod downloader;
pub mod library;
pub mod resolver;
pub mod sidecar;

use thiserror::Error;

/// Central error type, serialized to a string across the Tauri command boundary.
#[derive(Debug, Error)]
pub enum SteamError {
    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Could not parse response: {0}")]
    Parse(String),

    #[error("Not signed in")]
    NotAuthenticated,

    #[error("Game not owned by this account (app id {0})")]
    NotOwned(u32),

    #[error("Could not resolve version: {0}")]
    Unresolved(String),

    #[error("Not implemented yet: {0}")]
    NotImplemented(&'static str),

    #[error("{0}")]
    Other(String),
}

impl From<reqwest::Error> for SteamError {
    fn from(e: reqwest::Error) -> Self {
        SteamError::Http(e.to_string())
    }
}

/// Tauri commands return `Result<T, String>`; this alias keeps that consistent.
pub type CmdResult<T> = Result<T, String>;

impl From<SteamError> for String {
    fn from(e: SteamError) -> String {
        e.to_string()
    }
}
