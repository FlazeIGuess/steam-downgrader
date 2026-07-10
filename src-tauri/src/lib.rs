mod rollback;
mod steam;

use rollback::{AppliedInfo, RollbackEntry};

use steam::applier::{ApplyModel, ApplyResult};
use steam::auth::Session;
use steam::changelog::ChangelogEntry;
use steam::downloader::DownloadResult;
use steam::library::OwnedGame;
use steam::resolver::{DepotManifest, ResolvedVersion};
use tauri::AppHandle;

// --- Changelog (public, no login) -------------------------------------------

/// Update history of an app (public Steam news API).
#[tauri::command]
async fn fetch_changelog(app_id: u32, count: Option<u32>) -> Result<Vec<ChangelogEntry>, String> {
    steam::changelog::fetch(app_id, count.unwrap_or(20), &["steam_community_announcements"])
        .await
        .map_err(Into::into)
}

// --- Steam helper: auth / library / PICS ------------------------------------

/// Current depot / manifest info via PICS (anonymous).
#[tauri::command]
async fn steam_appinfo(app: AppHandle, app_id: u32) -> Result<serde_json::Value, String> {
    steam::sidecar::request(&app, "appinfo", serde_json::json!({ "app_id": app_id }))
        .await
        .map_err(Into::into)
}

/// Login with a Steam account (2FA via `steam-event` / `need_code`).
#[tauri::command]
async fn steam_login(app: AppHandle, username: String, password: String) -> Result<Session, String> {
    steam::auth::login(&app, &username, &password)
        .await
        .map_err(Into::into)
}

/// QR login: emits `qr_url` events, resolves once approved in the Steam app.
#[tauri::command]
async fn steam_login_qr(app: AppHandle) -> Result<Session, String> {
    steam::auth::login_qr(&app).await.map_err(Into::into)
}

/// Supply a requested 2FA code.
#[tauri::command]
async fn steam_provide_code(app: AppHandle, code: String) -> Result<(), String> {
    steam::auth::provide_code(&app, &code)
        .await
        .map_err(Into::into)
}

/// Games owned by the signed-in account.
#[tauri::command]
async fn steam_owned(app: AppHandle) -> Result<Vec<OwnedGame>, String> {
    steam::library::list_owned(&app).await.map_err(Into::into)
}

// --- Resolver ----------------------------------------------------------------

/// Resolve "the version before `before_ts`" (timestamp alignment; PICS + extra).
#[tauri::command]
async fn steam_resolve(
    app: AppHandle,
    app_id: u32,
    before_ts: i64,
    extra_history: Option<Vec<DepotManifest>>,
) -> Result<ResolvedVersion, String> {
    steam::resolver::resolve(&app, app_id, before_ts, extra_history.unwrap_or_default())
        .await
        .map_err(Into::into)
}

// --- Downloader --------------------------------------------------------------

/// Kept for compatibility; the engine is built in, so this is a no-op.
#[tauri::command]
async fn provision_depotdownloader(app: AppHandle) -> Result<String, String> {
    steam::downloader::provision(&app).await.map_err(Into::into)
}

/// Report whether the download engine is available.
#[tauri::command]
async fn depotdownloader_status() -> Result<bool, String> {
    Ok(steam::downloader::is_available())
}

/// Download a resolved version into an output directory via the embedded engine.
#[tauri::command]
async fn steam_download(
    app: AppHandle,
    version: ResolvedVersion,
    output_dir: String,
) -> Result<DownloadResult, String> {
    steam::downloader::download(&app, &version, output_dir.into())
        .await
        .map_err(Into::into)
}

// --- Applier -----------------------------------------------------------------

/// Apply the downgrade (model A = in-place freeze, B = separate copy + shortcut).
#[tauri::command]
async fn steam_apply(
    model: ApplyModel,
    app_id: u32,
    game_name: String,
    downloaded_dir: String,
    install_dir: String,
) -> Result<ApplyResult, String> {
    steam::applier::apply(
        model,
        app_id,
        &game_name,
        downloaded_dir.into(),
        install_dir.into(),
    )
    .await
    .map_err(Into::into)
}

