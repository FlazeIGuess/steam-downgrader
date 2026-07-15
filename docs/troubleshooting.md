# Troubleshooting and limits

Common issues and what the tool can and cannot do. For the full walkthrough, see
[Using Steam Downgrader](usage.md).

## Good to know

- Best for singleplayer or offline play. Online games often reject old clients.
- Games with their own launcher or updater may patch themselves back up.
- Very old builds can be removed from Steam's servers and may no longer download.
- Only the current build and builds cached on your PC appear in the dated list. For
  anything older, use the SteamDB link and paste a manifest.
- Your Steam login is sent only to Steam and is never stored by this app.

## Common issues

### "Connection to Steam failed" during download

The download engine needs its own Steam session, and the app hands it over
automatically. Just retry. If it persists, the download console shows `[steamkit]`
lines with the real reason.

### Depots take a while to load

The first game each session waits a few seconds for the Steam connection. After that
it is quick.

### Nothing appears under "owned, not installed"

Make sure you are signed in, then use retry in that group. Some accounts take a
moment to return the full library.

### An applied build still updates online

Start Steam in Offline Mode, or prefer the separate-copy method, which leaves your
real install current and adds the old build as a separate non-Steam shortcut.

### The build I want is not in the list

Only builds cached on your PC are listed. Open the depot on SteamDB, find the build
by date, copy its manifest, and paste it into that depot's field. See
[Getting a build from SteamDB](usage.md#getting-a-build-from-steamdb).

### A game is installed but not detected

Open the game and use "locate..." in its header to point at the install folder. That
re-enables the apply options.

## Safety notes

- Steam Downgrader only downloads builds of games your own account owns, using your
  own login. It does not bypass DRM or any copy protection.
- Rolling a game back is meant for singleplayer and offline use. Old or modified game
  files can be rejected online, and in games with anti-cheat (such as VAC) they may
  put your account at risk. Do not use downgraded builds in online or competitive
  games.
- Make sure your use follows the Steam Subscriber Agreement.
