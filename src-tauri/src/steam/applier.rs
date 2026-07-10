//! Applying the downgrade. Two models (the user picks per game):
//!
//! Model A - in-place overwrite + freeze
//!   1. Move the current install folder to `<name>.downgrader-backup` (fast rename).
//!   2. Copy the old build into the install folder.
//!   3. Patch `appmanifest_<appid>.acf`: `AutoUpdateBehavior "1"`, `StateFlags "6"`.
//!   4. Note: online, Steam realistically needs Offline Mode / an update block.
//!
//! Model B - separate copy + non-Steam shortcut (robust)
//!   1. Copy the old build into a separate `<install>_frozen` folder.
//!   2. The original is left untouched (stays current for online/MP).
//!   3. Add a non-Steam entry to `userdata/<id>/config/shortcuts.vdf` (binary VDF).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::SteamError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ApplyModel {
    /// In-place overwrite + freeze.
    InPlaceFreeze,
    /// Separate copy + non-Steam shortcut.
    SeparateCopyShortcut,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyResult {
    pub model: ApplyModel,
    pub backup_or_copy_path: PathBuf,
    pub launch_hint: String,
    pub warnings: Vec<String>,
}

// --- Public orchestration ----------------------------------------------------

pub async fn apply(
    model: ApplyModel,
    app_id: u32,
    game_name: &str,
    downloaded_dir: PathBuf,
    install_dir: PathBuf,
) -> Result<ApplyResult, SteamError> {
    if !downloaded_dir.exists() {
        return Err(SteamError::Other(format!(
            "download directory is missing: {}",
            downloaded_dir.display()
        )));
    }
    match model {
        ApplyModel::InPlaceFreeze => apply_in_place(app_id, downloaded_dir, install_dir),
        ApplyModel::SeparateCopyShortcut => {
            apply_separate(app_id, game_name, downloaded_dir, install_dir)
        }
    }
}

/// Undo an applied downgrade, per model. `applied_path` is the path returned by
/// apply (the backup folder for A, the frozen copy for B).
pub async fn revert(
    model: ApplyModel,
    app_id: u32,
    game_name: &str,
    applied_path: PathBuf,
) -> Result<(), SteamError> {
    match model {
        ApplyModel::InPlaceFreeze => revert_in_place(app_id, applied_path),
        ApplyModel::SeparateCopyShortcut => revert_separate(game_name, applied_path),
    }
}

/// Undo model A: remove the frozen build, restore the original from `backup`,
/// and re-enable auto-update in the ACF.
fn revert_in_place(app_id: u32, backup: PathBuf) -> Result<(), SteamError> {
    if !backup.exists() {
        return Err(SteamError::Other(
            "no backup found, nothing to undo".into(),
        ));
    }
    let install_dir = install_dir_from_backup(&backup).ok_or_else(|| {
        SteamError::Other("could not derive the install folder from the backup path".into())
    })?;
    if install_dir.exists() {
        std::fs::remove_dir_all(&install_dir).map_err(io)?;
    }
    std::fs::rename(&backup, &install_dir).map_err(io)?;

    // Restore the ACF if a backup exists.
    if let Some(acf) = find_acf(app_id, &install_dir) {
        let acf_bak = acf.with_extension("acf.downgrader-backup");
        if acf_bak.exists() {
            std::fs::copy(&acf_bak, &acf).map_err(io)?;
            let _ = std::fs::remove_file(&acf_bak);
        }
    }
    Ok(())
}

/// Undo model B: remove the non-Steam shortcut(s) pointing at the frozen copy
/// from every shortcuts.vdf and delete the copy (`frozen`). The original was
/// never changed, so there is nothing to restore.
fn revert_separate(game_name: &str, frozen: PathBuf) -> Result<(), SteamError> {
    if let Ok(configs) = find_steam_userdata_configs() {
        for cfg in configs {
            let _ = remove_shortcut(&cfg, &frozen, game_name);
        }
    }
    if frozen.exists() {
        std::fs::remove_dir_all(&frozen).map_err(io)?;
    }
    Ok(())
}

