# Steam Downgrader

Roll a Steam game back to an earlier build and play that old version, even though
Steam only ever installs the latest one.

Steam Downgrader is a desktop app (Tauri, Rust + React) that downloads a past
build straight from Steam's own servers, using your own login and only for games
you own, then either swaps it into your install or keeps it as a separate,
launchable copy. It is meant to be friendlier than running DepotDownloader by
hand: you browse your games, paste a manifest from SteamDB, and click download.

This is a Windows app.

## Table of contents

- [What it does](#what-it-does)
- [Requirements](#requirements)
- [Getting started](#getting-started)
- [Using the app](#using-the-app)
- [The two apply methods](#the-two-apply-methods)
- [How it works](#how-it-works)
- [Project layout](#project-layout)
- [Legal and safety](#legal-and-safety)
- [License](#license)
- [Acknowledgements](#acknowledgements)

## What it does

- Browse your installed games and the games you own but have not installed.
- Pick an old build by pasting its manifest id (copied from SteamDB).
- Download that build from Steam using your own account.
- Apply it to an installed game, or launch the downloaded build directly.
- Remember every download in a persistent rollback library, so you can launch,
  apply, or delete versions at any time.

## Requirements

To run from source you need:

- Windows 10 or 11
- Steam installed, and a Steam account that owns the games you want to roll back
- Rust (stable, 1.97 or newer) with the MSVC toolchain
- Node.js 18 or newer and npm
- .NET SDK 10 (for the helper process)
- The Tauri prerequisites (WebView2 is preinstalled on current Windows)

## Getting started

### 1. Clone with submodules

The download engine (DepotDownloader) is included as a git submodule and is
compiled into the helper, so clone recursively:

    git clone --recurse-submodules <your-fork-url>
    cd steam-downgrader

If you already cloned without submodules:

    git submodule update --init --recursive

### 2. Install frontend dependencies

    npm install

### 3. Build the helper

The .NET helper is built separately and the Rust side finds it by path:

    cd steam-helper
    dotnet build -c Debug
    cd ..

### 4. Run in development

    npm run tauri dev

This starts Vite and the Tauri window. On Windows you can also double-click
`run-dev.bat`, which runs the same command and logs to `devlog.txt`.

### 5. Build a release

    npm run tauri build

Build the helper in Release first (`dotnet build -c Release` in `steam-helper`)
so it is picked up by the packaged app.

## Using the app

The workflow is three steps.

### Step 1: sign in

Click "sign in to steam" and either scan the QR code with the Steam mobile app
(recommended, no typing) or use your username and password. Your credentials go
straight to Steam and are never stored by this app. You can only download games
your own account owns.

### Step 2: get a manifest from SteamDB

Steam's own app does not list old builds, but SteamDB does. A manifest is the id
of one specific build of one depot. For each depot:

1. Click "Open depot ... on SteamDB". It opens that depot's manifests page in
   your browser.
2. Find the build you want by date.
3. Click copy on that row, or copy just the manifest id.
4. Paste it into that depot's field. The app auto-detects the manifest id from
   Steam console output, DepotDownloader output, or a plain id.

For most Windows games you only need the main Windows 64-bit content depot, which
is shown by default. Use "show all depots" to reveal the rest.

### Step 3: download, then apply or launch

Pick a download location (optional; it defaults to a folder next to the game's
install and is remembered across sessions), then download. When it finishes:

- If the game is installed, apply the build (see below).
- If the game is not installed, launch the downloaded build directly.

Every download is saved under "your rollback versions" for that game, so you can
launch, apply, open the folder, or delete it at any time.

## The two apply methods

### Separate copy plus shortcut (recommended)

- Copies the downloaded build into a separate folder next to your install.
- Adds it to Steam as a non-Steam game shortcut, so you can launch it from your
  library.
- Your original install is never touched and stays current, so online and
  multiplayer keep working on the latest version.
- Restart Steam after applying so the shortcut appears.

### In-place freeze

- Moves your current install aside to a backup folder and copies the old build
  into the original location.
- Patches the app manifest to disable auto-update and mark the game up to date,
  so Steam does not immediately re-patch it.
- For online games, Steam may still flag an update when connected. The most
  reliable way to hold an old version is Steam's Offline Mode.

Deleting a version that was applied automatically undoes the apply first: it
removes the non-Steam shortcut and frozen copy (separate copy), or restores your
original install and re-enables updates (in-place). The app also has an in-app
docs and help panel that explains all of this.

## How it works

The app is a Tauri shell: a Rust backend and a React frontend. A small .NET
helper process handles everything that needs the Steam network protocol.

- The Rust backend talks to the .NET helper over newline-delimited JSON on
  stdin/stdout. Requests are correlated by id; events (2FA prompts, download
  progress) are forwarded to the UI.
- The helper uses SteamKit2 for login, ownership and depot info (PICS).
- Downloads run through DepotDownloader's engine, which is compiled into the
  helper from the git submodule (no separate process, no console window). Login
  reuses the token from the in-app sign-in.
- Applying and reverting are pure filesystem operations in Rust: moving folders,
  patching the appmanifest ACF, and editing Steam's binary shortcuts.vdf.

SteamDB is only ever linked to in your browser. The app never scrapes or embeds
it, to respect SteamDB's terms.

## Project layout

    src/                    React + TypeScript frontend (single-file UI in App.tsx)
    src-tauri/              Rust backend
      src/lib.rs            Tauri command registrations
      src/rollback.rs       persistent rollback library
      src/steam/            Steam integration
        sidecar.rs          bridge to the .NET helper
        auth.rs             login (QR / password)
        library.rs          owned games
        resolver.rs         manifest resolution
        downloader.rs       download via the embedded engine
        applier.rs          apply / revert (ACF, shortcuts.vdf, backups)
    steam-helper/           .NET helper (SteamKit2 + DepotDownloader engine)
      Program.cs            stdio JSON protocol and Steam logic
      vendor/DepotDownloader/  git submodule (SteamRE/DepotDownloader)

## Legal and safety

- Ownership boundary: the app only downloads games your signed-in account owns,
  with your own login. It does not bypass DRM or use other accounts. This is the
  same boundary DepotDownloader enforces.
- Multiplayer games usually reject old clients server-side, so downgrades are
  best for singleplayer or offline play.
- Games with their own launcher or auto-patcher may update themselves again.
- Very old manifests can be pruned from Steam's content servers and may no longer
  be downloadable.
- Your Steam credentials are sent only to Steam and are never stored by this app.

## License

This project bundles and compiles DepotDownloader, which is licensed under
GPL-2.0. As a result, a distributed build of Steam Downgrader is a derivative
work and must comply with GPL-2.0. Add a `LICENSE` file to your repository
accordingly before publishing. SteamKit2, used by the helper, is LGPL-2.1.

## Acknowledgements

- [SteamRE/DepotDownloader](https://github.com/SteamRE/DepotDownloader)
- [SteamKit2](https://github.com/SteamRE/SteamKit)
- [SteamDB](https://steamdb.info) for the public manifest history
- [Tauri](https://tauri.app)
