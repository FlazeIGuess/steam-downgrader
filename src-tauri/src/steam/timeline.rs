//! In-app build timeline: pick an older build by date, no SteamDB needed.
//!
//! Merges three sources for one game:
//!   1. PICS      the current public build (one dated manifest per depot).
//!   2. depotcache  every past build still cached on this PC, with its date.
//!   3. changelog   the public Steam news feed, to label a build with the patch
//!                  that came right after it ("the build before <patch>").
//!
//! Only the current build and locally cached builds can be resolved in-app; for
//! anything older the UI still links out to SteamDB. That limit is deliberate
//! (we never scrape SteamDB) and surfaced honestly in the frontend.

use serde::Serialize;
use tauri::AppHandle;

use super::{applier, changelog, sidecar, SteamError};

/// One build of one depot, dated and optionally labelled with a patch note.
#[derive(Debug, Clone, Serialize)]
pub struct BuildEntry {
    /// Manifest id (64-bit, crosses IPC as a string like everywhere else).
    pub manifest_id: String,
    /// Build creation time, Unix seconds.
    pub timestamp: i64,
    /// YYYY-MM-DD for display.
    pub date_iso: String,
    /// True for the current public build (the "you are here" anchor).
    pub is_current: bool,
    /// Where the build came from: "pics" (current) or "depotcache" (cached).
    pub source: String,
    /// Title of the update that shipped right after this build, if known, so the
    /// UI can say "the build before <patch>".
    pub patch_title: Option<String>,
}

/// The dated build list for one depot, newest first.
#[derive(Debug, Clone, Serialize)]
pub struct DepotTimeline {
    pub depot_id: u32,
    pub builds: Vec<BuildEntry>,
}

/// A patch note, kept for context next to the builds.
#[derive(Debug, Clone, Serialize)]
pub struct PatchEntry {
    pub title: String,
    pub date: i64,
    pub date_iso: String,
    pub url: String,
}

/// Everything the timeline UI needs for one game.
#[derive(Debug, Clone, Serialize)]
pub struct BuildTimeline {
    pub app_id: u32,
    pub current_build_id: Option<u64>,
    pub current_ts: i64,
    pub depots: Vec<DepotTimeline>,
    pub patches: Vec<PatchEntry>,
}

/// Unix seconds to a YYYY-MM-DD string.
fn iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// The update that shipped right after `ts` (within a window, so an ancient
/// build is not mislabelled with a patch from much later). Enables the accurate
/// "the build before <patch>" phrasing: this build predates that patch.
fn next_patch_after<'a>(patches: &'a [PatchEntry], ts: i64) -> Option<&'a str> {
    const WINDOW: i64 = 45 * 24 * 3600; // 45 days
    patches
        .iter()
        .filter(|p| p.date > ts && p.date - ts <= WINDOW)
        .min_by_key(|p| p.date - ts)
        .map(|p| p.title.as_str())
}

/// Merge the current build with cached builds for one depot into a dated list.
///
/// Pure so it can be tested without a live Steam connection: dedupes by manifest
/// id (current wins), labels each build with the next patch, and sorts newest
/// first.
fn assemble(
    current: Option<(String, i64)>,
    cached: &[(String, i64)],
    patches: &[PatchEntry],
) -> Vec<BuildEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut builds = Vec::new();

    if let Some((mid, ts)) = current {
        seen.insert(mid.clone());
        builds.push(BuildEntry {
            manifest_id: mid,
            timestamp: ts,
            date_iso: iso(ts),
            is_current: true,
            source: "pics".into(),
            patch_title: next_patch_after(patches, ts).map(str::to_string),
        });
    }

    for (mid, ts) in cached {
        if !seen.insert(mid.clone()) {
            continue; // duplicate manifest (already current, or in two cache dirs)
        }
        builds.push(BuildEntry {
            manifest_id: mid.clone(),
            timestamp: *ts,
            date_iso: iso(*ts),
            is_current: false,
            source: "depotcache".into(),
            patch_title: next_patch_after(patches, *ts).map(str::to_string),
        });
    }

    builds.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    builds
}

