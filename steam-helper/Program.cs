// steam-helper: a SteamKit2-based sidecar for the Steam Downgrader.
//
// Talks to the Rust/Tauri frontend over newline-delimited JSON:
//   Request:  {"id":1,"cmd":"appinfo","app_id":105600}
//   Response: {"id":1,"ok":true,"result":{...}}  |  {"id":1,"ok":false,"error":"..."}
//   Event:    {"event":"need_code","kind":"device"}   (e.g. a 2FA prompt)
//
// Supported commands:
//   ping                          -> {"pong":true}
//   appinfo   {app_id}            -> depots/manifests/build (anonymous, no login)
//   login     {username,password} -> login (2FA via need_code event + provide_code)
//   provide_code {code}           -> supply a requested 2FA code
//   owned                         -> list owned apps (after login)

using System.Text.Json;
using SteamKit2;
using SteamKit2.Authentication;

var helper = new Helper();
await helper.RunAsync();

/// <summary>Wraps the SteamClient + callback pump and the JSON command loop.</summary>
class Helper
{
    private readonly SteamClient _client = new();
    private readonly CallbackManager _manager;
    private readonly SteamUser _user;
    private readonly SteamApps _apps;

    private readonly object _outLock = new();
    private volatile bool _running = true;

    // The original stdout, captured BEFORE we redirect Console.Out to the event
    // writer during a download. Our JSON protocol always writes here.
    private readonly System.IO.TextWriter _stdout;

    // QR / password login yields a refresh token; the DepotDownloader engine uses
    // it to run its own login without another scan or password.
    private string? _refreshToken;
    private string? _accountName;
    private bool _ddInit;

    // Bridges from callback-based events to async/await.
    private TaskCompletionSource<bool>? _connected;
    private TaskCompletionSource<EResult>? _loggedOn;
    // Full license list (incl. per-package access token), used by the owned query.
    private TaskCompletionSource<IReadOnlyCollection<SteamApps.LicenseListCallback.License>>? _licenses;

    // For 2FA: the authenticator requests a code, the input loop supplies it.
    private TaskCompletionSource<string>? _pendingCode;

    public Helper()
    {
        _stdout = Console.Out;
        _manager = new CallbackManager(_client);
        _user = _client.GetHandler<SteamUser>()!;
        _apps = _client.GetHandler<SteamApps>()!;

        _manager.Subscribe<SteamClient.ConnectedCallback>(_ => _connected?.TrySetResult(true));
        _manager.Subscribe<SteamClient.DisconnectedCallback>(_ => _connected?.TrySetResult(false));
        _manager.Subscribe<SteamUser.LoggedOnCallback>(cb => _loggedOn?.TrySetResult(cb.Result));
        _manager.Subscribe<SteamApps.LicenseListCallback>(cb =>
            _licenses?.TrySetResult(cb.LicenseList));
    }

    public async Task RunAsync()
    {
        // Callback pump on its own thread.
        var pump = Task.Run(() =>
        {
            while (_running)
                _manager.RunWaitCallbacks(TimeSpan.FromMilliseconds(100));
        });

        // Command loop: one line = one JSON request.
        string? line;
        while (_running && (line = Console.In.ReadLine()) != null)
        {
            if (string.IsNullOrWhiteSpace(line)) continue;
            _ = HandleLineAsync(line); // fire-and-forget, responses are correlated by id
        }

        _running = false;
        await pump;
    }

