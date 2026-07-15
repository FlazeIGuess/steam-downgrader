//! Rust to .NET bridge: starts the `steam-helper` process and talks to it over
//! newline-delimited JSON (stdin/stdout). Responses are correlated by `id`;
//! unsolicited `event` messages (e.g. 2FA prompts) are forwarded to the frontend
//! as the Tauri event `steam-event`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{oneshot, Mutex, OnceCell};

use super::SteamError;

static SIDECAR: OnceCell<Sidecar> = OnceCell::const_new();

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

struct Sidecar {
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next_id: AtomicU64,
    _child: Child, // kept alive for the process lifetime
}

/// Locate the helper binary: env override, then the bundled resource (installed
/// app), then dev build output, then next to the app.
fn helper_path(app: &AppHandle) -> std::path::PathBuf {
    if let Ok(p) = std::env::var("STEAM_HELPER_PATH") {
        return p.into();
    }
    // Bundled next to the installed app (Tauri resource).
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("steam-helper.exe");
        if p.exists() {
            return p;
        }
    }
    let candidates = [
        "../steam-helper/bin/Release/net10.0/steam-helper.exe",
        "../steam-helper/bin/Debug/net10.0/steam-helper.exe",
        "steam-helper.exe",
    ];
    for c in candidates {
        let p = std::path::PathBuf::from(c);
        if p.exists() {
            return p;
        }
    }
    std::path::PathBuf::from(candidates[1])
}

async fn init(app: AppHandle) -> Result<Sidecar, SteamError> {
    let path = helper_path(&app);

    let mut cmd = Command::new(&path);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // steam-helper is a .NET console app. Spawned from the GUI (which has no
    // console of its own in release), Windows would otherwise pop up a console
    // window for it - very visible at first sign-in, and off-putting to users.
    // CREATE_NO_WINDOW starts it hidden; the stdin/stdout JSON pipes are unaffected.
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| {
        SteamError::Other(format!(
            "could not start steam-helper ({}): {e}",
            path.display()
        ))
    })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| SteamError::Other("helper has no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SteamError::Other("helper has no stdout".into()))?;

    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let pending_reader = pending.clone();

    // Reader task: responses go to `pending`, events go to the frontend.
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(val) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(id) = val.get("id").and_then(|v| v.as_u64()) {
                if let Some(tx) = pending_reader.lock().await.remove(&id) {
                    let _ = tx.send(val);
                }
            } else if val.get("event").is_some() {
                let _ = app.emit("steam-event", val);
            }
        }
    });

    Ok(Sidecar {
        stdin: Mutex::new(stdin),
        pending,
        next_id: AtomicU64::new(1),
        _child: child,
    })
}

async fn sidecar(app: &AppHandle) -> Result<&'static Sidecar, SteamError> {
    SIDECAR.get_or_try_init(|| init(app.clone())).await
}

/// Send a command to the helper and wait for the correlated response
/// (default 600s timeout, which covers login/2FA and PICS lookups).
pub async fn request(app: &AppHandle, cmd: &str, params: Value) -> Result<Value, SteamError> {
    request_timeout(app, cmd, params, 600).await
}

/// Like [`request`] but with an explicit timeout (e.g. long downloads).
pub async fn request_timeout(
    app: &AppHandle,
    cmd: &str,
    mut params: Value,
    timeout_secs: u64,
) -> Result<Value, SteamError> {
    let sc = sidecar(app).await?;
    let id = sc.next_id.fetch_add(1, Ordering::SeqCst);

    if !params.is_object() {
        params = json!({});
    }
    params["id"] = json!(id);
    params["cmd"] = json!(cmd);

    let (tx, rx) = oneshot::channel();
    sc.pending.lock().await.insert(id, tx);

    let line = format!("{}\n", serde_json::to_string(&params).unwrap());
    {
        let mut stdin = sc.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| SteamError::Other(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| SteamError::Other(e.to_string()))?;
    }

    let resp = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx)
        .await
        .map_err(|_| SteamError::Other(format!("timeout on command '{cmd}'")))?
        .map_err(|_| SteamError::Other("lost sidecar response".into()))?;

    if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(resp.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(SteamError::Other(
            resp.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        ))
    }
}
