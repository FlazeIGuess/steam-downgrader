# Changelog

The section for a version tag is used as the release notes and shown in the
in-app update window. Add a new "## vX.Y.Z" section at the top for each release.

## v0.3.0

Added

- Community version archive (optional, off by default). Turn it on and older builds
  that other players have discovered appear in the dated build list too, marked
  "from archive", so you can find and download versions by date without hunting for
  manifest numbers on SteamDB.
- A one-time prompt asks whether to join the archive when you first start the app.
  You can turn it on or off at any time under Data and security in the built-in help.
- Builds from the archive are labelled with the update they came before (from Steam's
  public patch notes), the same as your local builds.

Changed

- When the archive is on, the app shares the build ids and dates it finds, both from
  the games in your library and from builds you download, so the shared list keeps
  growing. Only build ids and dates are ever sent, never your account, your login,
  your files, or any personal data, and each build is sent only once per machine.

## v0.2.2

Added

- A transparency note on the sign-in screen, plus a new "Data and security" section
  in the in-app help, explaining what talks to the network and what stays on your
  machine.

## v0.2.1

Changed

- The in-app update window now shows a cleaner, structured changelog, with sections
  and bullet lists instead of raw text, and a clearer version summary.
- Reworked the documentation: the README is now a short overview, and the full
  guides moved into a docs folder (usage, how it works, privacy, build from source,
  and troubleshooting).

## v0.2.0

New

- Pick an older build by date, right in the app. Steam Downgrader now lists the
  current build and older builds still cached on your PC, each labelled with the
  update it came before, so you can usually skip SteamDB and manifest codes.
- One-click "Go back one build" jumps straight to the version right before the
  last update.
- The app version is now shown in the sidebar.

Changed

- Choosing a build is organised per depot: the depot that matches your machine is
  shown first with its own build list and manifest field, and any other depots are
  one click away under "show all depots". Pasting a manifest from SteamDB stays
  available for any build that is not cached locally.

Fixed

- Signing in no longer briefly opens a console window on Windows.
- The active sidebar tab stays readable on hover.

## v0.1.0

First release.

- Browse your installed Steam games and the games you own but have not installed.
- Download an older build of a game by pasting its manifest from SteamDB.
- Apply a build as a separate copy with a non-Steam shortcut, or freeze it in place.
- Keep every download in a rollback library and switch between versions anytime.
- Sign in with a QR code or with a username and password.
- Built-in help panel and an automatic update checker.
