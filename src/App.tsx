import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { QRCodeSVG } from "qrcode.react";
import "./App.css";

// --- Types (must match the Rust structs) ------------------------------------

interface DepotManifest {
  depot_id: number;
  manifest_id: string; // 64-bit -> string across IPC
  timestamp: number;
  build_id: number | null;
  source: string;
}
interface ResolvedVersion {
  app_id: number;
  target_timestamp: number;
  depots: DepotManifest[];
  source: string;
}
interface InstalledGame {
  app_id: number;
  name: string;
  install_dir: string;
  build_id: number | null;
  size_bytes: number | null;
  last_updated: number | null;
}
interface OwnedGame {
  app_id: number;
  name: string | null;
}
interface ApplyResult {
  model: string;
  backup_or_copy_path: string;
  launch_hint: string;
  warnings: string[];
}
interface DepotInfo {
  depot_id: number;
  manifest_id: string;
  name: string | null;
  oslist: string | null;
  osarch: string | null;
  language: string | null;
  dlcappid: string | null;
}
interface AppInfo {
  depots: DepotInfo[];
}
// Dated build timeline (matches steam::timeline in Rust). One build = one dated
// manifest of one depot; the current public build plus anything cached locally.
interface BuildEntry {
  manifest_id: string;
  timestamp: number;
  date_iso: string;
  is_current: boolean;
  source: string; // "pics" | "depotcache"
  patch_title: string | null;
}
interface DepotTimeline {
  depot_id: number;
  builds: BuildEntry[];
}
interface PatchEntry {
  title: string;
  date: number;
  date_iso: string;
  url: string;
}
interface BuildTimeline {
  app_id: number;
  current_build_id: number | null;
  current_ts: number;
  depots: DepotTimeline[];
  patches: PatchEntry[];
}
interface AppliedInfo { model: string; path: string; at: number; }
interface RollbackEntry {
  id: string;
  app_id: number;
  game_name: string;
  target_label: string;
  target_date: string;
  depots: { depot_id: number; manifest_id: string }[];
  download_dir: string;
  bytes: number;
  downloaded_at: number;
  applied: AppliedInfo | null;
}

type SteamEvent =
  | { event: "need_code"; kind: string }
  | { event: "qr_url"; url: string }
  | { event: "download_log"; line: string }
  | { event: "download_progress"; percent: number }
  | { event: "download_done"; line: string };

interface Selected {
  appId: number;
  name: string;
  installDir: string;
  buildId: number | null;
  installed: boolean;
}

// --- Helpers ----------------------------------------------------------------