/// `<install>.downgrader-backup` -> `<install>`.
fn install_dir_from_backup(backup: &Path) -> Option<PathBuf> {
    let name = backup.file_name()?.to_string_lossy();
    let orig = name.strip_suffix(".downgrader-backup")?;
    Some(backup.with_file_name(orig))
}

// --- Model A ------------------------------------------------------------------

fn apply_in_place(
    app_id: u32,
    downloaded_dir: PathBuf,
    install_dir: PathBuf,
) -> Result<ApplyResult, SteamError> {
    let backup = backup_path_for(&install_dir);

    // 1. Preserve the true original. On the first downgrade, move the install
    //    aside as the backup. If a backup already exists (a prior in-place
    //    downgrade is still active), keep it and just replace the current build,
    //    so switching methods or re-applying does not fail.
    if backup.exists() {
        if install_dir.exists() {
            std::fs::remove_dir_all(&install_dir).map_err(io)?;
        }
    } else if install_dir.exists() {
        std::fs::rename(&install_dir, &backup).map_err(io)?;
    }
    // 2. Copy the old build into the original location.
    copy_dir_all(&downloaded_dir, &install_dir)?;

    // 3. Patch the ACF (disable auto-update, mark as fully installed).
    let mut warnings = Vec::new();
    if let Some(acf) = find_acf(app_id, &install_dir) {
        // Back up the original ACF once (never overwrite an earlier backup).
        let acf_bak = acf.with_extension("acf.downgrader-backup");
        if !acf_bak.exists() {
            let _ = std::fs::copy(&acf, &acf_bak);
        }
        match patch_acf_file(&acf) {
            Ok(()) => {}
            Err(e) => warnings.push(format!("could not patch the ACF: {e}")),
        }
    } else {
        warnings.push(
            "appmanifest_*.acf not found, auto-update block not set.".into(),
        );
    }

    warnings.push(
        "Online, Steam may still mark this as 'update required'. Safest: start Steam in \
         Offline Mode, or leave auto-update disabled."
            .into(),
    );

    Ok(ApplyResult {
        model: ApplyModel::InPlaceFreeze,
        backup_or_copy_path: backup,
        launch_hint: "Start it through Steam as usual (in Offline Mode if needed).".into(),
        warnings,
    })
}

fn backup_path_for(install_dir: &Path) -> PathBuf {
    let name = install_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "game".into());
    install_dir.with_file_name(format!("{name}.downgrader-backup"))
}

// --- Model B ------------------------------------------------------------------

fn apply_separate(
    app_id: u32,
    game_name: &str,
    downloaded_dir: PathBuf,
    install_dir: PathBuf,
) -> Result<ApplyResult, SteamError> {
    let frozen = install_dir.with_file_name(format!(
        "{}_frozen",
        install_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("app_{app_id}"))
    ));
    if frozen.exists() {
        std::fs::remove_dir_all(&frozen).map_err(io)?;
    }
    copy_dir_all(&downloaded_dir, &frozen)?;

    let mut warnings = Vec::new();
    let exe = guess_main_exe(&frozen);
    let launch_hint = match &exe {
        Some(e) => format!("Launch directly (leave Steam running in the background): {}", e.display()),
        None => "Pick the main .exe in the frozen folder manually.".into(),
    };

    // Best-effort: add a non-Steam shortcut to shortcuts.vdf.
    match (find_steam_userdata_configs(), &exe) {
        (Ok(configs), Some(exe)) if !configs.is_empty() => {
            for cfg in configs {
                if let Err(e) = add_shortcut(&cfg, game_name, exe) {
                    warnings.push(format!("shortcuts.vdf ({}) not updated: {e}", cfg.display()));
                }
            }
        }
        (Ok(_), _) => warnings.push("No Steam user profiles found for the shortcut.".into()),
        (Err(e), _) => warnings.push(format!("Steam userdata not found: {e}")),
    }
    if exe.is_none() {
        warnings.push("Main .exe not detected automatically.".into());
    }
    warnings.push("Restart Steam after adding so the entry shows up.".into());

    Ok(ApplyResult {
        model: ApplyModel::SeparateCopyShortcut,
        backup_or_copy_path: frozen,
        launch_hint,
        warnings,
    })
}