    private async Task HandleLineAsync(string line)
    {
        JsonElement req;
        try { req = JsonSerializer.Deserialize<JsonElement>(line); }
        catch (Exception e) { WriteEvent(new { @event = "error", error = $"invalid json: {e.Message}" }); return; }

        int id = req.TryGetProperty("id", out var idEl) ? idEl.GetInt32() : 0;
        string cmd = req.TryGetProperty("cmd", out var cmdEl) ? cmdEl.GetString() ?? "" : "";

        try
        {
            switch (cmd)
            {
                case "ping":
                    Respond(id, new { pong = true });
                    break;

                case "provide_code":
                    _pendingCode?.TrySetResult(req.GetProperty("code").GetString() ?? "");
                    Respond(id, new { accepted = true });
                    break;

                case "appinfo":
                    Respond(id, await AppInfoAsync(req.GetProperty("app_id").GetUInt32()));
                    break;

                case "local_manifests":
                    Respond(id, LocalManifests(req.GetProperty("dirs"), req.GetProperty("depots")));
                    break;

                case "login":
                    Respond(id, await LoginAsync(
                        req.GetProperty("username").GetString()!,
                        req.GetProperty("password").GetString()!));
                    break;

                case "login_qr":
                    Respond(id, await LoginQrAsync());
                    break;

                case "owned":
                    Respond(id, await OwnedAsync());
                    break;

                case "download":
                    Respond(id, await DownloadAsync(
                        req.GetProperty("app_id").GetUInt32(),
                        req.GetProperty("depots"),
                        req.GetProperty("output_dir").GetString()!));
                    break;

                default:
                    RespondError(id, $"unknown command: {cmd}");
                    break;
            }
        }
        catch (Exception e)
        {
            RespondError(id, e.Message);
        }
    }

    // --- Steam-Ablauf ---------------------------------------------------------

    /// <summary>Connect and log in anonymously (for public PICS lookups).</summary>
    private async Task EnsureAnonAsync()
    {
        if (_client.IsConnected && _user.SteamID != null) return;
        await ConnectAsync();
        _loggedOn = new TaskCompletionSource<EResult>();
        _user.LogOnAnonymous();
        var res = await _loggedOn.Task.WaitAsync(TimeSpan.FromSeconds(30));
        if (res != EResult.OK) throw new Exception($"anonymous login failed: {res}");
    }

    private async Task ConnectAsync()
    {
        if (_client.IsConnected) return;

        // SteamKit can pick a bad first CM server, so retry a few times with
        // backoff instead of giving up on the first disconnect.
        for (var attempt = 1; attempt <= 5; attempt++)
        {
            _connected = new TaskCompletionSource<bool>();
            _client.Connect();

            bool ok;
            try { ok = await _connected.Task.WaitAsync(TimeSpan.FromSeconds(20)); }
            catch (TimeoutException) { ok = false; }

            if (ok) return;

            try { _client.Disconnect(); } catch { /* ignore */ }
            await Task.Delay(TimeSpan.FromSeconds(attempt)); // 1s,2s,3s,4s
        }

        throw new Exception("could not connect to Steam (multiple attempts)");
    }

    /// <summary>Disconnect our client and wait until the disconnect is processed.</summary>
    private async Task DisconnectClientAsync()
    {
        if (!_client.IsConnected) return;
        _client.Disconnect();
        for (var i = 0; i < 50 && _client.IsConnected; i++)
            await Task.Delay(100);
    }

    /// <summary>
    /// Sign our client back in after a download (best effort) so owned/appinfo
    /// keep working. If it fails, the last login state is kept.
    /// </summary>
    private async Task RelogonClientAsync()
    {
        if (string.IsNullOrEmpty(_refreshToken) || string.IsNullOrEmpty(_accountName)) return;
        try
        {
            await ConnectAsync();
            _loggedOn = new TaskCompletionSource<EResult>();
            _licenses = new TaskCompletionSource<IReadOnlyCollection<SteamApps.LicenseListCallback.License>>();
            _user.LogOn(new SteamUser.LogOnDetails
            {
                Username = _accountName,
                AccessToken = _refreshToken,
                ShouldRememberPassword = false,
            });
            await _loggedOn.Task.WaitAsync(TimeSpan.FromSeconds(30));
        }
        catch { /* best effort, the download is already finished */ }
    }

