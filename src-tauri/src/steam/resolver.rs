//! Resolution: changelog date to depot + manifest id via timestamp alignment.
//!
//! Every manifest has a timestamp. Given a target date, we pick the manifest
//! whose timestamp falls directly before it ("the version before that").
//!
//! The `(manifest id, timestamp)` list per depot comes from a fallback chain:
//!   1. PICS via login (current and most recent builds; clean)
//!   2. community manifest archives (deep history; inconsistent)
//!   3. manual entry (last resort)

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::{sidecar, SteamError};

/// A known build of a depot at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepotManifest {
    pub depot_id: u32,
    // Manifest ids are 64-bit and exceed JS number precision (2^53), so they
    // cross the IPC boundary as strings; otherwise large ids would be corrupted.
    #[serde(with = "u64_str")]
    pub manifest_id: u64,
    /// Unix seconds, same time base as `ChangelogEntry::date`.
    pub timestamp: i64,
    /// Optional associated Steam build id.
    pub build_id: Option<u64>,
    /// Where the mapping came from (surfaced in the UI for transparency).
    pub source: ManifestSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ManifestSource {
    Pics,
    CommunityArchive,
    SteamDb,
    Manual,
}

/// The result of a resolution: which depots + manifests make up "the version before".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedVersion {
    pub app_id: u32,
    /// Target time (Unix seconds) to roll back to.
    pub target_timestamp: i64,
    /// Per relevant depot, the manifest that was active at that time.
    pub depots: Vec<DepotManifest>,
    pub source: ManifestSource,
}

/// From a manifest history, pick the manifest active directly before `before_ts`.
///
/// Pure function, so it can be tested independently of the data source.
pub fn manifest_active_before(history: &[DepotManifest], before_ts: i64) -> Option<&DepotManifest> {
    history
        .iter()
        .filter(|m| m.timestamp < before_ts)
        .max_by_key(|m| m.timestamp)
}

/// PICS source: current depot manifests + build timestamp of an app (anonymous,
/// no login). This is source 1 of the fallback chain.
pub async fn appinfo(app: &AppHandle, app_id: u32) -> Result<Vec<DepotManifest>, SteamError> {
    let v = sidecar::request(app, "appinfo", serde_json::json!({ "app_id": app_id })).await?;
    let ts = v
        .get("time_updated")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let build = v
        .get("build_id")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<u64>().ok());
    let depots = v
        .get("depots")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(depots
        .into_iter()
        .filter_map(|d| {
            Some(DepotManifest {
                depot_id: d.get("depot_id")?.as_u64()? as u32,
                manifest_id: d.get("manifest_id")?.as_str()?.parse::<u64>().ok()?,
                timestamp: ts,
                build_id: build,
                source: ManifestSource::Pics,
            })
        })
        .collect())
}

/// Resolve the target version ("the version before `before_ts`") per depot.
///
/// `extra_history` lets the UI feed in additional known manifests (entered
/// manually or cached), which is needed for deep history that PICS does not
/// return. For each depot the newest manifest before `before_ts` is chosen.
pub async fn resolve(
    app: &AppHandle,
    app_id: u32,
    before_ts: i64,
    extra_history: Vec<DepotManifest>,
) -> Result<ResolvedVersion, SteamError> {
    // Merge PICS (current) with any supplied history.
    let mut all = appinfo(app, app_id).await?;
    all.extend(extra_history);

    // Group by depot.
    let mut by_depot: std::collections::HashMap<u32, Vec<DepotManifest>> =
        std::collections::HashMap::new();
    for m in all {
        by_depot.entry(m.depot_id).or_default().push(m);
    }

    let mut chosen = Vec::new();
    let mut missing = Vec::new();
    for (depot_id, history) in &by_depot {
        match manifest_active_before(history, before_ts) {
            Some(m) => chosen.push(m.clone()),
            None => missing.push(*depot_id),
        }
    }

    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(SteamError::Unresolved(format!(
            "No older version is available in the sources for these depots: {missing:?}. \
             Enter a manifest id manually, or pick a later version."
        )));
    }

    let source = if chosen.iter().all(|m| m.source == ManifestSource::Pics) {
        ManifestSource::Pics
    } else {
        ManifestSource::Manual
    };

    Ok(ResolvedVersion {
        app_id,
        target_timestamp: before_ts,
        depots: chosen,
        source,
    })
}

/// Serialize u64 as a string (JS precision for large manifest ids).
mod u64_str {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(ts: i64) -> DepotManifest {
        DepotManifest {
            depot_id: 1,
            manifest_id: ts as u64,
            timestamp: ts,
            build_id: None,
            source: ManifestSource::Manual,
        }
    }

    #[test]
    fn picks_latest_before_target() {
        let hist = vec![mk(100), mk(200), mk(300)];
        // A patch landed at 250, so "the version before" is the manifest from 200.
        let got = manifest_active_before(&hist, 250).unwrap();
        assert_eq!(got.timestamp, 200);
    }

    #[test]
    fn none_when_all_after() {
        let hist = vec![mk(400), mk(500)];
        assert!(manifest_active_before(&hist, 300).is_none());
    }
}
