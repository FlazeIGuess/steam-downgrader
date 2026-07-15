# How it works

A look under the hood of Steam Downgrader. For how to use it, see
[Using Steam Downgrader](usage.md).

## Tech stack

- Desktop shell: Tauri 2, a Rust backend and a web frontend in one native window.
- Frontend: React and TypeScript, built with Vite.
- App logic: Rust, the commands that scan your Steam libraries, apply and revert
  builds, and manage the rollback library.
- Steam integration: a small .NET helper that uses SteamKit2 for login, ownership,
  and build information.
- Downloads: the DepotDownloader engine, compiled into the helper from a git
  submodule.
- Updates: the Tauri updater, with signed release artifacts.

## The pieces

Steam Downgrader is made of three parts, all running on your machine:

1. The window you see, built with React and TypeScript.
2. A Rust backend inside the same app that does the local work: scanning your Steam
   libraries, applying and reverting builds, and keeping the rollback library.
3. A small .NET helper process that speaks to Steam. The Rust backend and the helper
   talk over a local text channel (JSON over standard input and output). None of
   that channel goes over the network.

## Where the build list comes from

The dated build list is assembled from what your machine already knows, so there is
no scraping and no third-party service involved:

- The current build comes from Steam's product info (PICS), read anonymously.
- Older builds come from Steam's local depotcache, where each manifest carries its
  real creation date.
- Patch note labels come from Steam's public news feed, matched to a build by date.

Builds that were never cached locally are not listed. For those, the app links out
to SteamDB in your browser so you can copy a manifest by hand.

## What happens when you roll a game back

1. You sign in. The helper authenticates with Steam and receives a login token.
2. The app reads your installed and owned games, and the depots of the game you pick.
3. You pick the build by date from the in-app list (the current build plus builds
   cached on your PC), or paste a manifest from SteamDB for anything older.
4. The helper downloads exactly that build straight from Steam's content servers
   using the DepotDownloader engine, and the app shows live progress.
5. You either apply the build or launch it directly:
   - Separate copy: the build is copied into a frozen folder and added to Steam as a
     non-Steam shortcut, so your real install stays untouched.
   - In-place freeze: your install is moved aside as a backup, the old build is
     copied into its place, and auto-update is disabled in the app manifest.

Applying and reverting are plain local file operations (moving folders, patching the
appmanifest, and editing Steam's shortcuts file). Deleting an applied version undoes
it automatically. Every download is remembered in the rollback library, so you can
launch, re-apply, or delete it later.