    private async Task<object> AppInfoAsync(uint appId)
    {
        await EnsureAnonAsync();

        var job = _apps.PICSGetProductInfo(
            apps: new[] { new SteamApps.PICSRequest(appId) },
            packages: Array.Empty<SteamApps.PICSRequest>());
        var resultSet = await job.ToTask();

        SteamApps.PICSProductInfoCallback.PICSProductInfo? info = null;
        foreach (var r in resultSet.Results!)
            if (r.Apps.TryGetValue(appId, out var found)) { info = found; break; }

        if (info == null) throw new Exception($"no app info for {appId} (not public?)");

        var kv = info.KeyValues;
        var name = kv["common"]["name"].Value;
        var depotsKv = kv["depots"];
        var publicBranch = depotsKv["branches"]["public"];

        var depots = new List<object>();
        foreach (var depot in depotsKv.Children)
        {
            if (!uint.TryParse(depot.Name, out var depotId)) continue; // skip "branches", "baselanguages", etc.
            var pub = depot["manifests"]["public"];
            if (pub == KeyValue.Invalid) continue;
            var gid = pub["gid"] != KeyValue.Invalid ? pub["gid"].Value : pub.Value;
            if (string.IsNullOrEmpty(gid)) continue;
            var dcfg = depot["config"];
            depots.Add(new
            {
                depot_id = depotId,
                manifest_id = gid,
                name = depot["name"].Value,
                oslist = dcfg["oslist"].Value,     // "windows" / "macos" / "linux" / "windows,macos"
                osarch = dcfg["osarch"].Value,     // "64" / "32"
                language = dcfg["language"].Value, // e.g. "german"
                dlcappid = depot["dlcappid"].Value,
            });
        }

        return new
        {
            app_id = appId,
            name,
            build_id = publicBranch["buildid"].Value,
            time_updated = publicBranch["timeupdated"].Value, // Unix seconds of the current build
            depots,
        };
    }

    private async Task<object> LoginAsync(string username, string password)
    {
        await ConnectAsync();

        var authSession = await _client.Authentication.BeginAuthSessionViaCredentialsAsync(new AuthSessionDetails
        {
            Username = username,
            Password = password,
            IsPersistentSession = false,
            Authenticator = new StdioAuthenticator(this),
        });

        var poll = await authSession.PollingWaitForResultAsync();
        _refreshToken = poll.RefreshToken;
        _accountName = poll.AccountName;

        _loggedOn = new TaskCompletionSource<EResult>();
        _licenses = new TaskCompletionSource<IReadOnlyCollection<SteamApps.LicenseListCallback.License>>();
        _user.LogOn(new SteamUser.LogOnDetails
        {
            Username = poll.AccountName,
            AccessToken = poll.RefreshToken,
            ShouldRememberPassword = false,
        });

        var res = await _loggedOn.Task.WaitAsync(TimeSpan.FromSeconds(30));
        if (res != EResult.OK) throw new Exception($"login failed: {res}");

        return new { account = poll.AccountName, steam_id = _user.SteamID?.ConvertToUInt64() ?? 0 };
    }

    /// <summary>
    /// List manifest files from Steam's depotcache folders for the requested
    /// depots and read the real creation date (build date) from the file.
    /// Falls back to the file's modified time on parse errors.
    /// </summary>
    private object LocalManifests(JsonElement dirsEl, JsonElement depotsEl)
    {
        var depots = new HashSet<uint>();
        foreach (var d in depotsEl.EnumerateArray())
            depots.Add(d.GetUInt32());

