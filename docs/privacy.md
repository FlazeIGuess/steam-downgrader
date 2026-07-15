# Your data and privacy

Steam Downgrader has no backend of its own. It does not collect analytics, it has no
telemetry, and it never sends anything about you or your usage to the developer or to
any third party.

## What talks to the network

The only things that ever talk to the network are:

- Steam's own servers, through the official SteamKit2 library and the DepotDownloader
  engine, for signing in, reading your library, looking up builds, and downloading
  them.
- GitHub, to check whether a newer version of the app exists and to download the
  update.
- SteamDB is only ever opened as a normal link in your web browser. The app itself
  never contacts or scrapes SteamDB.

## Your Steam login

- You sign in with a QR code (scanned in the Steam mobile app) or with your username
  and password. These go straight to Steam through SteamKit2. Your password is never
  stored and never sent anywhere except Steam.
- After you sign in, Steam issues a login token (a refresh token, not your password).
  So downloads can run without asking you to sign in every time, this token is cached
  on your machine in a local `account.config` file. Deleting that file removes the
  cached login.

## What is stored on your machine

- The rollback library at `%APPDATA%\steam-downgrader\rollbacks.json`: the games and
  builds you downloaded (app ids, names, manifest ids, folder paths, and whether a
  build is applied). It contains no credentials.
- The game builds you download, in the folder you choose (by default a folder next to
  the game's install).
- A small setting for your preferred download folder.

Everything above stays on your computer. You can remove all of it at any time by
deleting the app, the `account.config` file, and the folders listed above.
