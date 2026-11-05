# CoordsDisplay - coordinate overlay for Queen's Domain

A small [UE4SS](https://github.com/UE4SS-RE/RE-UE4SS) Lua mod that shows the
player's world position (X / Y / Z, plus yaw) in the corner of the screen,
plus the sublevel you're in (e.g. `500_AUT  (Ancient World)`), which is the
same map code as the report's *Map* column.

The numbers are the same Unreal world units that `qd-save-report` prints for
missing collectibles, so you can walk until the overlay matches the report.

| Key | Action |
|-----|--------|
| F6  | Toggle the overlay |
| F7  | Log the current position to the UE4SS console and `positions.txt` |

Keys, refresh rate, position and font size are at the top of
`CoordsDisplay/Scripts/main.lua`.

## Install

1. Download a UE4SS release (v3.x, the `UE4SS_v3.x.x.zip`, not the dev zip) and
   extract it into the game's binaries folder:

   ```
   <Steam>/steamapps/common/QueensDomain/QueensDomain/Binaries/Win64/
   ```

   so that `dwmapi.dll` and the `ue4ss/` folder sit next to
   `QueensDomain-Win64-Shipping.exe`.

2. Copy the `CoordsDisplay` folder from here into `Win64/ue4ss/Mods/`
   (older UE4SS layouts use `Win64/Mods/`):

   ```
   Win64/ue4ss/Mods/CoordsDisplay/enabled.txt
   Win64/ue4ss/Mods/CoordsDisplay/Scripts/main.lua
   ```

   `enabled.txt` turns the mod on. Alternatively add `CoordsDisplay : 1` to
   `ue4ss/Mods/mods.txt`.

3. **Linux / Steam Deck (Proton):** UE4SS loads through a `dwmapi.dll` proxy,
   which Wine ignores unless told otherwise. Set the game's Steam launch options to

   ```
   WINEDLLOVERRIDES="dwmapi=n,b" %command%
   ```

4. Start the game. The overlay appears once you are in the world. If it
   doesn't, enable the console in `ue4ss/UE4SS-settings.ini`
   (`GuiConsoleEnabled = 1`) and look for `[CoordsDisplay]` lines.

## Troubleshooting

* **No overlay but F7 prints positions:** the runtime widget could not be
  created on your UE4SS version. Positions still go to the console and to
  `positions.txt`.
* **Nothing at all:** UE4SS isn't loading. Check `UE4SS.log` next to the exe,
  and on Proton check the `WINEDLLOVERRIDES` launch option.

## Uninstall

Delete `ue4ss/Mods/CoordsDisplay`, or delete `dwmapi.dll` to disable UE4SS
completely. The mod never touches save files.
