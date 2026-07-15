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

- Browse your installed games, plus games you own but have not installed.
- Pick an older build by date, straight from the app, and download it.
- Swap it into your game, or keep it as a separate copy you can launch.
- Keep every download in a library and switch between versions anytime.

It only works with games your own account owns, and it never bypasses any
protection. This is a Windows app.

## Tutorial

<div align="center">
  <a href="https://youtu.be/oV0TT6KsZQI">
    <img src="https://img.youtube.com/vi/oV0TT6KsZQI/maxresdefault.jpg" alt="Steam Downgrader tutorial video" width="640" />
  </a>
  <p><a href="https://youtu.be/oV0TT6KsZQI">Watch the tutorial on YouTube</a></p>
</div>

## Get it

Go to the [Releases page](https://github.com/FlazeIGuess/steam-downgrader/releases)
and download the latest installer. Prefer to build it yourself? See
[Build from source](docs/build-from-source.md).

## Quick start

1. Sign in to Steam with a QR code (scan it in the Steam mobile app) or your
   username and password. Your login goes straight to Steam and is never stored.
2. Pick a game, then choose a build. Steam Downgrader lists builds by date: the
   current build plus older builds cached on your PC. Pick one, or use "Go back one
   build". For anything older, paste a manifest from SteamDB.
3. Download, then apply the build to your installed game or launch it directly.

Every download is saved under "your rollback versions" so you can launch, apply,
open, or delete it anytime. The app also has a built-in help panel with small "?"
tooltips.

For the full walkthrough, see [Using Steam Downgrader](docs/usage.md).

## Documentation

- [Using Steam Downgrader](docs/usage.md) - the full walkthrough.
- [Your data and privacy](docs/privacy.md) - what talks to the network and what is
  stored on your machine.
- [Troubleshooting and limits](docs/troubleshooting.md) - common issues and what the
  tool can and cannot do.
- [How it works](docs/how-it-works.md) - the tech stack and what happens end to end.
- [Build from source](docs/build-from-source.md) - requirements, build steps, and
  the project layout.

## Disclaimer

This project is not affiliated with, endorsed by, or associated with Valve
Corporation. Steam is a trademark of Valve Corporation.

Steam Downgrader is provided as is, without any warranty (see the license). You use
it at your own risk and are responsible for how you use it.

- It only downloads builds of games your own Steam account owns, using your own
  login. It does not bypass DRM or any copy protection.
- Rolling a game back is meant for singleplayer and offline use. Old or modified
  game files can be rejected online, and in games with anti-cheat (such as VAC) they
  may put your account at risk. Do not use downgraded builds in online or
  competitive games.
- Make sure your use follows the Steam Subscriber Agreement.

## License

Steam Downgrader is licensed under the GNU General Public License v2.0 (see
[LICENSE](LICENSE)). You are free to use and modify it, and every copy you pass on
must stay open source under the same license, so it cannot be turned into a
closed-source product. It bundles DepotDownloader (GPL-2.0) and uses SteamKit2
(LGPL-2.1).

## Credits

- [SteamRE/DepotDownloader](https://github.com/SteamRE/DepotDownloader)
- [SteamKit2](https://github.com/SteamRE/SteamKit)
- [SteamDB](https://steamdb.info) for the public build history
- [Tauri](https://tauri.app)
