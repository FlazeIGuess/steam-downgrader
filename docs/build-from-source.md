# Build from source

For developers who want to run or build Steam Downgrader themselves. For an overview
of the codebase, see [How it works](how-it-works.md).

## Requirements

- Windows 10 or 11
- Steam installed, plus a Steam account that owns the games
- Rust (stable, 1.97 or newer) with the MSVC toolchain
- Node.js 18 or newer and npm
- .NET SDK 10 (for the helper process)
- The Tauri prerequisites (WebView2 is preinstalled on current Windows)

## Clone

The download engine is a git submodule, so clone recursively:

```
git clone --recurse-submodules https://github.com/FlazeIGuess/steam-downgrader.git
cd steam-downgrader
```

If you already cloned without submodules:

```
git submodule update --init --recursive
```

## Install and run

```
npm install
cd steam-helper && dotnet build -c Debug && cd ..
npm run tauri dev
```

The Rust backend rebuilds itself on change. The .NET helper does not, so after
changing it run `dotnet build` again and restart the app.

## Build a release installer

Build the helper in Release first, then build the app:

```
cd steam-helper && dotnet build -c Release && cd ..
npm run tauri build
```

The installer bundles the helper as a single self-contained executable, so end users
do not need the .NET runtime installed.

## Project layout

```
src/            React + TypeScript frontend
src-tauri/      Rust backend (Tauri commands, Steam integration, apply/revert)
steam-helper/   .NET helper (SteamKit2 + embedded DepotDownloader engine)
docs/           Documentation
```