/// Undo an applied downgrade (model A: restore the original; model B: remove the
/// non-Steam shortcut from Steam and delete the frozen copy).
#[tauri::command]
async fn steam_revert(
    model: ApplyModel,
    app_id: u32,
    game_name: String,
    applied_path: String,
) -> Result<(), String> {
    steam::applier::revert(model, app_id, &game_name, applied_path.into())
        .await
        .map_err(Into::into)
}

/// Find local Steam libraries and the install path of an app.
#[tauri::command]
async fn find_install_dir(app_id: u32) -> Result<Option<String>, String> {
    Ok(steam::applier::find_install_dir(app_id).map(|p| p.to_string_lossy().into_owned()))
}

/// Open a native folder picker so the user can select the install folder
/// manually (when auto-detection did not find it).
#[tauri::command]
async fn pick_folder(title: Option<String>) -> Result<Option<String>, String> {
    let title = title.unwrap_or_else(|| "Select the game's install folder".into());
    let res = tokio::task::spawn_blocking(move || {
        rfd::FileDialog::new().set_title(title).pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(res.map(|p| p.to_string_lossy().into_owned()))
}

/// List all locally installed Steam games (no login required).
#[tauri::command]
async fn list_installed_games() -> Result<Vec<steam::applier::InstalledGame>, String> {
    Ok(steam::applier::list_installed())
}

/// Known manifests (with creation date) from Steam's depotcache for the depots.
#[tauri::command]
async fn list_local_manifests(app: AppHandle, depot_ids: Vec<u32>) -> Result<serde_json::Value, String> {
    let dirs: Vec<String> = steam::applier::depotcache_dirs()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    steam::sidecar::request_timeout(
        &app,
        "local_manifests",
        serde_json::json!({ "dirs": dirs, "depots": depot_ids }),
        120,
    )
    .await
    .map_err(Into::into)
}

// --- Rollback library (persistent) ------------------------------------------

#[tauri::command]
fn rollback_list() -> Result<Vec<RollbackEntry>, String> {
    Ok(rollback::load())
}

#[tauri::command]
fn rollback_add(entry: RollbackEntry) -> Result<Vec<RollbackEntry>, String> {
    rollback::add(entry)
}

#[tauri::command]
fn rollback_remove(id: String, delete_files: bool) -> Result<Vec<RollbackEntry>, String> {
    rollback::remove(&id, delete_files)
}

#[tauri::command]
fn rollback_set_applied(id: String, applied: Option<AppliedInfo>) -> Result<Vec<RollbackEntry>, String> {
    rollback::set_applied(&id, applied)
}

/// Open a folder in the file explorer.
#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(&path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Open a URL in the default browser (e.g. a SteamDB depot page). This is only a
/// link to a public page; the app never fetches or scrapes it.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(&url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Launch the downloaded build directly (its main .exe) while Steam runs in the
/// background, so the user can play the old version.
#[tauri::command]
fn launch_build(dir: String) -> Result<String, String> {
    let path = std::path::PathBuf::from(&dir);
    let exe = steam::applier::guess_main_exe(&path)
        .ok_or_else(|| "No main .exe found in the folder".to_string())?;
    let workdir = exe.parent().map(|p| p.to_path_buf()).unwrap_or(path);
    std::process::Command::new(&exe)
        .current_dir(&workdir)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(exe.to_string_lossy().into_owned())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            fetch_changelog,
            steam_appinfo,
            steam_login,
            steam_login_qr,
            steam_provide_code,
            steam_owned,
            steam_resolve,
            provision_depotdownloader,
            depotdownloader_status,
            steam_download,
            steam_apply,
            steam_revert,
            find_install_dir,
            pick_folder,
            list_installed_games,
            list_local_manifests,
            rollback_list,
            rollback_add,
            rollback_remove,
            rollback_set_applied,
            open_folder,
            open_url,
            launch_build,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
