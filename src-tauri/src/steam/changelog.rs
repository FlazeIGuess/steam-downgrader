//! Update history via the public Steam news API (no login required).
//!
//! Endpoint: `https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/`
//!
//! Returns announcements/patch notes with title, date and body. The `resolver`
//! then maps the `date` field to manifest ids via timestamp alignment.

use serde::{Deserialize, Serialize};

use super::SteamError;

/// Raw response from the Steam news API.
#[derive(Debug, Deserialize)]
struct NewsResponse {
    appnews: AppNews,
}

#[derive(Debug, Deserialize)]
struct AppNews {
    #[serde(default)]
    newsitems: Vec<RawNewsItem>,
}

#[derive(Debug, Deserialize)]
struct RawNewsItem {
    gid: String,
    title: String,
    url: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    contents: String,
    #[serde(default)]
    feedlabel: String,
    /// Unix timestamp (seconds) of publication.
    date: i64,
    #[serde(default)]
    feedname: String,
    #[serde(default)]
    tags: Vec<String>,
}

/// Changelog entry prepared for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct ChangelogEntry {
    pub gid: String,
    pub title: String,
    pub url: String,
    pub author: String,
    /// Cleaned plain-text excerpt (BBCode / clan-image tokens removed).
    pub summary: String,
    /// Unix seconds, same time base as manifest timestamps in the resolver.
    pub date: i64,
    /// ISO-8601 for display.
    pub date_iso: String,
    pub feed: String,
    pub tags: Vec<String>,
}

/// Fetch the update history of an app.
///
/// `count` limits the number of entries. `feeds` optionally filters to specific
/// feeds (e.g. `"steam_community_announcements"`); an empty slice means all feeds.
pub async fn fetch(
    app_id: u32,
    count: u32,
    feeds: &[&str],
) -> Result<Vec<ChangelogEntry>, SteamError> {
    let mut url = format!(
        "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/?appid={app_id}&count={count}&maxlength=600"
    );
    if !feeds.is_empty() {
        url.push_str(&format!("&feeds={}", feeds.join(",")));
    }

    let resp = reqwest::get(&url).await?;
    if !resp.status().is_success() {
        return Err(SteamError::Http(format!("Status {}", resp.status())));
    }

    let parsed: NewsResponse = resp
        .json()
        .await
        .map_err(|e| SteamError::Parse(e.to_string()))?;

    let entries = parsed
        .appnews
        .newsitems
        .into_iter()
        .map(|raw| {
            let date_iso = chrono::DateTime::from_timestamp(raw.date, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            ChangelogEntry {
                gid: raw.gid,
                title: raw.title,
                url: raw.url,
                author: raw.author,
                summary: clean_contents(&raw.contents),
                date: raw.date,
                date_iso,
                feed: if raw.feedlabel.is_empty() {
                    raw.feedname
                } else {
                    raw.feedlabel
                },
                tags: raw.tags,
            }
        })
        .collect();

    Ok(entries)
}

/// Strip the most common Steam BBCode / token artifacts for a preview.
/// Deliberately simple; full BBCode-to-HTML conversion happens later in the UI.
fn clean_contents(raw: &str) -> String {
    let mut s = raw.to_string();

    // Remove {STEAM_CLAN_IMAGE}/... image tokens up to the next whitespace.
    while let Some(start) = s.find("{STEAM_CLAN_IMAGE}") {
        let end = s[start..]
            .find(char::is_whitespace)
            .map(|rel| start + rel)
            .unwrap_or(s.len());
        s.replace_range(start..end, "");
    }

    // Roughly strip common BBCode tags.
    for tag in [
        "[list]", "[/list]", "[*]", "[h1]", "[/h1]", "[h2]", "[/h2]", "[h3]", "[/h3]", "[b]",
        "[/b]", "[i]", "[/i]", "[u]", "[/u]", "[hr][/hr]",
    ] {
        s = s.replace(tag, " ");
    }

    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 400 {
        let truncated: String = collapsed.chars().take(400).collect();
        format!("{truncated}...")
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_clan_image_and_bbcode() {
        let raw = "{STEAM_CLAN_IMAGE}/123/abc.jpg [h2]Patch[/h2] [b]Fix[/b] crash";
        let cleaned = clean_contents(raw);
        assert!(!cleaned.contains("STEAM_CLAN_IMAGE"));
        assert!(!cleaned.contains("[h2]"));
        assert!(cleaned.contains("Patch"));
        assert!(cleaned.contains("crash"));
    }

    /// Live test against the real Steam news API (Terraria). Network-dependent,
    /// so ignored by default: run with `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_fetch_terraria() {
        let entries = fetch(105600, 5, &["steam_community_announcements"])
            .await
            .expect("fetch should succeed");
        assert!(!entries.is_empty(), "should return entries");
        let first = &entries[0];
        assert!(!first.title.is_empty());
        assert!(first.date > 0);
        assert_eq!(first.date_iso.len(), 10); // YYYY-MM-DD
        println!("Latest entry: {} ({})", first.title, first.date_iso);
    }
}