        var results = new List<object>();
        foreach (var dirEl in dirsEl.EnumerateArray())
        {
            var dir = dirEl.GetString();
            if (string.IsNullOrEmpty(dir) || !Directory.Exists(dir)) continue;

            foreach (var file in Directory.EnumerateFiles(dir, "*.manifest"))
            {
                var name = Path.GetFileNameWithoutExtension(file); // "<depot>_<manifest>"
                var us = name.IndexOf('_');
                if (us <= 0) continue;
                if (!uint.TryParse(name.AsSpan(0, us), out var depotId)) continue;
                if (depots.Count > 0 && !depots.Contains(depotId)) continue;
                if (!ulong.TryParse(name.AsSpan(us + 1), out var manifestId)) continue;

                long creation;
                try
                {
                    var manifest = DepotManifest.Deserialize(File.ReadAllBytes(file));
                    creation = manifest.CreationTime != default
                        ? new DateTimeOffset(manifest.CreationTime.ToUniversalTime()).ToUnixTimeSeconds()
                        : new DateTimeOffset(File.GetLastWriteTimeUtc(file)).ToUnixTimeSeconds();
                }
                catch
                {
                    creation = new DateTimeOffset(File.GetLastWriteTimeUtc(file)).ToUnixTimeSeconds();
                }

                results.Add(new
                {
                    depot_id = depotId,
                    manifest_id = manifestId.ToString(),
                    creation_time = creation,
                });
            }
        }
        return new { manifests = results };
    }

    private async Task<object> LoginQrAsync()
    {
        await ConnectAsync();

        // No authenticator: the QR login is completed purely by device approval
        // (scan + approve in the app). An authenticator would, with the mobile
        // authenticator active, trigger the DeviceCode guard, which a QR session
        // cannot service.
        var authSession = await _client.Authentication.BeginAuthSessionViaQRAsync(new AuthSessionDetails
        {
            DeviceFriendlyName = "Steam Downgrader",
        });

        // Initiale Challenge-URL + Rotation ans Frontend melden (dort als QR gerendert).
        WriteEvent(new { @event = "qr_url", url = authSession.ChallengeURL });
        authSession.ChallengeURLChanged = () =>
            WriteEvent(new { @event = "qr_url", url = authSession.ChallengeURL });

        var poll = await authSession.PollingWaitForResultAsync();
        _refreshToken = poll.RefreshToken;
        _accountName = poll.AccountName;

        _loggedOn = new TaskCompletionSource<EResult>();
        _licenses = new TaskCompletionSource<IReadOnlyCollection<SteamApps.LicenseListCallback.License>>();
        _user.LogOn(new SteamUser.LogOnDetails
        {
            Username = poll.AccountName,
            AccessToken = poll.RefreshToken,
            ShouldRememberPassword = false,
        });

        var res = await _loggedOn.Task.WaitAsync(TimeSpan.FromSeconds(30));
        if (res != EResult.OK) throw new Exception($"login failed: {res}");

        return new { account = poll.AccountName, steam_id = _user.SteamID?.ConvertToUInt64() ?? 0 };
    }

    private async Task<object> OwnedAsync()
    {
        if (_licenses == null) throw new Exception("not signed in");
        var licenses = await _licenses.Task.WaitAsync(TimeSpan.FromSeconds(30));

        // Packages -> app ids. Pass the per-license access token, otherwise
        // token-gated packages return no contents.
        var appIds = new HashSet<uint>();
        var pkgReqs = licenses
            .Select(l => new SteamApps.PICSRequest(l.PackageID, l.AccessToken))
            .ToList();
        if (pkgReqs.Count > 0)
        {
            var pkgSet = await _apps.PICSGetProductInfo(
                apps: Array.Empty<SteamApps.PICSRequest>(),
                packages: pkgReqs).ToTask();
            foreach (var r in pkgSet.Results)
                foreach (var pkg in r.Packages.Values)
                    foreach (var appKv in pkg.KeyValues["appids"].Children)
                        if (uint.TryParse(appKv.Value, out var a)) appIds.Add(a);
        }

        // Fetch app access tokens. WITHOUT a token, PICS returns no "common"
        // section (no name/type) for many apps, so the type filter would drop all.
        var appTokens = new Dictionary<uint, ulong>();
        if (appIds.Count > 0)
        {
            var tok = await _apps.PICSGetAccessTokens(appIds, Array.Empty<uint>()).ToTask();
            foreach (var kv in tok.AppTokens) appTokens[kv.Key] = kv.Value;
        }

        // App ids -> names (launchable titles only: game / application), in
        // batches, since large accounts can own thousands of apps.
        var seen = new HashSet<uint>();
        var games = new List<object>();
        foreach (var chunk in appIds.Chunk(500))
        {
            var reqs = chunk.Select(a =>
                appTokens.TryGetValue(a, out var t)
                    ? new SteamApps.PICSRequest(a, t)
                    : new SteamApps.PICSRequest(a));
            var appSet = await _apps.PICSGetProductInfo(
                apps: reqs, packages: Array.Empty<SteamApps.PICSRequest>()).ToTask();
            foreach (var r in appSet.Results)
                foreach (var app in r.Apps.Values)
                {
                    var common = app.KeyValues["common"];
                    var type = common["type"].Value;
                    if (string.IsNullOrEmpty(type)) continue;
                    if (!(type.Equals("game", StringComparison.OrdinalIgnoreCase) ||
                          type.Equals("application", StringComparison.OrdinalIgnoreCase))) continue;
                    if (!seen.Add(app.ID)) continue;
                    games.Add(new { app_id = app.ID, name = common["name"].Value });
                }
        }

        return new { count = games.Count, games };
    }