function fmtSize(b: number | null): string {
  if (!b) return "";
  const gb = b / 1073741824;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.max(1, Math.round(b / 1048576))} MB`;
}
function fmtDate(unixSecs: number): string {
  return new Date(unixSecs * 1000).toISOString().slice(0, 10);
}
/** Accept Steam-console / DepotDownloader / bare-ID paste - the manifest ID is
 *  always the longest digit run (≈19-20 digits vs ≤7 for app/depot ids). */
function extractManifestId(text: string): string {
  const nums = text.match(/\d+/g);
  if (!nums) return "";
  return nums.reduce((a, b) => (b.length >= a.length ? b : a), "");
}
function steamdbUrl(depotId: number): string {
  return `https://steamdb.info/depot/${depotId}/manifests/`;
}
/** Strip characters Windows forbids in folder names. */
function safeFolder(s: string): string {
  return s.replace(/[<>:"/\|?*]/g, "").replace(/\s+/g, " ").trim() || "game";
}
function cap(s: string): string { return s.charAt(0).toUpperCase() + s.slice(1); }
function depotIsWindows(d: DepotInfo): boolean { return !d.oslist || d.oslist.includes("windows"); }
function depotIs64(d: DepotInfo): boolean { return !d.osarch || d.osarch === "64"; }
/** Human label from PICS metadata: name / OS / arch / language / DLC. */
function depotLabel(d: DepotInfo): string {
  const parts: string[] = [];
  if (d.name) parts.push(d.name);
  if (d.oslist) {
    const os = d.oslist.split(",").map((o) => o === "windows" ? "Windows" : o === "macos" ? "macOS" : o === "linux" ? "Linux" : o).join("/");
    parts.push(d.osarch ? `${os} ${d.osarch}-bit` : os);
  }
  if (d.language) parts.push(cap(d.language));
  if (d.dlcappid) parts.push(`DLC ${d.dlcappid}`);
  return parts.join(" · ") || "shared / all platforms";
}

/** The app's terminal `>_` logo (same artwork as the window/exe icon). */
function Logo({ size = 36 }: { size?: number }) {
  return (
    <svg className="logo-svg" width={size} height={size} viewBox="0 0 512 512" aria-hidden="true">
      <rect width="512" height="512" rx="72" fill="#1c1c1c" />
      <rect x="40.5" y="40.5" width="431" height="431" rx="40" fill="none" stroke="#454545" strokeWidth="3" />
      <polyline points="154,168 242,256 154,344" fill="none" stroke="#e3e3e3" strokeWidth="42" strokeLinecap="round" strokeLinejoin="round" />
      <rect x="268" y="312" width="110" height="34" rx="10" fill="#e3e3e3" />
    </svg>
  );
}

/** Render the simple release-note format (short headings + "- " bullet lists,
 *  with wrapped continuation lines) as structured JSX instead of raw text. */
function ReleaseNotes({ body }: { body: string }) {
  type Block =
    | { type: "heading"; text: string }
    | { type: "list"; items: string[] }
    | { type: "para"; text: string };
  const blocks: Block[] = [];
  let list: string[] | null = null;
  for (const raw of body.replace(/\r/g, "").split("\n")) {
    const line = raw.replace(/\s+$/, "");
    if (!line.trim()) { list = null; continue; }
    const bullet = line.match(/^\s*[-*]\s+(.*)$/);
    if (bullet) {
      if (!list) { list = []; blocks.push({ type: "list", items: list }); }
      list.push(bullet[1]);
    } else if (/^\s+\S/.test(raw) && list && list.length) {
      list[list.length - 1] += " " + line.trim(); // continuation of the last bullet
    } else {
      list = null;
      const t = line.trim();
      if (t.length <= 24 && !/[.:]$/.test(t)) blocks.push({ type: "heading", text: t });
      else blocks.push({ type: "para", text: t });
    }
  }
  return (
    <div className="rn">
      {blocks.map((b, i) =>
        b.type === "heading" ? (
          <div className="rn-h" key={i}>{b.text}</div>
        ) : b.type === "para" ? (
          <p className="rn-p" key={i}>{b.text}</p>
        ) : (
          <ul className="rn-list" key={i}>{b.items.map((it, j) => <li key={j}>{it}</li>)}</ul>
        )
      )}
    </div>
  );
}

// ============================================================================

function App() {
  // App version (read at runtime from the Tauri config).
  const [appVersion, setAppVersion] = useState("");

  // Docs / help
  const [docsOpen, setDocsOpen] = useState(false);
  const [docsSection, setDocsSection] = useState<string>("");

  // Custom confirm dialog (replaces the native window.confirm)
  const [confirmState, setConfirmState] = useState<
    { title: string; message: string; confirmLabel: string; danger: boolean; resolve: (v: boolean) => void } | null
  >(null);

  // Update watcher (custom UI over the Tauri updater plugin)
  const [update, setUpdate] = useState<Update | null>(null);
  const [updateOpen, setUpdateOpen] = useState(false);
  const [updateStage, setUpdateStage] = useState<"checking" | "available" | "downloading" | "installing" | "uptodate" | "error">("uptodate");
  const [updateProgress, setUpdateProgress] = useState(0);
  const [updateError, setUpdateError] = useState<string | null>(null);

  // Sidebar
  const [sidebarTab, setSidebarTab] = useState<"games" | "rollbacks">("games");
  const [installed, setInstalled] = useState<InstalledGame[]>([]);
  const [owned, setOwned] = useState<OwnedGame[]>([]);
  const [ownedLoading, setOwnedLoading] = useState(false);
  const [ownedError, setOwnedError] = useState<string | null>(null);
  const [installedOpen, setInstalledOpen] = useState(true);
  const [ownedOpen, setOwnedOpen] = useState(true);
  const [filter, setFilter] = useState("");
  const [scanning, setScanning] = useState(false);
  const [manualId, setManualId] = useState("");
  const [rollbacks, setRollbacks] = useState<RollbackEntry[]>([]);

  // Selected game + its depots
  const [selected, setSelected] = useState<Selected | null>(null);
  const [depots, setDepots] = useState<DepotInfo[]>([]);
  const [manifestInputs, setManifestInputs] = useState<Record<number, string>>({});
  const [showAllDepots, setShowAllDepots] = useState(false);
  const [depotsLoading, setDepotsLoading] = useState(false);
  const [versionLabel, setVersionLabel] = useState("");

  // In-app build timeline (pick a build by date) + UI state.
  const [timeline, setTimeline] = useState<BuildTimeline | null>(null);
  const [timelineLoading, setTimelineLoading] = useState(false);
  const [buildsShown, setBuildsShown] = useState<Record<number, boolean>>({});

  // 3-step download wizard
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [justId, setJustId] = useState<string | null>(null);

  // Auth
  const [account, setAccount] = useState<string | null>(null);
  const [showLogin, setShowLogin] = useState(false);
  const [authMode, setAuthMode] = useState<"qr" | "password">("qr");
  const [qrUrl, setQrUrl] = useState<string | null>(null);
  const [qrPending, setQrPending] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [need2fa, setNeed2fa] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [authBusy, setAuthBusy] = useState(false);

  // Download
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [downloadDir, setDownloadDir] = useState<string>(() => {
    try { return localStorage.getItem("dl_base") ?? ""; } catch { return ""; }
  });

  // Manage (per version)
  const [applyOpenId, setApplyOpenId] = useState<string | null>(null);
  const [applyResults, setApplyResults] = useState<Record<string, ApplyResult>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const consoleRef = useRef<HTMLDivElement>(null);

  // --- Effects --------------------------------------------------------------
  useEffect(() => {
    loadInstalled();
    refreshRollbacks();
    const un = listen<SteamEvent>("steam-event", (e) => {
      const p = e.payload;
      if (p.event === "need_code") setNeed2fa(p.kind);
      else if (p.event === "qr_url") setQrUrl(p.url);
      else if (p.event === "download_progress") setProgress(p.percent);
      else if (p.event === "download_log") setLog((l) => [...l.slice(-400), p.line]);
      else if (p.event === "download_done") setLog((l) => [...l, `done · ${p.line}`]);
    });
    return () => { un.then((f) => f()); };
  }, []);
  useEffect(() => { consoleRef.current?.scrollTo(0, consoleRef.current.scrollHeight); }, [log]);
  // Load the owned-games list once signed in (reveals not-installed games).
  useEffect(() => { if (account) loadOwned(); }, [account]);
  // Silently check for an app update on startup.
  useEffect(() => { checkForUpdate(false); }, []);
  // Read the running app version once (for the sidebar footer).
  useEffect(() => { getVersion().then(setAppVersion).catch(() => { /* ignore */ }); }, []);
  // Scroll the docs modal to a requested section.
  useEffect(() => {
    if (!docsOpen || !docsSection) return;
    const el = document.getElementById(`docs-${docsSection}`);
    if (el) setTimeout(() => el.scrollIntoView({ behavior: "smooth", block: "start" }), 60);
  }, [docsOpen, docsSection]);
  // Keyboard shortcuts for the confirm dialog: Enter confirms, Escape cancels.
  useEffect(() => {
    if (!confirmState) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") { e.preventDefault(); closeConfirm(true); }
      else if (e.key === "Escape") { e.preventDefault(); closeConfirm(false); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [confirmState]);

  // --- Data -----------------------------------------------------------------
  async function loadInstalled() {
    setScanning(true);
    try { setInstalled(await invoke<InstalledGame[]>("list_installed_games")); }
    catch (e) { setError(String(e)); }
    finally { setScanning(false); }
  }
  async function loadOwned() {
    setOwnedLoading(true); setOwnedError(null);
    try { setOwned(await invoke<OwnedGame[]>("steam_owned")); }
    catch (e) { setOwnedError(String(e)); }
    finally { setOwnedLoading(false); }
  }
  async function refreshRollbacks() {
    try { setRollbacks(await invoke<RollbackEntry[]>("rollback_list")); } catch { /* ignore */ }
  }

  function resetWizard() {
    setStep(1); setJustId(null);
    setManifestInputs({}); setVersionLabel("");
    setDownloading(false); setProgress(null); setLog([]);
  }

  async function selectGame(g: { appId: number; name: string; installDir?: string; buildId?: number | null; installed?: boolean }) {
    let installDir = g.installDir ?? "";
    let installed = g.installed ?? false;
    if (installDir) {
      installed = true;
    } else {
      try {
        const d = await invoke<string | null>("find_install_dir", { appId: g.appId });
        if (d) { installDir = d; installed = true; }
      } catch { /* not installed / not detected */ }
    }
    setSelected({ appId: g.appId, name: g.name, installDir, buildId: g.buildId ?? null, installed });
    setDepots([]); setShowAllDepots(false); setApplyOpenId(null); setError(null);
    resetWizard();
    loadDepots(g.appId);
  }

  async function loadDepots(appId: number) {
    setDepotsLoading(true);
    setTimeline(null); setBuildsShown({}); setTimelineLoading(true);
    let list: DepotInfo[] = [];
    try {
      const info = await invoke<AppInfo>("steam_appinfo", { appId });
      list = info.depots ?? [];
      setDepots(list);
      setManifestInputs({});
      // Default to Windows 64-bit only; reveal the rest via the toggle.
      setShowAllDepots(!list.some((d) => depotIsWindows(d) && depotIs64(d) && !d.dlcappid));
    } catch (e) { setError(String(e)); setDepots([]); }
    finally { setDepotsLoading(false); }
    // The Steam connection is warm now, so fetch the dated build timeline (non-
    // blocking, additive: the manual SteamDB paste path always stays available).
    invoke<BuildTimeline>("steam_build_timeline", { appId })
      .then((t) => setTimeline(t))
      .catch(() => setTimeline(null))
      .finally(() => setTimelineLoading(false));
  }

  function openManualFromLibrary(e: FormEvent) {
    e.preventDefault();
    const id = Number(manualId);
    if (id > 0) selectGame({ appId: id, name: `App ${id}` });
  }

  function setDepotManifest(depotId: number, raw: string) {
    setManifestInputs((m) => ({ ...m, [depotId]: extractManifestId(raw) }));
  }
  function openSteamdb(depotId: number) {
    invoke("open_url", { url: steamdbUrl(depotId) }).catch((e) => setError(String(e)));
  }

  // --- Build timeline (pick a build by date) --------------------------------
  function depotBuilds(depotId: number): BuildEntry[] {
    return timeline?.depots.find((d) => d.depot_id === depotId)?.builds ?? [];
  }
  /** Select a build for ONE depot: fills that depot's manifest (single value,
   *  so a depot can only ever have one build selected). Suggests a name. */
  function pickDepotBuild(depotId: number, b: BuildEntry) {
    setManifestInputs((m) => ({ ...m, [depotId]: b.manifest_id }));
    setVersionLabel((cur) => (cur.trim() ? cur : b.patch_title ? `before ${b.patch_title}` : `build ${b.date_iso}`));
  }
  /** Select the build right before the current one for the primary depot. */
  function restorePrimaryPrevious() {
    if (primaryDepot && primaryPrevBuild) pickDepotBuild(primaryDepot.depot_id, primaryPrevBuild);
  }
  function openFolder(path: string) { invoke("open_folder", { path }).catch((e) => setError(String(e))); }
  function openDocs(section?: string) { setDocsSection(section ?? ""); setDocsOpen(true); }
  /** Promise-based confirm dialog styled to match the app (replaces window.confirm). */
  function askConfirm(opts: { title?: string; message: string; confirmLabel?: string; danger?: boolean }): Promise<boolean> {
    return new Promise((resolve) => setConfirmState({
      title: opts.title ?? "confirm",
      message: opts.message,
      confirmLabel: opts.confirmLabel ?? "confirm",
      danger: opts.danger ?? false,
      resolve,
    }));
  }
  function closeConfirm(result: boolean) {
    confirmState?.resolve(result);
    setConfirmState(null);
  }

  // --- Update watcher -------------------------------------------------------
  /** Check the release endpoint. `manual` opens the dialog even when up to date. */
  async function checkForUpdate(manual: boolean) {
    setUpdateError(null);
    if (manual) { setUpdateStage("checking"); setUpdateOpen(true); }
    try {
      const u = await check();
      if (u) {
        setUpdate(u);
        setUpdateStage("available");
        setUpdateOpen(true);
      } else {
        setUpdate(null);
        setUpdateStage("uptodate");
      }
    } catch (e) {
      setUpdateError(String(e));
      setUpdateStage("error");
      // Only surface check failures when the user asked explicitly.
      if (!manual) setUpdateOpen(false);
    }
  }
  async function installUpdate() {
    if (!update) return;
    setUpdateError(null); setUpdateStage("downloading"); setUpdateProgress(0);
    try {
      let total = 0;
      let downloaded = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? 0;
        else if (e.event === "Progress") {
          downloaded += e.data.chunkLength;
          setUpdateProgress(total > 0 ? Math.min(100, (downloaded / total) * 100) : 0);
        } else if (e.event === "Finished") {
          setUpdateProgress(100);
          setUpdateStage("installing");
        }
      });
      // Installed. Relaunch into the new version.
      await relaunch();
    } catch (e) {
      setUpdateError(String(e));
      setUpdateStage("error");
    }
  }
  const updateBusy = updateStage === "downloading" || updateStage === "installing";
  /** A small "?" help badge: hover shows a tooltip, click opens the full docs. */
  const q = (tip: string, section?: string) => (
    <button type="button" className="q" data-tip={tip}
      onClick={() => openDocs(section)} aria-label="Help">
      <svg className="q-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor"
        strokeWidth="3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
        <path d="M12 17h.01" />
      </svg>
    </button>
  );

  /** Manually point at the install folder when auto-detection missed it. */
  async function locateInstall() {
    try {
      const p = await invoke<string | null>("pick_folder", { title: "Select the game's install folder" });
      if (p) setSelected((s) => (s ? { ...s, installDir: p, installed: true } : s));
    } catch (e) { setError(String(e)); }
  }
  /** Pick where downloaded builds are saved (remembered across sessions). */
  async function chooseDownloadDir() {
    try {
      const p = await invoke<string | null>("pick_folder", { title: "Choose where rollback downloads are saved" });
      if (p) { setDownloadDir(p); try { localStorage.setItem("dl_base", p); } catch { /* ignore */ } }
    } catch (e) { setError(String(e)); }
  }
  function clearDownloadDir() {
    setDownloadDir("");
    try { localStorage.removeItem("dl_base"); } catch { /* ignore */ }
  }

  // --- Auth -----------------------------------------------------------------
  function openLogin() { if (account) return; setShowLogin(true); setAuthMode("qr"); setError(null); startQr(); }
  function closeLogin() { setShowLogin(false); }
  async function startQr() {
    if (qrPending) return;
    setQrPending(true); setQrUrl(null); setError(null);
    try {
      const s = await invoke<{ account_name: string }>("steam_login_qr");
      setAccount(s.account_name || "steam user"); setShowLogin(false);
    } catch (e) { setError(String(e)); }
    finally { setQrPending(false); setQrUrl(null); }
  }
  async function doLogin() {
    setAuthBusy(true); setError(null);
    try {
      const s = await invoke<{ account_name: string }>("steam_login", { username, password });
      setAccount(s.account_name || username); setNeed2fa(null); setShowLogin(false);
    } catch (e) { setError(String(e)); }
    finally { setAuthBusy(false); }
  }
  async function submitCode() {
    try { await invoke("steam_provide_code", { code }); setNeed2fa(null); setCode(""); }
    catch (e) { setError(String(e)); }
  }

  // --- Download a version (3-step wizard) -----------------------------------
  function filledDepots(): DepotManifest[] {
    return depots
      .filter((d) => /^\d+$/.test(manifestInputs[d.depot_id] ?? ""))
      .map((d) => ({ depot_id: d.depot_id, manifest_id: manifestInputs[d.depot_id], timestamp: 0, build_id: null, source: "Manual" }));
  }
  function pickedDate(): string {
    return fmtDate(Math.floor(Date.now() / 1000));
  }
  async function startDownload() {
    if (!selected) return;
    const picked = filledDepots();
    if (picked.length === 0) { setError("Paste a manifest for at least one depot."); return; }

    const version: ResolvedVersion = { app_id: selected.appId, target_timestamp: 0, depots: picked, source: "Manual" };
    const id = `${selected.appId}|${picked.map((d) => `${d.depot_id}:${d.manifest_id}`).sort().join(",")}`;
    const suffix = picked[0].manifest_id.slice(-6);
    const dir = downloadDir
      ? `${downloadDir.replace(/[\\/]+$/, "")}\\${safeFolder(selected.name || `app_${selected.appId}`)}_rb_${suffix}`
      : `${selected.installDir || "app_" + selected.appId}_rb_${suffix}`;
    const date = pickedDate();
    const label = versionLabel.trim() || `version ${date}`;

    setError(null); setStep(2); setDownloading(true); setProgress(0); setLog([`Downloading to ${dir}`]);
    try {
      const res = await invoke<{ output_dir: string; bytes_total: number }>("steam_download", { version, outputDir: dir });
      setProgress(100);
      const entry: RollbackEntry = {
        id, app_id: selected.appId, game_name: selected.name,
        target_label: label, target_date: date,
        depots: picked.map((d) => ({ depot_id: d.depot_id, manifest_id: d.manifest_id })),
        download_dir: res.output_dir ?? dir, bytes: res.bytes_total ?? 0,
        downloaded_at: Math.floor(Date.now() / 1000), applied: null,
      };
      setRollbacks(await invoke<RollbackEntry[]>("rollback_add", { entry }));
      setJustId(id); setStep(3);
      setLog((l) => [...l, `✓ saved to your versions: ${label}`]);
    }
    catch (e) { setError(String(e)); setStep(1); }
    finally { setDownloading(false); }
  }

  // --- Manage versions ------------------------------------------------------
  async function launch(v: RollbackEntry) {
    setBusyId(v.id); setError(null);
    try { await invoke<string>("launch_build", { dir: v.download_dir }); }
    catch (e) { setError(String(e)); }
    finally { setBusyId(null); }
  }
  async function applyVersion(v: RollbackEntry, model: "InPlaceFreeze" | "SeparateCopyShortcut") {
    if (!selected) return;
    setBusyId(v.id); setError(null);
    try {
      // If this version is already applied (with either method), undo that first,
      // so switching methods just works and no leftover backup blocks the re-apply.
      if (v.applied) {
        await revertApplied(v);
        setApplyResults((m) => { const n = { ...m }; delete n[v.id]; return n; });
      }
      const r = await invoke<ApplyResult>("steam_apply", {
        model, appId: v.app_id, gameName: v.game_name, downloadedDir: v.download_dir, installDir: selected.installDir,
      });
      setApplyResults((m) => ({ ...m, [v.id]: r }));
      const applied: AppliedInfo = { model, path: r.backup_or_copy_path, at: Math.floor(Date.now() / 1000) };
      setRollbacks(await invoke<RollbackEntry[]>("rollback_set_applied", { id: v.id, applied }));
    } catch (e) { setError(String(e)); }
    finally { setBusyId(null); }
  }
  /** Undo an applied downgrade (both models). Uses the stored applied model+path. */
  async function revertApplied(v: RollbackEntry): Promise<void> {
    if (!v.applied) return;
    await invoke("steam_revert", {
      model: v.applied.model, appId: v.app_id, gameName: v.game_name, appliedPath: v.applied.path,
    });
  }
  async function revertVersion(v: RollbackEntry) {
    if (!v.applied) return;
    setBusyId(v.id); setError(null);
    try {
      await revertApplied(v);
      setApplyResults((m) => { const n = { ...m }; delete n[v.id]; return n; });
      setRollbacks(await invoke<RollbackEntry[]>("rollback_set_applied", { id: v.id, applied: null }));
    } catch (e) { setError(String(e)); }
    finally { setBusyId(null); }
  }
  async function removeRollback(v: RollbackEntry) {
    const undoNote = v.applied
      ? v.applied.model === "SeparateCopyShortcut"
        ? " This also removes the non-Steam shortcut from Steam and deletes the frozen copy."
        : " This also restores your current install and re-enables Steam updates."
      : "";
    const ok = await askConfirm({
      title: "delete version",
      message: `Delete "${v.target_label}" (${v.game_name}) and its downloaded files?${undoNote}`,
      confirmLabel: "delete",
      danger: true,
    });
    if (!ok) return;
    setBusyId(v.id); setError(null);
    try {
      // Undo the apply first so the game is also removed from Steam.
      if (v.applied) await revertApplied(v);
      setRollbacks(await invoke<RollbackEntry[]>("rollback_remove", { id: v.id, deleteFiles: true }));
      setApplyResults((m) => { const n = { ...m }; delete n[v.id]; return n; });
    } catch (e) { setError(String(e)); }
    finally { setBusyId(null); }
  }

  // --- Derived --------------------------------------------------------------
  const installedList = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return installed
      .filter((g) => !q || g.name.toLowerCase().includes(q))
      .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  }, [installed, filter]);

  const installedIds = useMemo(() => new Set(installed.map((g) => g.app_id)), [installed]);

  const ownedNotInstalled = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return owned
      .filter((o) => !installedIds.has(o.app_id))
      .map((o) => ({ app_id: o.app_id, name: o.name ?? `App ${o.app_id}` }))
      .filter((g) => !q || g.name.toLowerCase().includes(q))
      .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  }, [owned, installedIds, filter]);

  const rollbackGames = useMemo(() => {
    const map = new Map<number, { app_id: number; name: string; count: number }>();
    for (const r of rollbacks) {
      const cur = map.get(r.app_id);
      if (cur) cur.count++;
      else map.set(r.app_id, { app_id: r.app_id, name: r.game_name, count: 1 });
    }
    return [...map.values()].sort((a, b) => a.name.localeCompare(b.name));
  }, [rollbacks]);

  const gameVersions = useMemo(
    () => (selected ? rollbacks.filter((r) => r.app_id === selected.appId) : []),
    [rollbacks, selected]);

  const canDownload = useMemo(
    () => depots.some((d) => /^\d+$/.test(manifestInputs[d.depot_id] ?? "")),
    [depots, manifestInputs]);

  // The depot that matches this machine: the main Windows 64-bit content depot.
  // Shown directly with its own manifest field + date picker.
  const primaryDepot = useMemo<DepotInfo | null>(() => {
    const content = depots.filter((d) => !d.dlcappid);
    return (
      content.find((d) => d.oslist?.includes("windows") && d.osarch === "64") ||
      content.find((d) => d.oslist?.includes("windows")) ||
      content.find((d) => !d.oslist) || // shared / all-platforms
      content[0] || depots[0] || null
    );
  }, [depots]);

  // Every other depot (extra content, DLC, other OS); revealed by "show all depots".
  const otherDepots = useMemo(
    () => depots.filter((d) => d.depot_id !== primaryDepot?.depot_id),
    [depots, primaryDepot]);

  // The newest build before the current one, for the primary depot (restore CTA).
  const primaryPrevBuild = useMemo<BuildEntry | null>(() => {
    if (!timeline || !primaryDepot) return null;
    const builds = timeline.depots.find((t) => t.depot_id === primaryDepot.depot_id)?.builds ?? [];
    return builds.find((b) => !b.is_current && b.timestamp < timeline.current_ts) ?? null;
  }, [timeline, primaryDepot]);

  const justEntry = useMemo(() => rollbacks.find((r) => r.id === justId) ?? null, [rollbacks, justId]);
  const canApply = !!(selected?.installed && selected.installDir);

  // --- Reusable: one depot's manifest field + dated build picker -------------
  function renderDepotPicker(d: DepotInfo) {
    const input = manifestInputs[d.depot_id] ?? "";
    const filled = /^\d+$/.test(input);
    const builds = depotBuilds(d.depot_id);
    const LIMIT = 6;
    const showAll = buildsShown[d.depot_id] ?? false;
    const shown = showAll ? builds : builds.slice(0, LIMIT);
    return (
      <div className="depot-pick">
        <div className="depot-head">
          <span className="depot-id">{depotLabel(d)} <span className="depot-num">· depot {d.depot_id}</span></span>
          <button className="btn btn--depot btn--sm" onClick={() => openSteamdb(d.depot_id)}>Open depot {d.depot_id} on SteamDB ↗</button>
        </div>
        {/* the manifest that will be downloaded for this depot - always visible */}
        <div className="depot-paste">
          <input className="field" placeholder="paste a manifest, or pick a build below - Steam console / DepotDownloader / plain ID"
            value={input} onChange={(e) => setDepotManifest(d.depot_id, e.target.value)} />
          {filled && <span className="depot-ok" title="Manifest detected">✓</span>}
        </div>
        {timelineLoading && builds.length === 0 ? (
          <p className="pick-loading">reading your build history…</p>
        ) : builds.length > 0 ? (
          <div className="picker">
            <div className="pick-head">
              <span className="pick-head-title">or pick a build by date</span>
              <span className="pick-head-sub">current + cached on this PC</span>
            </div>
            <ul className="builds">
              {shown.map((b) => {
                const sel = input === b.manifest_id;
                return (
                  <li key={b.manifest_id}
                    className={`build${b.is_current ? " build--current" : ""}${sel ? " build--sel" : ""}`}>
                    <span className="build-dot" />
                    <div className="build-info">
                      <div className="build-line">
                        <span className="build-date">{b.date_iso}</span>
                        {b.is_current && <span className="build-tag">current</span>}
                      </div>
                      {b.patch_title && !b.is_current && (
                        <div className="build-patch">the build before &ldquo;<em>{b.patch_title}</em>&rdquo;</div>
                      )}
                    </div>
                    {b.is_current ? (
                      <span className="build-here">you are here</span>
                    ) : (
                      <button className={`btn btn--sm${sel ? "" : " btn--depot"}`} onClick={() => pickDepotBuild(d.depot_id, b)}>
                        {sel ? "✓ selected" : "use this build"}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
            {builds.length > LIMIT && (
              <button className="pick-more" onClick={() => setBuildsShown((s) => ({ ...s, [d.depot_id]: !showAll }))}>
                {showAll ? "show fewer" : `show all ${builds.length} builds`}
              </button>
            )}
          </div>
        ) : (
          <p className="pick-empty">No older builds are cached on this PC. Paste a manifest from SteamDB above.</p>
        )}
      </div>
    );
  }

  // --- Reusable: the two apply models for one version -----------------------
  function renderApplyModels(v: RollbackEntry) {
    return (
      <div className="ver-apply">
        <div className="models">
          <div className="model model--rec">
            <h4>separate copy <span className="badge">recommended</span>
              {q("Copies the build into a separate _frozen folder and adds it to Steam as a non-Steam shortcut. Your real install stays untouched and current, so online play keeps working.", "apply-separate")}
            </h4>
            <p>Keeps a frozen copy outside Steam and adds a non-Steam shortcut. Your original stays current for online play.</p>
            <button className="btn btn--primary btn--sm" onClick={() => applyVersion(v, "SeparateCopyShortcut")} disabled={!canApply || busyId === v.id}>use separate copy</button>
          </div>
          <div className="model">
            <h4>in-place freeze
              {q("Moves your current install to a .downgrader-backup folder, drops the old build in its place, and disables auto-update in the ACF. Undo restores it. Online games may still need Steam Offline Mode.", "apply-inplace")}
            </h4>
            <p>Overwrites the install and disables auto-update. Online, Steam may still need Offline Mode to hold the version.</p>
            <button className="btn btn--sm" onClick={() => applyVersion(v, "InPlaceFreeze")} disabled={!canApply || busyId === v.id}>overwrite in place</button>
          </div>
        </div>
        {!canApply && (
          <p className="hint">Install path not detected - apply needs it. Use <strong>locate…</strong> in the header if this game is actually installed. Otherwise just <strong>launch</strong> the downloaded build.</p>
        )}
        {applyResults[v.id] && (
          <div className="result">
            <p><strong>applied - {applyResults[v.id].model === "SeparateCopyShortcut" ? "separate copy" : "in-place freeze"}.</strong></p>
            {applyResults[v.id].launch_hint && <p>{applyResults[v.id].launch_hint}</p>}
            <p><code>{applyResults[v.id].backup_or_copy_path}</code></p>
            {applyResults[v.id].warnings.length > 0 && <ul className="warn-list">{applyResults[v.id].warnings.map((w, i) => <li key={i}>{w}</li>)}</ul>}
            <button className="btn btn--danger btn--sm" onClick={() => revertVersion(v)} style={{ marginTop: 12 }}>undo this apply</button>
          </div>
        )}
      </div>
    );
  }

  // --- Render ---------------------------------------------------------------
  return (
    <div className="app">
      {/* ---------- Sidebar ---------- */}
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark"><Logo size={38} /></div>
          <div>
            <div className="brand-name">STEAM DOWNGRADER</div>
            <div className="brand-sub">roll a game back to an earlier build</div>
          </div>
        </div>

        <button className={`account ${account ? "account--in" : ""}`} onClick={openLogin}>
          <span className={`dot ${account ? "" : "dot--off"}`} />
          {account ? <>signed in <small>{account}</small></> : "sign in to steam"}
        </button>

        <button className="docs-btn" onClick={() => openDocs()}>
          <span className="docs-btn-mark">?</span> docs &amp; help - how it works
        </button>

        {updateStage === "available" && (
          <button className="update-chip" onClick={() => setUpdateOpen(true)}>
            <span className="update-dot" /> update available{update ? ` · v${update.version}` : ""}
          </button>
        )}

        <div className="tabs">
          <button className={`tab ${sidebarTab === "games" ? "tab--active" : ""}`} onClick={() => setSidebarTab("games")}>games</button>
          <button className={`tab ${sidebarTab === "rollbacks" ? "tab--active" : ""}`} onClick={() => setSidebarTab("rollbacks")}>
            rollbacks{rollbacks.length ? ` (${rollbacks.length})` : ""}
          </button>
        </div>

        {sidebarTab === "games" ? (
          <div className="lib">
            <div className="lib-head">
              <span>your games</span>
              <button className="icon-btn" title="Rescan" onClick={() => { loadInstalled(); if (account) loadOwned(); }}>{scanning || ownedLoading ? "…" : "↻"}</button>
            </div>
            <input className="lib-search" placeholder="filter games…" value={filter}
              onChange={(e) => setFilter(e.currentTarget.value)} />
            <div className="lib-list">
              {/* Installed group */}
              <button className="lib-group" onClick={() => setInstalledOpen((o) => !o)}>
                <span><span className="lib-caret">{installedOpen ? "▾" : "▸"}</span> installed</span>
                <span className="lib-group-count">{installedList.length}</span>
              </button>
              {installedOpen && (
                <>
                  {installedList.map((g) => (
                    <button key={g.app_id}
                      className={`game-row ${selected?.appId === g.app_id ? "game-row--active" : ""}`}
                      onClick={() => selectGame({ appId: g.app_id, name: g.name, installDir: g.install_dir, buildId: g.build_id, installed: true })}>
                      <span className="game-name">{g.name}</span>
                      <span className="game-meta">
                        {g.build_id != null && <span>build {g.build_id}</span>}
                        {g.size_bytes != null && <span>{fmtSize(g.size_bytes)}</span>}
                      </span>
                    </button>
                  ))}
                  {!scanning && installedList.length === 0 && (
                    <div className="lib-empty">
                      {filter ? `No installed game matches “${filter}”.` : "No installed games found. Sign in to download ones you own, or use the App ID box below."}
                    </div>
                  )}
                </>
              )}

              {/* Owned-but-not-installed group */}
              {account ? (
                <>
                  <button className="lib-group" onClick={() => setOwnedOpen((o) => !o)}>
                    <span><span className="lib-caret">{ownedOpen ? "▾" : "▸"}</span> owned · not installed</span>
                    <span className="lib-group-count">{ownedLoading ? "…" : ownedNotInstalled.length}</span>
                  </button>
                  {ownedOpen && (
                    <>
                      {ownedLoading ? (
                        <div className="lib-empty">loading your Steam library…</div>
                      ) : ownedError ? (
                        <div className="lib-empty">
                          Couldn't load your owned games: {ownedError}
                          <div style={{ marginTop: 8 }}><button className="btn btn--sm" onClick={loadOwned}>retry</button></div>
                        </div>
                      ) : (
                        <>
                          {ownedNotInstalled.map((g) => (
                            <button key={g.app_id}
                              className={`game-row ${selected?.appId === g.app_id ? "game-row--active" : ""}`}
                              onClick={() => selectGame({ appId: g.app_id, name: g.name, installed: false })}>
                              <span className="game-name">{g.name}</span>
                              <span className="game-meta"><span className="game-tag">download only</span></span>
                            </button>
                          ))}
                          {ownedNotInstalled.length === 0 && (
                            <div className="lib-empty">{filter ? "Nothing else matches." : owned.length === 0 ? "No owned games came back from Steam. Try retry, or check the account." : "Every game you own is already installed."}</div>
                          )}
                        </>
                      )}
                    </>
                  )}
                </>
              ) : (
                <div className="lib-empty">Sign in to Steam to browse and download builds of games you own but haven't installed.</div>
              )}
            </div>
            <form className="manual" onSubmit={openManualFromLibrary}>
              <input className="field" placeholder="…or App ID" inputMode="numeric"
                value={manualId} onChange={(e) => setManualId(e.currentTarget.value)} />
              <button className="btn btn--sm" type="submit">open</button>
            </form>
          </div>
        ) : (
          <div className="lib">
            <div className="lib-head"><span>my rollbacks</span></div>
            <div className="lib-list">
              {rollbackGames.length === 0 && (
                <div className="lib-empty">No saved rollbacks yet. Pick a game, download an old build - it's remembered here across restarts.</div>
              )}
              {rollbackGames.map((g) => (
                <button key={g.app_id}
                  className={`game-row ${selected?.appId === g.app_id ? "game-row--active" : ""}`}
                  onClick={() => selectGame({ appId: g.app_id, name: g.name })}>
                  <span className="game-name">{g.name}</span>
                  <span className="game-meta"><span>{g.count} version{g.count > 1 ? "s" : ""}</span></span>
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Shared footer: visible under both tabs */}
        <div className="sidebar-footer">
          <button className="coffee-btn"
            onClick={() => invoke("open_url", { url: "https://ko-fi.com/flazeiguess" }).catch((e) => setError(String(e)))}>
            <svg className="coffee-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor"
              strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M17 8h1a4 4 0 1 1 0 8h-1" />
              <path d="M3 8h14v9a4 4 0 0 1-4 4H7a4 4 0 0 1-4-4Z" />
              <line x1="6" x2="6" y1="2" y2="4" />
              <line x1="10" x2="10" y1="2" y2="4" />
              <line x1="14" x2="14" y1="2" y2="4" />
            </svg>
            buy me a coffee
          </button>
          {appVersion && <div className="app-version">v{appVersion}</div>}
        </div>
      </aside>

      {/* ---------- Workspace ---------- */}
      <main className="workspace">
        {error && <div className="banner">{error}</div>}

        {!selected ? (
          <div className="ws-empty">
            <div className="ws-empty-mark"><Logo size={72} /></div>
            <h2>pick a game to roll back</h2>
            <p>
              Choose a game on the left - <strong>installed</strong> ones can be rolled back in place, games you only
              <strong> own</strong> can be downloaded and launched directly. Find the build you want by date on <strong>SteamDB</strong>,
              paste its manifest here, and download. Every download is remembered in <strong>rollbacks</strong>.
            </p>
          </div>
        ) : (
          <>
            <header className="ws-header">
              <h1 className="ws-title">{selected.name}</h1>
              <div className="ws-meta">
                <span className="meta-item">app id <code>{selected.appId}</code></span>
                {selected.buildId != null && <span className="meta-item">current build <code>{selected.buildId}</code></span>}
                <span className="meta-item">
                  {selected.installed ? "install" : "status"}{" "}
                  <code>{selected.installDir || "not installed"}</code>
                  <button className="meta-link" onClick={locateInstall}>{selected.installDir ? "change…" : "locate…"}</button>
                </span>
              </div>
            </header>

            <div className="ws-body">
              {/* Existing rollback versions for this game */}
              {gameVersions.length > 0 && (
                <section>
                  <p className="section-label">your rollback versions {q("Every build you've downloaded. Launch it, apply it to your install, open its folder, or delete it (which also undoes an applied downgrade).", "manage")}</p>
                  {!canApply && (
                    <p className="hint" style={{ marginTop: -8, marginBottom: 16 }}>
                      This game isn't installed - <strong>launch</strong> runs the downloaded build directly. Use <strong>locate…</strong> above if it is installed and you want to apply.
                    </p>
                  )}
                  <div className="ver-list">
                    {gameVersions.map((v) => (
                      <div className="ver-row" key={v.id}>
                        <div className="ver-top">
                          <div className="ver-info">
                            <span className="ver-label">{v.target_label}</span>
                            <span className="ver-meta">
                              <span>{v.target_date}</span>
                              {v.bytes ? <span>{fmtSize(v.bytes)}</span> : null}
                              {v.applied && <span className="rb-applied">{v.applied.model === "SeparateCopyShortcut" ? "applied · copy" : "applied · in-place"}</span>}
                            </span>
                          </div>
                          <div className="ver-actions">
                            <button className="btn btn--primary btn--sm" onClick={() => launch(v)} disabled={busyId === v.id}>▶ launch</button>
                            {canApply && <button className="btn btn--sm" onClick={() => setApplyOpenId(applyOpenId === v.id ? null : v.id)}>apply ▾</button>}
                            <button className="icon-btn" title="Open folder" aria-label="Open folder" onClick={() => openFolder(v.download_dir)}>
                              <svg className="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                                <path d="M3 6a1 1 0 0 1 1-1h4.5l2 2H20a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6z" />
                              </svg>
                            </button>
                            <button className="icon-btn" title="Delete" onClick={() => removeRollback(v)} disabled={busyId === v.id}>✕</button>
                          </div>
                        </div>
                        {canApply && applyOpenId === v.id && renderApplyModels(v)}
                      </div>
                    ))}
                  </div>
                </section>
              )}

              {/* Add a version - 3-step wizard */}
              <section className="add-panel">
                <p className="section-label">add a version {q("Pick a build's manifest from SteamDB, download it, then apply it (installed games) or launch it directly.", "roll-back")}</p>

                <ol className="stepper">
                  <li className={`st ${step === 1 ? "st--on" : "st--done"}`}><span className="st-n">1</span> choose build</li>
                  <li className={`st ${step === 2 ? "st--on" : step > 2 ? "st--done" : ""}`}><span className="st-n">2</span> download</li>
                  <li className={`st ${step === 3 ? "st--on" : ""}`}><span className="st-n">3</span> {selected.installed ? "apply" : "play"}</li>
                </ol>

                {/* STEP 1 - choose the build */}
                {step === 1 && (
                  <>
                    <p className="pick-intro">
                      Pick an older build <strong>by date</strong> for your game's depot below, or paste a
                      manifest from SteamDB. The current build and any builds cached on your PC are listed
                      automatically - no manifest codes needed.
                      {q("Steam Downgrader lists the builds your PC already knows about (the current one from Steam, older ones from Steam's local depotcache) by date, for the depot that matches your machine. Builds that were never cached aren't listed - for those, open the depot on SteamDB and paste a manifest. Each depot is downloaded independently: only the depots you fill in are fetched.", "build-picker")}
                    </p>

                    {/* One-click: go back one build (the version before the current update) */}
                    {primaryDepot && primaryPrevBuild && (
                      <button className="restore-cta" onClick={restorePrimaryPrevious}>
                        <svg className="restore-cta-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                          strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                          <polyline points="11 19 4 12 11 5" />
                          <polyline points="20 19 13 12 20 5" />
                        </svg>
                        <span className="restore-cta-text">
                          <strong>Go back one build</strong>
                          <span>Select the build from {primaryPrevBuild.date_iso} - the version right before the current update.</span>
                        </span>
                        <span className="restore-cta-go">{manifestInputs[primaryDepot.depot_id] === primaryPrevBuild.manifest_id ? "selected ✓" : "select →"}</span>
                      </button>
                    )}

                    {depotsLoading ? (
                      <p className="hint">loading depots…</p>
                    ) : depots.length === 0 ? (
                      <p className="hint">No depots found for this app.</p>
                    ) : !primaryDepot ? (
                      <p className="hint">No downloadable depot found for this app.</p>
                    ) : (
                      <>
                        {/* Primary depot: the one that matches this machine, shown directly */}
                        {renderDepotPicker(primaryDepot)}

                        {/* Other depots (extra content, DLC, other OS) - shown inline when expanded */}
                        {showAllDepots && otherDepots.map((d) => (
                          <div className="adv-depot" key={d.depot_id}>{renderDepotPicker(d)}</div>
                        ))}
                        {otherDepots.length > 0 && (
                          <button className="load-more" onClick={() => setShowAllDepots((v) => !v)}>
                            {showAllDepots ? "show only the main depot" : `show all depots (${otherDepots.length} more) - extra content, DLC, other OS`}
                          </button>
                        )}

                        <div className="dl-loc">
                          <span className="dl-loc-label">save to {q("Where the downloaded build is stored. Default is a folder next to the game's install. Pick your own (e.g. D:\\Rollbacks) - it's remembered.", "download-location")}</span>
                          <code className="dl-loc-path" title={downloadDir || undefined}>
                            {downloadDir ? `${downloadDir}\\${safeFolder(selected.name)}_rb_…` : "default - next to the game's install folder"}
                          </code>
                          <button className="btn btn--sm" onClick={chooseDownloadDir}>choose…</button>
                          {downloadDir && <button className="btn btn--sm" onClick={clearDownloadDir}>reset</button>}
                        </div>

                        <div className="add-controls">
                          <input className="field" placeholder="version name (optional, e.g. v1.4 pre-nerf)"
                            value={versionLabel} onChange={(e) => setVersionLabel(e.currentTarget.value)} />
                          {account ? (
                            <button className="btn btn--primary" onClick={startDownload} disabled={!canDownload || downloading}>
                              {downloading ? "downloading…" : "download this version →"}
                            </button>
                          ) : (
                            <button className="btn btn--primary" onClick={openLogin}>sign in to download</button>
                          )}
                        </div>
                      </>
                    )}
                  </>
                )}

                {/* STEP 2 - download */}
                {step === 2 && (
                  <>
                    <p className="hint">Downloading the selected build with your Steam login. Large games can take a while - progress is live below.</p>
                    {progress != null && (
                      <div className="progress">
                        <div className="progress-bar"><span style={{ width: `${Math.min(100, progress)}%` }} /></div>
                        <div className="progress-pct">{progress.toFixed(1)}%</div>
                      </div>
                    )}
                    {log.length > 0 && (
                      <div className="console" ref={consoleRef}>
                        {log.map((l, i) => <div className="console-row" key={i}>{l}</div>)}
                      </div>
                    )}
                  </>
                )}

                {/* STEP 3 - apply / play */}
                {step === 3 && (
                  <>
                    <div className="result" style={{ marginTop: 0 }}>
                      <p><strong>downloaded ✓ {justEntry?.target_label}</strong></p>
                      <p style={{ margin: 0 }}>Saved to <strong>your rollback versions</strong> above - launch or manage it anytime.</p>
                    </div>

                    {selected.installed && justEntry ? (
                      <>
                        <p className="hint" style={{ marginTop: 16 }}>Apply it to your installed copy, or just launch this build directly:</p>
                        {renderApplyModels(justEntry)}
                        {justEntry && (
                          <button className="btn" style={{ marginTop: 12 }} onClick={() => launch(justEntry)} disabled={busyId === justEntry.id}>▶ launch this build directly</button>
                        )}
                      </>
                    ) : (
                      <div style={{ marginTop: 16 }}>
                        <p className="hint">This game isn't installed, so there's nothing to overwrite - <strong>apply doesn't apply here</strong>. Launch the downloaded build directly:</p>
                        {justEntry && (
                          <button className="btn btn--primary" onClick={() => launch(justEntry)} disabled={busyId === justEntry.id}>▶ launch this build</button>
                        )}
                        <p className="hint" style={{ marginTop: 12 }}>Installed after all? Use <strong>locate…</strong> in the header, then apply from <strong>your rollback versions</strong> above.</p>
                      </div>
                    )}

                    <button className="load-more" style={{ marginTop: 20 }} onClick={resetWizard}>+ download another version</button>
                  </>
                )}
              </section>
            </div>
          </>
        )}
      </main>

      {/* ---------- Docs / help modal ---------- */}
      {docsOpen && (
        <div className="modal-overlay" onClick={() => setDocsOpen(false)}>
          <div className="modal modal--docs" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>docs &amp; help</h3>
              <button className="close-x" onClick={() => setDocsOpen(false)}>×</button>
            </div>
            <div className="docs">
              <nav className="docs-toc">
                {[
                  ["overview", "What this is"],
                  ["roll-back", "Roll a game back"],
                  ["build-picker", "Pick a build by date"],
                  ["manifest", "Manifest from SteamDB"],
                  ["depots", "Depots explained"],
                  ["apply-separate", "Apply · separate copy"],
                  ["apply-inplace", "Apply · in-place freeze"],
                  ["manage", "Launch & manage"],
                  ["delete-undo", "Delete & undo"],
                  ["download-location", "Download location"],
                  ["owned-installed", "Owned vs installed"],
                  ["sign-in", "Signing in"],
                  ["data-security", "Data & security"],
                  ["troubleshooting", "Troubleshooting"],
                ].map(([key, label]) => (
                  <button key={key} className="docs-toc-link"
                    onClick={() => document.getElementById(`docs-${key}`)?.scrollIntoView({ behavior: "smooth", block: "start" })}>
                    {label}
                  </button>
                ))}
                <button className="docs-toc-link docs-toc-action"
                  onClick={() => { setDocsOpen(false); checkForUpdate(true); }}>
                  check for updates
                </button>
              </nav>

              <div className="docs-content">
                <section id="docs-overview" className="docs-sec">
                  <h4>What this is</h4>
                  <p>Steam Downgrader rolls a game back to an <strong>earlier build</strong> and lets you play that old
                    version - even though Steam only ever installs the latest. It downloads a past build straight from
                    Steam's servers using <strong>your own login</strong> (only for games you own), then either swaps it
                    into your install or keeps it as a separate, launchable copy.</p>
                </section>

                <section id="docs-roll-back" className="docs-sec">
                  <h4>Roll a game back - the 3 steps</h4>
                  <ol className="docs-list">
                    <li><strong>Pick a game</strong> on the left. Installed games can be downgraded in place; games you
                      only own are download-and-launch only.</li>
                    <li><strong>Choose the build.</strong> Pick one by date from the in-app list, or paste a manifest from SteamDB (see below).</li>
                    <li><strong>Download</strong>, then <strong>Apply</strong> (installed) or <strong>Launch</strong>{" "}
                      (not installed). Every download is saved under <em>Rollbacks</em>.</li>
                  </ol>
                </section>

                <section id="docs-build-picker" className="docs-sec">
                  <h4>Pick a build by date</h4>
                  <p>In step 1 the app shows a <strong>dated list of builds</strong> for each depot, so you can
                    usually skip SteamDB entirely. It's built from what your PC already knows:</p>
                  <ul className="docs-list">
                    <li>The <strong>current build</strong> from Steam (marked <em>current - you are here</em>).</li>
                    <li>Older builds still in Steam's local <strong>depotcache</strong>, each with its real build date.</li>
                    <li>Where a patch note lines up, a build is labelled <em>the build before &ldquo;&hellip;&rdquo;</em>, using
                      Steam's public news feed.</li>
                  </ul>
                  <p>Click <strong>use this build</strong> on any row to select it, then download. The shortcut
                    <strong> Go back one build</strong> selects the version right before the current update in one click.</p>
                  <p className="docs-note">Only the current build and builds cached on your PC can be listed here.
                    Steam prunes the depotcache over time, so deep history may not appear - for those, use the
                    <strong> SteamDB</strong> fallback under each depot. We never scrape SteamDB; it only opens as a link.</p>
                </section>

                <section id="docs-manifest" className="docs-sec">
                  <h4>Getting a manifest from SteamDB</h4>
                  <p>Steam's app doesn't list old builds - <strong>SteamDB</strong> does. A <em>manifest</em> is the ID of
                    one specific build of one depot. For each depot:</p>
                  <ol className="docs-list">
                    <li>Click <strong>“Open depot … on SteamDB”</strong> - it opens that depot's Manifests page in your browser.</li>
                    <li>Find the build you want <strong>by date</strong> in the list.</li>
                    <li>Click the <strong>copy</strong> button on that row (or copy just the Manifest ID).</li>
                    <li>Paste it into that depot's field. The app auto-detects the manifest ID (the long ~19-digit number).</li>
                  </ol>
                  <p>Accepted paste formats:</p>
                  <ul className="docs-list">
                    <li>Steam console: <code>download_depot 945361 945362 1234567890123456789</code></li>
                    <li>DepotDownloader: <code>-depot 945362 -manifest 1234567890123456789</code></li>
                    <li>Plain Manifest ID: <code>1234567890123456789</code></li>
                  </ul>
                  <p className="docs-note">We only <em>link</em> to SteamDB and never scrape it, to respect their terms.</p>
                </section>

                <section id="docs-depots" className="docs-sec">
                  <h4>Depots explained</h4>
                  <p>A game is split into <strong>depots</strong>: separate content packages - e.g. Windows 64-bit game
                    files, 32-bit files, macOS/Linux builds, language packs, and DLC. Each has its own manifest history.</p>
                  <p>For most Windows games you only need the <strong>main Windows 64-bit content depot</strong>, which is
                    shown by default. Use <strong>“show all depots”</strong> to reveal the rest (other OS, 32-bit, DLC) and
                    paste a manifest for each one you actually want.</p>
                </section>

                <section id="docs-apply-separate" className="docs-sec">
                  <h4>Apply · separate copy + shortcut <span className="badge">recommended</span></h4>
                  <p>How it works:</p>
                  <ul className="docs-list">
                    <li>Copies the downloaded build into a separate <code>…_frozen</code> folder next to your install.</li>
                    <li>Adds it to Steam as a <strong>Non-Steam game shortcut</strong>, so you can launch it from your library.</li>
                    <li>Your <strong>original install is never touched</strong> and stays current - so online / multiplayer
                      keeps working on the latest version.</li>
                  </ul>
                  <p>Best when you want to keep playing the current version online but also have the old build around.
                    <strong> Undo</strong> removes the shortcut from Steam and deletes the frozen copy.</p>
                  <p className="docs-note">Restart Steam after applying so the new shortcut shows up in your library.</p>
                </section>

                <section id="docs-apply-inplace" className="docs-sec">
                  <h4>Apply · in-place freeze</h4>
                  <p>How it works:</p>
                  <ul className="docs-list">
                    <li>Moves your current install aside to a <code>…​.downgrader-backup</code> folder.</li>
                    <li>Copies the old build into the original install location.</li>
                    <li>Patches the app manifest (<code>appmanifest_*.acf</code>) to <strong>disable auto-update</strong> and
                      mark the game “up to date”, so Steam won't immediately re-patch it.</li>
                  </ul>
                  <p><strong>Caveat:</strong> for online games, Steam may still flag an update when connected. The most
                    reliable way to hold an old version is Steam's <strong>Offline Mode</strong>. <strong>Undo</strong>{" "}
                    restores your original from the backup and re-enables updates.</p>
                </section>

                <section id="docs-manage" className="docs-sec">
                  <h4>Launch &amp; manage</h4>
                  <p>Each saved version has four actions:</p>
                  <ul className="docs-list">
                    <li><strong>▶ Launch</strong> - runs the old build's main .exe directly (Steam can stay running in the background).</li>
                    <li><strong>Apply ▾</strong> - only for installed games; choose one of the two methods above.</li>
                    <li><strong>Folder</strong> - opens the download folder in Explorer.</li>
                    <li><strong>✕ Delete</strong> - see below.</li>
                  </ul>
                </section>

                <section id="docs-delete-undo" className="docs-sec">
                  <h4>Delete &amp; undo</h4>
                  <p>Deleting a version that was <strong>applied</strong> automatically undoes the apply first, so the game
                    is also removed from Steam:</p>
                  <ul className="docs-list">
                    <li><strong>Separate copy</strong> → removes the Non-Steam shortcut from Steam and deletes the frozen copy.</li>
                    <li><strong>In-place freeze</strong> → restores your original install from the backup and re-enables updates.</li>
                  </ul>
                  <p>Then the downloaded files are deleted. The confirm dialog tells you exactly what will happen.</p>
                </section>

                <section id="docs-download-location" className="docs-sec">
                  <h4>Download location</h4>
                  <p>By default, builds are saved in a folder <strong>next to the game's install</strong>. Use
                    <strong> “Save to → choose…”</strong> in step 1 to pick another folder (e.g. <code>D:\Rollbacks</code>) -
                    handy to avoid writing into <code>Program Files</code>. Your choice is remembered across sessions.</p>
                </section>

                <section id="docs-owned-installed" className="docs-sec">
                  <h4>Owned vs installed</h4>
                  <p><strong>Installed</strong> = games found locally on your machine. <strong>Owned · not installed</strong>{" "}
                    = games you own but haven't installed (loaded after you sign in). Both can be downloaded; only installed
                    ones can be <em>applied</em> in place.</p>
                  <p>If a game <em>is</em> installed but wasn't detected (e.g. an unusual library folder), open it and use
                    <strong> “locate…”</strong> in its header to point at the install folder - that re-enables Apply.</p>
                </section>

                <section id="docs-sign-in" className="docs-sec">
                  <h4>Signing in</h4>
                  <p>Sign in with a <strong>QR code</strong> (scan it in the Steam Mobile App, approve - no typing) or with
                    <strong> username &amp; password</strong>. Credentials go straight to Steam and are <strong>never stored
                    by this app</strong>. You can only download games your own account owns.</p>
                </section>

                <section id="docs-data-security" className="docs-sec">
                  <h4>Data &amp; security</h4>
                  <p>Steam Downgrader has <strong>no backend of its own</strong>. It collects no analytics, has no
                    telemetry, and never sends anything about you or your usage to the developer or any third party.</p>
                  <p>The only things that ever talk to the network are:</p>
                  <ul className="docs-list">
                    <li><strong>Steam's servers</strong>, through the official SteamKit2 library and the DepotDownloader
                      engine, for signing in, reading your library, looking up builds, and downloading them.</li>
                    <li><strong>GitHub</strong>, to check for and download app updates.</li>
                    <li><strong>SteamDB</strong> is only ever opened as a normal link in your browser. The app never
                      contacts or scrapes it.</li>
                  </ul>
                  <p>Your <strong>Steam login</strong> goes straight to Steam through SteamKit2. Your password is never
                    stored and never sent anywhere except Steam. After sign-in, Steam issues a login token (a refresh
                    token, not your password), cached locally in <code>account.config</code> so downloads do not ask you
                    to sign in every time. Deleting that file removes the cached login.</p>
                  <p>What is stored <strong>on your machine</strong>:</p>
                  <ul className="docs-list">
                    <li>The rollback library at <code>%APPDATA%\steam-downgrader\rollbacks.json</code> (games, builds,
                      folder paths, applied status). It contains no credentials.</li>
                    <li>The game builds you download, in the folder you choose.</li>
                    <li>A small setting for your preferred download folder.</li>
                  </ul>
                  <p className="docs-note">You can remove all of it at any time by deleting the app, the
                    <code>account.config</code> file, and the folders listed above.</p>
                </section>

                <section id="docs-troubleshooting" className="docs-sec">
                  <h4>Troubleshooting</h4>
                  <ul className="docs-list">
                    <li><strong>“Connection to Steam failed” during download</strong> - the download engine needs its own
                      Steam session; the app hands it over automatically. Just retry. If it persists, the download console
                      shows <code>[steamkit]</code> lines with the real reason.</li>
                    <li><strong>Depots take a while to load</strong> - the first game each session waits ~15s for the Steam
                      connection; after that it's instant.</li>
                    <li><strong>Nothing under “owned”</strong> - hit <em>retry</em> in that group and make sure you're signed in.</li>
                    <li><strong>Applied build still updates online</strong> - start Steam in Offline Mode, or prefer the
                      separate-copy method.</li>
                  </ul>
                </section>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ---------- Update dialog (custom UI over the updater plugin) ---------- */}
      {updateOpen && (
        <div className="modal-overlay" onClick={() => !updateBusy && setUpdateOpen(false)}>
          <div className="modal modal--update" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>app update</h3>
              <button className="close-x" onClick={() => setUpdateOpen(false)} disabled={updateBusy}>×</button>
            </div>
            <div className="modal-body">
              {updateStage === "checking" && <p className="modal-note">Checking for updates...</p>}

              {updateStage === "uptodate" && (
                <p className="update-msg">You are on the latest version.</p>
              )}

              {updateStage === "error" && (
                <>
                  <p className="update-msg">Could not check for updates.</p>
                  {updateError && <p className="modal-note" style={{ wordBreak: "break-word" }}>{updateError}</p>}
                  <div className="confirm-actions">
                    <button className="btn" onClick={() => setUpdateOpen(false)}>close</button>
                    <button className="btn btn--primary" onClick={() => checkForUpdate(true)}>retry</button>
                  </div>
                </>
              )}

              {update && (updateStage === "available" || updateBusy) && (
                <>
                  <div className="update-hero">
                    <div className="uv">
                      {update.currentVersion && (
                        <>
                          <span className="uv-old">v{update.currentVersion}</span>
                          <span className="uv-arrow">→</span>
                        </>
                      )}
                      <span className="uv-new">v{update.version}</span>
                    </div>
                    <p className="update-sub">A new version of Steam Downgrader is available.</p>
                  </div>

                  {update.body ? (
                    <div className="update-changelog">
                      <div className="rn-title">what&apos;s new</div>
                      <div className="update-notes"><ReleaseNotes body={update.body} /></div>
                    </div>
                  ) : null}

                  {updateBusy ? (
                    <div className="update-progress">
                      <div className="progress">
                        <div className="progress-bar"><span style={{ width: `${updateProgress}%` }} /></div>
                        <div className="progress-pct">{updateProgress.toFixed(0)}%</div>
                      </div>
                      <p className="modal-note">{updateStage === "installing" ? "Installing, the app will restart..." : "Downloading update..."}</p>
                    </div>
                  ) : (
                    <div className="confirm-actions">
                      <button className="btn" onClick={() => setUpdateOpen(false)}>later</button>
                      <button className="btn btn--primary" onClick={installUpdate}>install and restart</button>
                    </div>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ---------- Confirm dialog ---------- */}
      {confirmState && (
        <div className="modal-overlay" onClick={() => closeConfirm(false)}>
          <div className="modal modal--confirm" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>{confirmState.title}</h3>
              <button className="close-x" onClick={() => closeConfirm(false)}>×</button>
            </div>
            <div className="modal-body">
              <p className="confirm-msg">{confirmState.message}</p>
              <div className="confirm-actions">
                <button className="btn" onClick={() => closeConfirm(false)}>cancel</button>
                <button className={`btn ${confirmState.danger ? "btn--danger" : "btn--primary"}`} autoFocus onClick={() => closeConfirm(true)}>
                  {confirmState.confirmLabel}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ---------- Login modal (QR-first) ---------- */}
      {showLogin && (
        <div className="modal-overlay" onClick={closeLogin}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <h3>sign in to steam</h3>
              <button className="close-x" onClick={closeLogin}>×</button>
            </div>
            {authMode === "qr" ? (
              <div className="modal-body qr-wrap">
                <p className="modal-note">Open the <strong>Steam Mobile App</strong>, tap the QR-scan icon (top-left), and scan this code. Approve on your phone to sign in - no code typing.</p>
                <div className="qr-tile">
                  {qrUrl
                    ? <QRCodeSVG value={qrUrl} size={198} bgColor="#ffffff" fgColor="#000000" level="M" marginSize={2} />
                    : <div className="qr-loading">generating code…</div>}
                </div>
                <div className="qr-status">
                  <span className={`dot ${qrPending ? "" : "dot--off"}`} />
                  {qrPending ? "waiting for approval in the Steam app…" : "starting…"}
                </div>
                <button className="link-toggle" onClick={() => setAuthMode("password")}>use username &amp; password instead</button>
              </div>
            ) : (
              <div className="modal-body">
                <p className="modal-note">Your own account, used only to download builds you own. Credentials go straight to Steam - never stored by this app.</p>
                <input className="field" placeholder="steam username" autoComplete="off"
                  value={username} onChange={(e) => setUsername(e.currentTarget.value)} />
                <input className="field" type="password" placeholder="password" autoComplete="off"
                  value={password} onChange={(e) => setPassword(e.currentTarget.value)} />
                <button className="btn btn--primary" onClick={doLogin} disabled={authBusy || !username || !password}>
                  {authBusy ? "signing in…" : "sign in"}
                </button>
                {need2fa && (
                  <div className="modal-2fa">
                    <p className="modal-note">Steam Guard ({need2fa}) code:</p>
                    <div className="row">
                      <input className="field" value={code} onChange={(e) => setCode(e.currentTarget.value)}
                        onKeyDown={(e) => { if (e.key === "Enter") submitCode(); }} />
                      <button className="btn btn--primary btn--sm" onClick={submitCode}>confirm</button>
                    </div>
                  </div>
                )}
                <button className="link-toggle" onClick={() => { setAuthMode("qr"); startQr(); }}>use QR code instead</button>
              </div>
            )}
            <div className="login-transparency">
              <span className="lt-badge">transparency</span>
              <p>Your login goes straight to Steam through the official SteamKit2 library. Your password is never stored or sent anywhere except Steam, and this app has no server and no telemetry.</p>
              <button className="lt-link" onClick={() => { closeLogin(); openDocs("data-security"); }}>how your data is handled</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
