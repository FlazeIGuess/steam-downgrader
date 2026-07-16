# Your data and privacy

Steam Downgrader collects no analytics and has no telemetry. It never sends anything
about you, your account, or your usage to the developer or to any third party. The one
optional exception is the community version archive, which is off by default and
described below.

## What talks to the network

The things that talk to the network are:

- Steam's own servers, through the official SteamKit2 library and the DepotDownloader
  engine, for signing in, reading your library, looking up builds, and downloading
  them.
- GitHub, to check whether a newer version of the app exists and to download the
  update.
- SteamDB is only ever opened as a normal link in your web browser. The app itself
  never contacts or scrapes SteamDB.
- The community version archive, but only if you turn it on (see below). Off by
  default.

## Community version archive (optional)

On its own, the app only lists builds that are cached on your own PC, and for older builds
you would have to look up their manifest ids on SteamDB by hand. The community version
archive pools the manifest ids that players' apps discover, so more builds show up directly
in the dated list and you can pick them by date without hunting for manifest numbers. It
works both ways: what you find is shared too.

- If you turn it on, the app shares the build manifest ids and dates it finds for your
  games, and reads back what others have contributed, so you can find older builds by
  date even if they were never cached on your PC.
- Only manifest ids and build dates are shared. Never your account, your login, your
  Steam id, your files, or any personal data. The archive stores no IP address.
- It is off by default. You are asked once, and you can turn it off anytime under docs,
  Data and security. With it off, nothing is sent or fetched.

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
