# Using Steam Downgrader

A full walkthrough of rolling a game back and managing your downloads. For a quick
overview, see the [main README](../README.md).

## Contents

- [Sign in to Steam](#sign-in-to-steam)
- [Pick a game](#pick-a-game)
- [Choose a build](#choose-a-build)
- [Depots](#depots)
- [Where downloads are saved](#where-downloads-are-saved)
- [Apply or launch a build](#apply-or-launch-a-build)
- [Manage your rollback versions](#manage-your-rollback-versions)
- [Delete and undo](#delete-and-undo)

## Sign in to Steam

Sign in with a QR code (scan it in the Steam mobile app and approve, no typing) or
with your username and password. Credentials go straight to Steam and are never
stored by this app. You can only download builds of games your own account owns.

See [Your data and privacy](privacy.md) for exactly what is sent and stored.

## Pick a game

The sidebar lists your games in two groups:

- Installed: games found locally on your machine. These can be downgraded in place.
- Owned, not installed: games you own but have not installed (loaded after you sign
  in). These can be downloaded and launched directly, but not applied in place.

If a game is installed but was not detected (for example an unusual library folder),
open it and use "locate..." in its header to point at the install folder. That
re-enables the apply options.

## Choose a build

Steam does not list old builds, but Steam Downgrader finds the ones your PC already
knows about and lists them by date, for the depot that matches your machine:

- The current build from Steam, marked "current - you are here".
- Older builds still in Steam's local depotcache, each with its real build date.
- Where a patch note lines up, a build is labelled "the build before ...", using
  Steam's public news feed.

Click "use this build" on any row to select it. The shortcut "Go back one build"
selects the version right before the current update in one click.

Only the current build and builds cached on your PC can be listed. Steam prunes the
depotcache over time, so deep history may not appear.

### Getting a build from SteamDB

For a build that is not listed, get its manifest from the website SteamDB. A
manifest is the id of one specific build of one depot.

1. Click "Open depot ... on SteamDB". It opens that depot's manifests page in your
   browser.
2. Find the build you want by date.
3. Copy the manifest id (or the whole row).
4. Paste it into that depot's field. The app auto-detects the manifest id (the long
   number, around 19 digits).

Accepted paste formats:

- Steam console: `download_depot 945361 945362 1234567890123456789`
- DepotDownloader: `-depot 945362 -manifest 1234567890123456789`
- Plain manifest id: `1234567890123456789`

Steam Downgrader only opens SteamDB as a normal link in your browser. It never
contacts or scrapes SteamDB, to respect their terms.

## Depots

A game is split into depots: separate content packages, for example Windows 64-bit
game files, 32-bit files, macOS or Linux builds, language packs, and DLC. Each has
its own manifest history and is downloaded on its own.

For most Windows games you only need the main Windows 64-bit content depot, which is
shown first with its own build list and manifest field. Use "show all depots" to
reveal the rest and fill in a build for each one you actually want. Only the depots
you fill in are downloaded.

## Where downloads are saved

By default, builds are saved in a folder next to the game's install. Use
"save to -> choose..." to pick another folder (for example `D:\Rollbacks`), which is
handy to avoid writing into `Program Files`. Your choice is remembered across
sessions.

## Apply or launch a build

After a build downloads it is saved under "your rollback versions". For an installed
game you can apply it in one of two ways, or just launch it directly.

### Separate copy (recommended)

- Copies the downloaded build into a separate frozen folder next to your install.
- Adds it to Steam as a non-Steam game shortcut, so you can launch it from your
  library.
- Your original install is never touched and stays current, so online and
  multiplayer keep working on the latest version.

Best when you want to keep playing the current version online but also have the old
build around. Restart Steam after applying so the shortcut shows up.

### In-place freeze

- Moves your current install aside to a `.downgrader-backup` folder.
- Copies the old build into the original install location.
- Patches the app manifest (`appmanifest_*.acf`) to disable auto-update and mark the
  game up to date, so Steam does not immediately re-patch it.

For online games, Steam may still flag an update when connected. The most reliable
way to hold an old version is Steam's Offline Mode.

### Launch directly

Launch runs the old build's main executable directly, with Steam able to stay
running in the background. This works for both installed and download-only games.

## Manage your rollback versions

Every download is remembered in the rollback library (kept across restarts). Each
saved version has four actions:

- Launch: runs the old build's main executable directly.
- Apply: only for installed games, choose one of the two methods above.
- Folder: opens the download folder in Explorer.
- Delete: removes the version, see below.

## Delete and undo

Deleting a version that was applied automatically undoes the apply first, so the
game is also cleaned up in Steam:

- Separate copy: removes the non-Steam shortcut from Steam and deletes the frozen
  copy.
- In-place freeze: restores your original install from the backup and re-enables
  updates.

Then the downloaded files are deleted. The confirm dialog tells you exactly what
will happen before anything is removed.
