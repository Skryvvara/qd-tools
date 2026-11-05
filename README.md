# Please beware that this is vibecoded slop

I didn't anticipate that anyone would show interest in these tools so I let claude
spin this up real fast. I will spend the next few days to clean this up and improve the
user experience.

# Queen's Domain tools

Fan-made helpers for [Queen's Domain](https://store.steampowered.com/app/1799840/)
(Steam app 1799840), for anyone chasing 100% completion.

| Folder | What it is |
|--------|------------|
| [`qd-save-report/`](qd-save-report/) | Command-line tool (Rust) that reads your save folder and writes an HTML report: every achievement, your progress, and exactly what is still missing, with world coordinates. |
| [`qd-coords-mod/`](qd-coords-mod/) | [UE4SS](https://github.com/UE4SS-RE/RE-UE4SS) Lua mod that shows your X / Y / Z position and current map code (e.g. `500_AUT`) in-game, so you can walk to the coordinates from the report. |

The two are meant to be used together: the report says *what* is missing and
*where*, and the overlay tells you when you're there.

## Quick start

1. Download `qd-save-report` for Linux or Windows from the
   [Releases](../../releases) page and unpack it.
2. Run it on your save folder:

   ```
   qd-save-report <SAVE_DIR> -o qd-report.html
   ```

   Your save folder is:

   * **Windows:** `%LOCALAPPDATA%\QueensDomain\Saved\SaveGames\<steamid>`
   * **Linux / Steam Deck (Proton):**
     `<Steam library>/steamapps/compatdata/1799840/pfx/drive_c/users/steamuser/AppData/Local/QueensDomain/Saved/SaveGames/<steamid>`

   The game install is found automatically from your Steam libraries. If it
   isn't, pass `--game <path>`.
3. Open `qd-report.html` in a browser.
4. Optional: install the coordinates mod as described in
   [`qd-coords-mod/README.md`](qd-coords-mod/README.md).

The report tool only reads files. It never modifies your save or the game.

## What the report covers

* All Steam achievements, with progress for the unfinished ones.
* Per achievement, what is still missing: items, Skull Key thresholds, weapons
  left to upgrade, secret walls, endings, and hints for the special cases.
* World coordinates of every uncollected Skull Key, secret wall, weapon,
  throwable, treasure, accessory, upgrade, chest, Orbewa egg and Grimoire ink.

The data comes from the game's own `.pak` (challenge definitions, item data,
shop stock and the actors placed in all world maps), not from a wiki, so it
matches your installed version.

## Building from source

```
cd qd-save-report
cargo build --release
```

The crate has no dependencies. See [`qd-save-report/README.md`](qd-save-report/README.md)
for a static musl build and how the parser works.

Releases are built by GitHub Actions (`.github/workflows/release.yml`): when a
release is published, Linux and Windows binaries are built and attached to it.

## Disclaimer

This is an unofficial fan project and is not affiliated with or endorsed by the
developers or publisher of Queen's Domain. No game files are included in this
repository; the tools read the copy of the game you own.

## License

[MIT](LICENSE)