    // --- Download via the vendored DepotDownloader engine ---------------------

    /// <summary>
    /// Download the resolved version through DepotDownloader's engine (in-process).
    /// Login reuses the refresh token already obtained via QR: we place it in
    /// AccountSettingsStore, then InitializeSteam3 runs the login with it, so
    /// DepotDownloader never prompts for QR or a password itself.
    /// </summary>
    private async Task<object> DownloadAsync(uint appId, JsonElement depotsEl, string outDir)
    {
        if (string.IsNullOrEmpty(_refreshToken) || string.IsNullOrEmpty(_accountName))
            throw new Exception("not signed in, sign in via QR first");

        if (!_ddInit)
        {
            DepotDownloader.AccountSettingsStore.LoadFromFile("account.config");
            _ddInit = true;
        }

        var cfg = DepotDownloader.ContentDownloader.Config;
        cfg.RememberPassword = true;
        cfg.InstallDirectory = outDir;
        cfg.MaxDownloads = 8;

        // Store our QR token as a saved login token, so the engine picks it up in
        // InitializeSteam3 as the access token and signs in silently.
        DepotDownloader.AccountSettingsStore.Instance.LoginTokens[_accountName] = _refreshToken;

        var depots = new List<(uint, ulong)>();
        foreach (var d in depotsEl.EnumerateArray())
        {
            var depotId = d.GetProperty("depot_id").GetUInt32();
            var manifestId = ulong.Parse(d.GetProperty("manifest_id").GetString()!);
            depots.Add((depotId, manifestId));
        }

        Directory.CreateDirectory(outDir);

        // DepotDownloader opens its OWN Steam session. Two concurrent connections
        // from the same process/account conflict (the new one gets dropped by
        // Steam -> "Connection to Steam failed"), so disconnect our client first
        // and restore it after the download.
        await DisconnectClientAsync();

        // Redirect the engine's console to our event writer (our own protocol
        // keeps writing directly to _stdout).
        var prevOut = Console.Out;
        var writer = new ConsoleEventWriter(this);
        Console.SetOut(writer);

        // Mirror SteamKit's DebugLog into the download log for a moment, so a
        // connection error reveals its real cause.
        var dbg = new DebugForwarder(this);
        DebugLog.AddListener(dbg);
        DebugLog.Enabled = true;

        try
        {
            if (!DepotDownloader.ContentDownloader.InitializeSteam3(_accountName, null))
                throw new Exception("the download engine's Steam login failed");

            await DepotDownloader.ContentDownloader.DownloadAppAsync(
                appId, depots, DepotDownloader.ContentDownloader.DEFAULT_BRANCH,
                os: null, arch: null, language: null, lv: false, isUgc: false);
        }
        finally
        {
            DepotDownloader.ContentDownloader.ShutdownSteam3();
            writer.Flush();
            Console.SetOut(prevOut);
            DebugLog.RemoveListener(dbg);
            DebugLog.Enabled = false;
            await RelogonClientAsync();
        }

        return new { output_dir = outDir, bytes_total = DirSize(outDir) };
    }

