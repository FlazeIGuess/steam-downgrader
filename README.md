<div align="center">
  <img src="src-tauri/icons/icon.png" alt="Steam Downgrader" width="120" />
  <h1>Steam Downgrader</h1>
  <p>Roll a Steam game back to an older version, and play it, even though Steam only ever installs the latest one.</p>
</div>

## What it does

Sometimes an update breaks a game, removes a feature, or nerfs something you liked.
Steam only keeps the newest version, so there is normally no way back. Steam
Downgrader gets an older build straight from Steam using your own account, and lets
you play it again.

With it you can:

- Browse your installed games, plus games you own but have not installed.
- Pick an older build by date and download it.
- Swap it into your game, or keep it as a separate copy you can launch.
- Keep every download in a library and switch between versions anytime.

It only works with games your own account owns, and it never bypasses any
protection. This is a Windows app.

## Get it

1. Go to the [Releases page](https://github.com/FlazeIGuess/steam-downgrader/releases)
   and download the latest installer.
2. If there is no release yet, you can build it yourself (see
   [Build from source](#build-from-source) at the bottom).

## How to use it

Three steps:

1. Sign in to Steam. Scan the QR code with the Steam mobile app (easiest), or use
   your username and password. Your login goes straight to Steam and is never saved
   by the app.
2. Choose the build. Steam does not list old builds, but the website SteamDB does.
   Click "Open on SteamDB", find the build you want by date, copy its manifest, and
   paste it into the app.
3. Download, then apply or play. Choose where to save it and download. Then either
   apply the build to your installed game, or launch the downloaded build directly.

Every download is saved under "your rollback versions", so you can launch it, apply
it, open its folder, or delete it at any time. The app also has a built-in help
panel that explains everything with small "?" tooltips.

## The two ways to apply a build

- Separate copy (recommended): keeps a frozen copy of the old build next to your
  game and adds it to Steam as a non-Steam shortcut. Your real game stays up to
  date, so online and multiplayer keep working.
- In-place freeze: replaces your installed game with the old build and turns off
  auto-update. For online games you may still need Steam's Offline Mode to stop it
  from updating back.

Deleting a version that you applied undoes it automatically: it removes the shortcut
and the frozen copy, or restores your original install.

## Good to know

- Best for singleplayer or offline play. Online games often reject old clients.
- Games with their own launcher or updater may patch themselves back up.
- Very old builds can be removed from Steam's servers and may no longer download.
- Your Steam login is sent only to Steam and is never stored by this app.

## Build from source

<details>
<summary>Requirements, build steps, and how it works (for developers)</summary>

### Requirements

- Windows 10 or 11
- Steam installed, plus a Steam account that owns the games
- Rust (stable, 1.97 or newer) with the MSVC toolchain
- Node.js 18 or newer and npm
- .NET SDK 10 (for the helper process)
- The Tauri prerequisites (WebView2 is preinstalled on current Windows)

### Steps

Clone with submodules (the download engine is a git submodule):

```
git clone --recurse-submodules https://github.com/FlazeIGuess/steam-downgrader.git
cd steam-downgrader
```

Install and build:

```
npm install
cd steam-helper && dotnet build -c Debug && cd ..
npm run tauri dev
```

Build a release installer (build the helper in Release first):

```
cd steam-helper && dotnet build -c Release && cd ..
npm run tauri build
```

### How it works

The app is a Tauri shell (Rust backend, React frontend) plus a small .NET helper
that talks to Steam.

- The Rust backend talks to the .NET helper over newline-delimited JSON on
  stdin/stdout.
- The helper uses SteamKit2 for login, ownership, and depot info, and runs downloads
  through DepotDownloader's engine, which is compiled in from a git submodule.
- Applying and reverting are plain filesystem operations: moving folders, patching
  the appmanifest, and editing Steam's shortcuts file.

SteamDB is only ever opened in your browser. The app never scrapes it.

### Project layout

```
src/            React + TypeScript frontend
src-tauri/      Rust backend (Tauri commands, Steam integration, apply/revert)
steam-helper/   .NET helper (SteamKit2 + embedded DepotDownloader engine)
```

</details>

## License

This project bundles and compiles DepotDownloader, which is licensed under GPL-2.0,
so a distributed build of Steam Downgrader must comply with GPL-2.0. SteamKit2 is
LGPL-2.1. Add a `LICENSE` file to the repository accordingly before publishing.

## Credits

- [SteamRE/DepotDownloader](https://github.com/SteamRE/DepotDownloader)
- [SteamKit2](https://github.com/SteamRE/SteamKit)
- [SteamDB](https://steamdb.info) for the public build history
- [Tauri](https://tauri.app)