/// Guess the main .exe (largest .exe, skipping obvious helper programs).
pub fn guess_main_exe(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in walkdir::WalkDir::new(dir).max_depth(2).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.extension().map(|e| e.eq_ignore_ascii_case("exe")) != Some(true) {
            continue;
        }
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        if ["unins", "crashh", "vcredist", "dxsetup", "setup"].iter().any(|s| name.contains(s)) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if best.as_ref().map(|(s, _)| size > *s).unwrap_or(true) {
            best = Some((size, p.to_path_buf()));
        }
    }
    best.map(|(_, p)| p)
}

// --- ACF (text VDF) ----------------------------------------------------------

fn find_acf(app_id: u32, install_dir: &Path) -> Option<PathBuf> {
    // install_dir = .../steamapps/common/<name>, so steamapps is two levels up.
    let steamapps = install_dir.parent()?.parent()?;
    let acf = steamapps.join(format!("appmanifest_{app_id}.acf"));
    acf.exists().then_some(acf)
}

fn patch_acf_file(path: &Path) -> Result<(), SteamError> {
    let content = std::fs::read_to_string(path).map_err(io)?;
    let patched = patch_acf(&content);
    std::fs::write(path, patched).map_err(io)?;
    Ok(())
}

/// Set `AutoUpdateBehavior "1"` and `StateFlags "6"` (installed, updates
/// suspended) in an appmanifest ACF. Missing keys are added.
fn patch_acf(content: &str) -> String {
    let mut out = set_kv(content, "AutoUpdateBehavior", "1");
    out = set_kv(&out, "StateFlags", "6");
    out
}

/// Replace or add a `"key" "value"` entry in a text VDF.
fn set_kv(content: &str, key: &str, value: &str) -> String {
    let needle = format!("\"{key}\"");
    if let Some(pos) = content.find(&needle) {
        // Find the key's line and replace it entirely (keeping indentation).
        let line_start = content[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let line_end = content[pos..].find('\n').map(|i| pos + i).unwrap_or(content.len());
        let indent: String = content[line_start..pos].chars().take_while(|c| c.is_whitespace()).collect();
        let mut s = String::with_capacity(content.len());
        s.push_str(&content[..line_start]);
        s.push_str(&format!("{indent}\"{key}\"\t\t\"{value}\""));
        s.push_str(&content[line_end..]);
        s
    } else {
        // Insert before the last closing brace.
        if let Some(pos) = content.rfind('}') {
            let mut s = String::with_capacity(content.len() + 32);
            s.push_str(&content[..pos]);
            s.push_str(&format!("\t\"{key}\"\t\t\"{value}\"\n"));
            s.push_str(&content[pos..]);
            s
        } else {
            format!("{content}\n\"{key}\"\t\t\"{value}\"\n")
        }
    }
}

// --- Steam paths -------------------------------------------------------------

fn steam_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("STEAM_PATH") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    for c in [
        r"C:\Program Files (x86)\Steam",
        r"C:\Program Files\Steam",
    ] {
        let pb = PathBuf::from(c);
        if pb.exists() {
            return Some(pb);
        }
    }
    None
}

/// Find an app's install path via libraryfolders.vdf + appmanifest.
pub fn find_install_dir(app_id: u32) -> Option<PathBuf> {
    let root = steam_root()?;
    let mut libraries = vec![root.clone()];

    let libfolders = root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&libfolders) {
        for path in extract_library_paths(&text) {
            libraries.push(PathBuf::from(path));
        }
    }

    for lib in libraries {
        let steamapps = lib.join("steamapps");
        let acf = steamapps.join(format!("appmanifest_{app_id}.acf"));
        if let Ok(text) = std::fs::read_to_string(&acf) {
            if let Some(installdir) = extract_kv_value(&text, "installdir") {
                let dir = steamapps.join("common").join(installdir);
                if dir.exists() {
                    return Some(dir);
                }
            }
        }
    }
    None
}