    private static ulong DirSize(string path)
    {
        if (!Directory.Exists(path)) return 0;
        ulong total = 0;
        foreach (var f in Directory.EnumerateFiles(path, "*", SearchOption.AllDirectories))
        {
            try { total += (ulong)new FileInfo(f).Length; } catch { }
        }
        return total;
    }

    /// <summary>Mirror SteamKit's DebugLog line by line into the download log (diagnostics).</summary>
    private sealed class DebugForwarder : IDebugListener
    {
        private readonly Helper _h;
        public DebugForwarder(Helper h) => _h = h;
        public void WriteLine(string category, string message)
            => _h.WriteEvent(new { @event = "download_log", line = $"[steamkit] {category}: {message}" });
    }

    /// <summary>Forward the engine's console output line by line as events and
    /// parse progress (" 42.13% file") into a download_progress event.</summary>
    private sealed class ConsoleEventWriter : System.IO.TextWriter
    {
        private readonly Helper _h;
        private readonly System.Text.StringBuilder _sb = new();

        public ConsoleEventWriter(Helper h) => _h = h;

        public override System.Text.Encoding Encoding => System.Text.Encoding.UTF8;

        public override void Write(char value)
        {
            if (value == '\n') Emit();
            else if (value != '\r') _sb.Append(value);
        }

        public override void Write(string? value)
        {
            if (value == null) return;
            foreach (var c in value) Write(c);
        }

        public override void Flush()
        {
            if (_sb.Length > 0) Emit();
        }

        private void Emit()
        {
            var line = _sb.ToString();
            _sb.Clear();
            if (line.Length == 0) return;
            _h.WriteEvent(new { @event = "download_log", line });

            var m = System.Text.RegularExpressions.Regex.Match(line, @"^\s*(\d+(?:\.\d+)?)%");
            if (m.Success &&
                double.TryParse(m.Groups[1].Value, System.Globalization.CultureInfo.InvariantCulture, out var pct))
            {
                _h.WriteEvent(new { @event = "download_progress", percent = pct });
            }
        }
    }

    // --- 2FA authenticator ----------------------------------------------------

    private sealed class StdioAuthenticator(Helper h) : IAuthenticator
    {
        public Task<string> GetDeviceCodeAsync(bool previousCodeWasIncorrect) => h.RequestCodeAsync("device", previousCodeWasIncorrect);
        public Task<string> GetEmailCodeAsync(string email, bool previousCodeWasIncorrect) => h.RequestCodeAsync("email", previousCodeWasIncorrect);
        public Task<bool> AcceptDeviceConfirmationAsync()
        {
            h.WriteEvent(new { @event = "device_confirmation" });
            return Task.FromResult(false); // force code entry instead of waiting for in-app approval
        }
    }

    private Task<string> RequestCodeAsync(string kind, bool retry)
    {
        _pendingCode = new TaskCompletionSource<string>();
        WriteEvent(new { @event = "need_code", kind, retry });
        return _pendingCode.Task;
    }

    // --- JSON output ----------------------------------------------------------

    private void Respond(int id, object result) => WriteRaw(new { id, ok = true, result });
    private void RespondError(int id, string error) => WriteRaw(new { id, ok = false, error });
    public void WriteEvent(object ev) => WriteRaw(ev);

    private void WriteRaw(object obj)
    {
        var json = JsonSerializer.Serialize(obj);
        lock (_outLock)
        {
            _stdout.WriteLine(json);
            _stdout.Flush();
        }
    }
}