/// Assemble the full build timeline for an app.
pub async fn build_timeline(app: &AppHandle, app_id: u32) -> Result<BuildTimeline, SteamError> {
    // 1. Current build via PICS (anonymous, no login).
    let info = sidecar::request(app, "appinfo", serde_json::json!({ "app_id": app_id })).await?;
    let current_ts = info
        .get("time_updated")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let current_build_id = info
        .get("build_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<u64>().ok());

    // depot_id -> current manifest id, in PICS order.
    let mut current: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut depot_ids: Vec<u32> = Vec::new();
    if let Some(arr) = info.get("depots").and_then(|d| d.as_array()) {
        for d in arr {
            let Some(id) = d.get("depot_id").and_then(|v| v.as_u64()) else {
                continue;
            };
            let id = id as u32;
            if let Some(mid) = d.get("manifest_id").and_then(|v| v.as_str()) {
                if current.insert(id, mid.to_string()).is_none() {
                    depot_ids.push(id);
                }
            }
        }
    }

    // 2. Historical manifests from Steam's depotcache (dates included).
    let dirs: Vec<String> = applier::depotcache_dirs()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let mut cached: std::collections::HashMap<u32, Vec<(String, i64)>> =
        std::collections::HashMap::new();
    if !dirs.is_empty() {
        if let Ok(v) = sidecar::request_timeout(
            app,
            "local_manifests",
            serde_json::json!({ "dirs": dirs, "depots": depot_ids }),
            120,
        )
        .await
        {
            if let Some(arr) = v.get("manifests").and_then(|m| m.as_array()) {
                for m in arr {
                    let Some(id) = m.get("depot_id").and_then(|v| v.as_u64()) else {
                        continue;
                    };
                    let mid = m
                        .get("manifest_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    if mid.is_empty() {
                        continue;
                    }
                    let ct = m.get("creation_time").and_then(|v| v.as_i64()).unwrap_or(0);
                    cached.entry(id as u32).or_default().push((mid, ct));
                }
            }
        }
    }

    // 3. Patch notes (best effort; used only to label builds).
    let patches: Vec<PatchEntry> = changelog::fetch(app_id, 40, &["steam_community_announcements"])
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| PatchEntry {
            title: c.title,
            date: c.date,
            date_iso: c.date_iso,
            url: c.url,
        })
        .collect();

    // 4. Assemble one timeline per depot.
    let empty: Vec<(String, i64)> = Vec::new();
    let depots: Vec<DepotTimeline> = depot_ids
        .iter()
        .map(|depot_id| {
            let cur = current.get(depot_id).map(|m| (m.clone(), current_ts));
            let hist = cached.get(depot_id).unwrap_or(&empty);
            DepotTimeline {
                depot_id: *depot_id,
                builds: assemble(cur, hist, &patches),
            }
        })
        .collect();

    Ok(BuildTimeline {
        app_id,
        current_build_id,
        current_ts,
        depots,
        patches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(title: &str, date: i64) -> PatchEntry {
        PatchEntry {
            title: title.into(),
            date,
            date_iso: iso(date),
            url: String::new(),
        }
    }

    const DAY: i64 = 24 * 3600;

    #[test]
    fn labels_build_with_the_next_patch() {
        let patches = vec![patch("Balance update", 100 * DAY)];
        // A build from 98 days: the update at day 100 came right after it.
        assert_eq!(next_patch_after(&patches, 98 * DAY), Some("Balance update"));
        // A build long before the window: not labelled (would be misleading).
        assert_eq!(next_patch_after(&patches, 10 * DAY), None);
        // A build after the patch: that patch is not "next".
        assert_eq!(next_patch_after(&patches, 101 * DAY), None);
    }

    #[test]
    fn current_first_then_history_newest_first() {
        let current = Some(("cur".to_string(), 300 * DAY));
        let cached = vec![
            ("a".to_string(), 100 * DAY),
            ("b".to_string(), 200 * DAY),
        ];
        let builds = assemble(current, &cached, &[]);
        assert_eq!(builds.len(), 3);
        assert!(builds[0].is_current);
        assert_eq!(builds[0].manifest_id, "cur");
        assert_eq!(builds[1].manifest_id, "b"); // newer history before older
        assert_eq!(builds[2].manifest_id, "a");
    }

    #[test]
    fn dedupes_current_against_cache() {
        // The current manifest is also present in the depotcache: keep it once,
        // and keep the is_current flag / PICS timestamp.
        let current = Some(("same".to_string(), 300 * DAY));
        let cached = vec![("same".to_string(), 299 * DAY), ("old".to_string(), 100 * DAY)];
        let builds = assemble(current, &cached, &[]);
        assert_eq!(builds.len(), 2);
        assert_eq!(builds[0].manifest_id, "same");
        assert!(builds[0].is_current);
        assert_eq!(builds[0].timestamp, 300 * DAY);
        assert_eq!(builds[1].manifest_id, "old");
    }

    #[test]
    fn dedupes_repeated_cache_entries() {
        // The same manifest can live in several depotcache folders.
        let cached = vec![("x".to_string(), 100 * DAY), ("x".to_string(), 100 * DAY)];
        let builds = assemble(None, &cached, &[]);
        assert_eq!(builds.len(), 1);
    }
}
