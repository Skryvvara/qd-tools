# qd-save-report

Reads a **Queen's Domain** save folder and writes a single self-contained HTML
report showing:

* every Steam achievement, with progress for the unfinished ones
* exactly what is still missing per achievement: items, Skull Key thresholds,
  weapons left to upgrade (with level and upgrade-data status), the secret
  walls that still count, and hints for the special cases (Ending 4,
  Flying Sword)
* world coordinates of every uncollected Skull Key, counting secret wall,
  weapon, throwable, treasure, accessory, upgrade, chest, Orbewa egg and
  Grimoire ink

Nothing is hard-coded from a wiki. The tool reads the game's own data out of
its `.pak` (challenge definitions, string table, item data, shop stock, and the
placed actors of all 46 world maps) and compares it with the save.

## Usage

```
qd-save-report <SAVE_DIR> [-o report.html] [-g <GAME_DIR|PAK_FILE>]
```

* `SAVE_DIR`: the folder with `World0.sav` and `Challenges`, or a parent folder.
  On Linux/Proton that's
  `~/.local/share/Steam/steamapps/compatdata/1799840/pfx/drive_c/users/steamuser/AppData/Local/QueensDomain/Saved/SaveGames/<steamid>`
  (or the Steam Cloud copy under `userdata/<id>/1799840/remote`).
* The game install is auto-detected from your Steam libraries
  (`libraryfolders.vdf`). Override it with `--game` or `$QD_GAME_DIR`.
* The newest world save (regular or suspend) is used. All saves are listed at
  the bottom of the report.

Example:

```
qd-save-report ../76561198272838960 -o qd-report.html
```

Coordinates in the report are Unreal world units with the sublevel streaming
offsets applied, so they match the in-game overlay from `../qd-coords-mod`.

## Building

No external crates. Standard build:

```
cargo build --release
```

On a machine without a C toolchain (no `cc`), build the static musl binary.
`.cargo/config.toml` makes it link with Rust's bundled `rust-lld`:

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
# -> target/x86_64-unknown-linux-musl/release/qd-save-report
```

## How it works

| Module | What it does |
|--------|--------------|
| `gvas.rs` | UE4 `.sav` (GVAS) tagged property parser |
| `pak.rs` | `.pak` v11 reader (unencrypted index, uncompressed entries) |
| `uasset.rs` | cooked `.uasset`/`.umap` + `.uexp` tagged property parser |
| `game.rs` | extracts achievements (`DefaultEngine.ini`), `FS_ChallengeData`, string table, `FS_ItemData` assets, shop stock, skull door rewards, and every actor with save `GUIDs` in the maps |
| `save.rs` | loads `World*.sav` and `Challenges` |
| `report.rs` | joins the two and renders the HTML |

An actor counts as collected/opened when one of its GUIDs appears in the
save's `SavableActorStates`.