/// A locally installed Steam game, read from its appmanifest_*.acf.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledGame {
    pub app_id: u32,
    pub name: String,
    pub install_dir: String,
    pub build_id: Option<u64>,
    pub size_bytes: Option<u64>,
    pub last_updated: Option<i64>,
}

/// Scan every Steam library for installed apps. Fully local, no login.
pub fn list_installed() -> Vec<InstalledGame> {
    let mut games = Vec::new();
    let Some(root) = steam_root() else {
        return games;
    };

    let mut libraries = vec![root.clone()];
    if let Ok(text) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for p in extract_library_paths(&text) {
            libraries.push(PathBuf::from(p));
        }
    }

    let mut seen = std::collections::HashSet::new();
    for lib in libraries {
        let steamapps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&steamapps) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let fname = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let app_id = extract_kv_value(&text, "appid")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if app_id == 0 || !seen.insert(app_id) {
                continue;
            }
            let installdir = extract_kv_value(&text, "installdir").unwrap_or_default();
            let full = steamapps.join("common").join(&installdir);
            games.push(InstalledGame {
                app_id,
                name: extract_kv_value(&text, "name").unwrap_or_else(|| format!("App {app_id}")),
                install_dir: full.to_string_lossy().into_owned(),
                build_id: extract_kv_value(&text, "buildid").and_then(|s| s.parse().ok()),
                size_bytes: extract_kv_value(&text, "SizeOnDisk").and_then(|s| s.parse().ok()),
                last_updated: extract_kv_value(&text, "LastUpdated").and_then(|s| s.parse().ok()),
            });
        }
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

/// All existing Steam `depotcache` folders (main + config + per library).
/// They contain historical `<depot>_<manifest>.manifest` files.
pub fn depotcache_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let Some(root) = steam_root() else {
        return dirs;
    };
    dirs.push(root.join("depotcache"));
    dirs.push(root.join("config").join("depotcache"));

    let mut libs = vec![root.clone()];
    if let Ok(t) = std::fs::read_to_string(root.join("steamapps").join("libraryfolders.vdf")) {
        for p in extract_library_paths(&t) {
            libs.push(PathBuf::from(p));
        }
    }
    for l in libs {
        dirs.push(l.join("steamapps").join("depotcache"));
    }
    dirs.retain(|d| d.exists());
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Pull all `"path"` values from libraryfolders.vdf.
fn extract_library_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text;
    let mut idx = 0;
    while let Some(rel) = bytes[idx..].find("\"path\"") {
        let pos = idx + rel + "\"path\"".len();
        if let Some(v) = next_quoted(&bytes[pos..]) {
            out.push(v.replace("\\\\", "\\"));
        }
        idx = pos;
    }
    out
}

/// Read the value of a `"key" "value"` pair from a text VDF.
fn extract_kv_value(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let pos = text.find(&needle)? + needle.len();
    next_quoted(&text[pos..]).map(|s| s.replace("\\\\", "\\"))
}

/// Return the contents of the next `"..."` in the string.
fn next_quoted(s: &str) -> Option<String> {
    let start = s.find('"')? + 1;
    let rest = &s[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// All `userdata/<id>/config/shortcuts.vdf` paths (including non-existent ones, to create).
fn find_steam_userdata_configs() -> Result<Vec<PathBuf>, SteamError> {
    let root = steam_root().ok_or_else(|| SteamError::Other("Steam directory not found".into()))?;
    let userdata = root.join("userdata");
    if !userdata.exists() {
        return Err(SteamError::Other("userdata directory is missing".into()));
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&userdata).map_err(io)? {
        let entry = entry.map_err(io)?;
        if entry.path().is_dir() {
            out.push(entry.path().join("config").join("shortcuts.vdf"));
        }
    }
    Ok(out)
}

// --- shortcuts.vdf (binary VDF) ----------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub app_name: String,
    pub exe: String,
    pub start_dir: String,
}

/// Add a non-Steam shortcut (read existing, append, write back).
fn add_shortcut(vdf_path: &Path, name: &str, exe: &Path) -> Result<(), SteamError> {
    let mut shortcuts = if vdf_path.exists() {
        let bytes = std::fs::read(vdf_path).map_err(io)?;
        parse_shortcuts(&bytes).unwrap_or_default()
    } else {
        if let Some(parent) = vdf_path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        Vec::new()
    };

    let exe_str = exe.to_string_lossy().into_owned();
    let start_dir = exe
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Avoid duplicates.
    if shortcuts.iter().any(|s| s.exe == exe_str && s.app_name == name) {
        return Ok(());
    }
    shortcuts.push(Shortcut {
        app_name: name.to_string(),
        exe: format!("\"{exe_str}\""),
        start_dir: format!("\"{start_dir}\""),
    });

    let bytes = serialize_shortcuts(&shortcuts);
    std::fs::write(vdf_path, bytes).map_err(io)?;
    Ok(())
}

/// Remove non-Steam shortcuts whose exe lives in `frozen` (the ones the apply
/// step created). Returns true if anything was removed.
fn remove_shortcut(vdf_path: &Path, frozen: &Path, _game_name: &str) -> Result<bool, SteamError> {
    if !vdf_path.exists() {
        return Ok(false);
    }
    let bytes = std::fs::read(vdf_path).map_err(io)?;
    let Some(mut shortcuts) = parse_shortcuts(&bytes) else {
        return Ok(false);
    };
    let needle = frozen.to_string_lossy().to_string();
    let before = shortcuts.len();
    shortcuts.retain(|s| !s.exe.contains(&needle));
    if shortcuts.len() == before {
        return Ok(false);
    }
    std::fs::write(vdf_path, serialize_shortcuts(&shortcuts)).map_err(io)?;
    Ok(true)
}

// Binary VDF type tags.
const VDF_MAP: u8 = 0x00;
const VDF_STRING: u8 = 0x01;
const VDF_END: u8 = 0x08;

/// Serialize the shortcuts.vdf structure:
/// `\0 "shortcuts" { "0" { fields... } "1" {...} } END END`.
fn serialize_shortcuts(shortcuts: &[Shortcut]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(VDF_MAP);
    push_cstr(&mut b, "shortcuts");
    for (i, sc) in shortcuts.iter().enumerate() {
        b.push(VDF_MAP);
        push_cstr(&mut b, &i.to_string());
        push_string_field(&mut b, "AppName", &sc.app_name);
        push_string_field(&mut b, "Exe", &sc.exe);
        push_string_field(&mut b, "StartDir", &sc.start_dir);
        b.push(VDF_END); // end of this entry
    }
    b.push(VDF_END); // end of the "shortcuts" map
    b.push(VDF_END); // end of the root document
    b
}

fn push_string_field(b: &mut Vec<u8>, key: &str, value: &str) {
    b.push(VDF_STRING);
    push_cstr(b, key);
    push_cstr(b, value);
}

fn push_cstr(b: &mut Vec<u8>, s: &str) {
    b.extend_from_slice(s.as_bytes());
    b.push(0x00);
}

/// Minimal shortcuts.vdf parser: reads AppName/Exe/StartDir per entry.
fn parse_shortcuts(bytes: &[u8]) -> Option<Vec<Shortcut>> {
    let mut i = 0usize;
    // Root map + "shortcuts"
    if bytes.get(i)? != &VDF_MAP {
        return None;
    }
    i += 1;
    let _root_key = read_cstr(bytes, &mut i)?;

    let mut out = Vec::new();
    loop {
        match bytes.get(i)? {
            &VDF_END => {
                break; // end of the shortcuts map
            }
            &VDF_MAP => {
                i += 1;
                let _index = read_cstr(bytes, &mut i)?;
                let sc = parse_entry(bytes, &mut i)?;
                out.push(sc);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn parse_entry(bytes: &[u8], i: &mut usize) -> Option<Shortcut> {
    let mut app_name = String::new();
    let mut exe = String::new();
    let mut start_dir = String::new();
    loop {
        match bytes.get(*i)? {
            &VDF_END => {
                *i += 1;
                break;
            }
            &VDF_STRING => {
                *i += 1;
                let key = read_cstr(bytes, i)?;
                let val = read_cstr(bytes, i)?;
                match key.to_lowercase().as_str() {
                    "appname" => app_name = val,
                    "exe" => exe = val,
                    "startdir" => start_dir = val,
                    _ => {}
                }
            }
            &VDF_MAP => {
                // skip a nested map (e.g. tags)
                *i += 1;
                let _k = read_cstr(bytes, i)?;
                skip_map(bytes, i)?;
            }
            &0x02 => {
                // int32
                *i += 1;
                let _k = read_cstr(bytes, i)?;
                *i += 4;
            }
            _ => return None,
        }
    }
    Some(Shortcut { app_name, exe, start_dir })
}

fn skip_map(bytes: &[u8], i: &mut usize) -> Option<()> {
    loop {
        match bytes.get(*i)? {
            &VDF_END => {
                *i += 1;
                return Some(());
            }
            &VDF_STRING => {
                *i += 1;
                read_cstr(bytes, i)?;
                read_cstr(bytes, i)?;
            }
            &VDF_MAP => {
                *i += 1;
                read_cstr(bytes, i)?;
                skip_map(bytes, i)?;
            }
            &0x02 => {
                *i += 1;
                read_cstr(bytes, i)?;
                *i += 4;
            }
            _ => return None,
        }
    }
}

fn read_cstr(bytes: &[u8], i: &mut usize) -> Option<String> {
    let start = *i;
    while *bytes.get(*i)? != 0x00 {
        *i += 1;
    }
    let s = String::from_utf8_lossy(&bytes[start..*i]).into_owned();
    *i += 1; // null byte
    Some(s)
}

// --- Helpers -----------------------------------------------------------------

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), SteamError> {
    std::fs::create_dir_all(dst).map_err(io)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.map_err(|e| SteamError::Other(e.to_string()))?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .map_err(|e| SteamError::Other(e.to_string()))?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).map_err(io)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::copy(entry.path(), &target).map_err(io)?;
        }
    }
    Ok(())
}

fn io(e: std::io::Error) -> SteamError {
    SteamError::Other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_acf_replaces_existing_and_adds_missing() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"105600\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"buildid\"\t\t\"111\"\n}\n";
        let patched = patch_acf(acf);
        assert!(patched.contains("\"StateFlags\"\t\t\"6\""));
        assert!(patched.contains("\"AutoUpdateBehavior\"\t\t\"1\""));
        // appid is preserved
        assert!(patched.contains("\"appid\"\t\t\"105600\""));
    }

    #[test]
    fn extract_installdir_from_acf() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"105600\"\n\t\"installdir\"\t\t\"Terraria\"\n}\n";
        assert_eq!(extract_kv_value(acf, "installdir"), Some("Terraria".into()));
    }

    #[test]
    fn extract_library_paths_parses_all() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";
        let paths = extract_library_paths(vdf);
        assert_eq!(paths.len(), 2);
        assert!(paths[1].contains("SteamLibrary"));
    }

    #[test]
    fn shortcuts_roundtrip() {
        let shortcuts = vec![
            Shortcut {
                app_name: "Terraria (old)".into(),
                exe: "\"C:\\games\\terraria\\Terraria.exe\"".into(),
                start_dir: "\"C:\\games\\terraria\"".into(),
            },
            Shortcut {
                app_name: "Witcher 3 v1.31".into(),
                exe: "\"D:\\w3\\witcher3.exe\"".into(),
                start_dir: "\"D:\\w3\"".into(),
            },
        ];
        let bytes = serialize_shortcuts(&shortcuts);
        let parsed = parse_shortcuts(&bytes).expect("parse");
        assert_eq!(parsed, shortcuts);
    }

    #[test]
    fn parse_empty_shortcuts() {
        let bytes = serialize_shortcuts(&[]);
        let parsed = parse_shortcuts(&bytes).expect("parse");
        assert!(parsed.is_empty());
    }
}
