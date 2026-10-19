# Portrait and touch-only roadmap (Android only)

Goal: a portrait mode that feels native (repositioned UI, pannable battle
map) and a touch-only control scheme. Desktop code under `crates/` at the
repository root is not changed; only `AndroidPort/` is.

## Done (0.26-android-preview)

- **Linux/cloud builds.** `scripts/build_android.py` runs on Linux, and
  `.claude/hooks/session-start.sh` installs the SDK, NDK and Rust target in
  cloud sessions. The debug key is in `android/app/debug.keystore` so every
  preview APK updates the previous one in place (saves are kept).
- **Headless runner.** See `HEADLESS.md`. Renders any screen size from scripted
  buttons/taps, with an `FFTA_UI_DEBUG` dump of UI layout values, the raw world
  texture, the 240x160 hardware frame and BG registers.
- **Touch-only mode** (Input > Touchscreen). Hides the gamepad and enables tap
  selection; a one-finger drag holds the D-pad direction of its offset, a
  quick two-finger tap presses B, a slim top-right bar has B, Start, Select,
  L, R, fast forward and Pause.
- **Orientation** (Video & zoom): Landscape (default), Portrait, Auto-rotate.
  Portrait always uses the full-screen Device view (256 wide, up to 800 tall).

## Done (0.27-android-preview)

- **Save export/import.** Start screen: Export save / Import save; pause menu:
  Export save. Import keeps `ffta.sav.bak` and reloads a loaded game.
- **Fast forward draws only the frame it shows** (`MemMap::skip_render`).
  Snowball battle, portrait, 4x, host native code: 49.4 -> 23.7 ms per 4-frame
  batch; all 11500 presented frames hash-identical with skipping on and off.
- **Separate portrait touch layout** with defaults in the lower screen.
- **Portrait battles dock UI panels** to a frame from the top safe area (below
  the touch-only bar) to above the gamepad (33% reserved) or the bottom inset
  (`ui_layout::Band::docked`, `Engine::ui_band`). Other scenes keep the band.
- **Padded scenes in portrait/square** (world map, menus, title) show only the
  240x160 picture fitted to the screen width over a blurred, dimmed copy of it;
  the 8 wrapped side columns are dropped. `Engine::set_view` maps taps to the
  same rectangle GameSurface draws.
- **Title screen taps (USA).** A tap on "Push START" presses Start; on the
  New Game / Saved Game / Color Mode menu a tap moves to the tapped row with
  the game's own Up/Down and presses A; taps between rows do nothing
  (`title_touch.rs`, RAM addresses documented there). Measured: tapping
  Saved Game opens the Resume Battle / Load menu.

## Done (0.28-android-preview)

- **Portrait shop** (`touch::pieces`, `Layout::split`): town/site canvases are
  split at empty rows/columns; the trader text stays docked on top, the main
  lower block (Buy/Sell/Leave) sits lower right with an 8% inset at 80% of the
  frame, smaller blocks (Clan Funds) in the bottom right corner. Shop list
  canvases align with the native picture (their values show through holes).
- **Portrait world map** (`world_map.rs`, gba-core `world_hud.rs`), only when
  the view is at least 360 rows tall:
  - BG0/BG1 are 256x512; the map (rows 0..352) is rendered beyond the native
    rows and the texture is shifted so it stays still while BG0VOFS scrolls.
  - The game culls town objects near its camera. 32x32 priority-2 objects
    stable for 30 frames are remembered (decoded pixels) and drawn while
    culled; forgotten after 20 frames missing inside the native view.
  - HUD (BG2/BG3 + priority-0 tiles 40-4b and 5e-71) is lifted out of the
    picture and pinned under the top safe area/bar and above the gamepad or
    bottom inset. While BG2/BG3 draw between rows 32-128 (town menus), the HUD
    stays in the picture so translucent menus compose as usual.
  - Host sprite copies are not drawn in the extended rows (stale positions).
  - World taps accept any map row (Y 22-330), so towns off the native screen
    can be tapped (from 0.30 the first tap only moves the cursor).
  - Landscape: 2131-frame world map session hash-identical before/after.

## Done (0.29-android-preview)

- **Shop buy/sell lists dock to the bottom in portrait** so the shop artwork
  stays visible. Their stat values are host-drawn digit objects (OBJ tiles
  0x2c0-0x2d3, priority 0) that the UI capture now takes with the canvas
  (`ppu.rs shop_numeral`); tiles 0x00-0x19 priority 2 are the characters.
  A separate dialogue-sized block at the canvas top (the trader's reply)
  stays docked on top. Measured: Down and taps on the moved list update the
  values; a tap on an equipped item gets the game's "can't sell" reply.

## Portrait audit (0.30-android-preview)

Frame-by-frame captures (every 6-8 frames, touch-only, 1080x2400) from the
user's save: world map menus, Party/status, Shop with a full purchase, Pub
(Rumors, lists), Prison. Artefacts found and what changed:

1. Remembered towns stayed bright during fades -> stored as OBJ palette
   indices and coloured with the current palette and BLDCNT brightness.
2. Pinned HUD showed as black shapes during fades on a fixed-colour margin ->
   margin is a deep shade of the game's backdrop colour (fades with it).
3. HUD jumped into the map whenever a menu opened, and 4. a town menu's first
   opening frame was pinned at the top -> only BG2/BG3 pixels inside the
   measured HUD boxes are lifted (world_hud.rs HUD_BOXES); menus never are.
5. Menus flashed at their native left position for ~4 frames before moving
   (the game draws them before the classifier recognises the town menu),
6. Clan Funds jumped between positions, 7. lists flashed mid-screen before
   docking and 8. Clan Funds was pushed off the right edge -> docked
   placement is decided only by a block's native position (touch.rs
   dock_for, ui_layout.rs Layout::docked): blocks in the top rows dock to the
   top, narrow lower-left blocks (menus) to the thumb zone, everything else
   to the bottom at its native x. Shop lists keep: list bottom, reply top.
9. The 8 extra columns of the 256-wide portrait view showed wrapped scenery
   (Prison edge, world map strips) and the UI used a 4.5x scale over a 4.22x
   picture -> portrait still renders 256 columns (scenes need the margin) but
   shows only the central 240 across the screen width (`Engine::set_view`,
   GameSurface and TouchSelection use the same crop).
10. The blurred backdrop showed hard stripes (each native row stretched ~15x)
   -> the engine writes a 30x20 grid of 8x8 averages into unused padding rows
   and GameSurface stretches it with linear filtering.
11. Between town menus (menu closing, list opening, shop fade-in) the game is
   in no recognised menu state, so its UI fell back into the picture at its
   native rows (mid-screen in portrait) for 5-10 frames -> in portrait the
   whole town scene is recognised by its BG control (4c03 4e02 0b08 0a08,
   the values MenuScene::layout_menus requires) and its UI stays docked.
12. The trader bubble's closing animation (rows 13-67) docked to the bottom,
   and a shop list's opening animation (rows 6-29 growing to 6-85) docked to
   the top, then jumped -> a bubble is a block touching a screen edge and
   ending above row 72; in shops everything else docks to the bottom.
13. Buy/Sell/Leave (and Pub, Prison menus) appeared at the bottom left while
   sliding in or out -> the sliding block keeps the translation of the
   complete menu's thumb placement (`Dock::Follow`), so it slides in and out
   of its own left edge as on the original screen.
14. Stat digits the shop parks beside the screen (x -24 and 288) showed:
   in portrait as "0"/"4" floating in the thumb zone, in landscape as digit
   columns in the wide margins -> UI blocks outside the 240 columns are not
   placed, and those digit objects are not drawn outside the native columns.
15. The quantity prompt drew parked BG2/BG3 tiles below the native rows ->
   a docked town scene's menu planes are not drawn in the extended rows.

Verified frame by frame (1080x2400, every 1-8 frames) on the shop purchase,
Pub (Rumors), Prison and Party flows, plus landscape shop frames.

## 0.36-android-preview

**Resume where the game was left.** gba-core `savestate.rs` stores the whole
machine between frames: CPU (all banks), every memory region and I/O
register, timers, DMA, interrupt and video timing, PSG channels and FIFOs,
the backup chip and RTC, and the battle map origins; the engine adds the
portrait caches (remembered towns, the party picture). Measured: running on
from a restored state gives identical memory and screens to running on
uninterrupted (world map, Party screens, mid-battle; interpreted and native),
and the first frame after a relaunch is the frame that was left.
`ffta.resume` (next to `ffta.sav`) is written before any changed battery
save, every 600 frames, and on pause, onStop, exit and errors, through a
synced temp file and a rename. On launch the game continues from it; an
unusable one is kept as `.resume.bad` and the battery save is used. Import
save removes it. Measured: an in-game save made 200 frames before a crash
(no checkpoint) is there after the relaunch.

**Game-style buttons in the bottom HUD row** (`game_buttons.rs`): the menu
window's bar, translucent body and item gem with the label in the game's menu
font and colours (font and palettes read from the ROM; the five window tiles
are byte copies of what the game loads into VRAM). World map: Menu (Start; B
while a menu is open, then highlighted). Party screens: Return (B), the unit
screen sits one row higher. Town scenes: Return (B) and, while the shop shows
its "R Info" hint (OAM tiles 0x2d4-0x2d9), Info (R, opens and closes the item
info). In town scenes the docked windows move up one row to keep the row free.

**Party grid:** three to a row in the roster's zig-zag (every other row half
a step right), rows spread to fill the upper half; a party taller than the
half scrolls with a drag on the grid (`gbaDragStarts`/`gbaDrag`).

**Direct unit selection.** The selection is a row and column (EWRAM 0xfd2a,
0xfd29; index = row * 4 + column at 0xfd3e, recomputed every frame by the
routine at 0x08072f22). On the roster a tap sets them and the game's cursor
glides straight to the unit, then A; unit screens only switch on Left/Right,
so the unit next to the target is set and one Right (Left) switches straight
to it with the game's own single slide.

**Party screens as one layout (0.35)** (`menu_layout.rs`). The upper half
always shows the whole party; the lower half the selected unit (the roster's
unit panel, or the unit's status, Equip, Abilities, Jobs screen in one piece,
docked to the bottom).
- The game only draws the party on the roster. Each unit's cell (sprite and
  "No." label, neighbours' labels masked out) and the hint bar are kept from
  the settled roster (bar drawn, and BG text between bar and panel present:
  the game erases the labels first when it leaves) and laid out three to a
  row (four beyond nine units), centred and spread over the upper half.
- The game's selection ellipse is captured as its own picture (semi-
  transparent OBJ, raw colours) and blended under the unit selected now
  (EWRAM 0xfd3e, also on unit screens).
- The roster's unit-panel portrait (priority 1) always belongs to the panel,
  also host sprite copies, so it is never split across the screen (fixes the
  cut portrait on the roster).
- Taps: a unit on the roster moves the game's cursor there one press at a
  time (each waits until the cursor rests) and opens it with A; on unit
  screens the game's own Left/Right switches units, the shorter way round.
  The hint bar's thirds press L, Start and R. Menu and list rows
  (MENU, EQUIP, ITEMS, ABILITIES) move the game's cursor (OAM tiles
  0x120-0x127) with Up/Down to the tapped row and press A.
- Audited frame by frame: fade in and out, cursor moves, roster to status
  and back, Equip and Items lists, menu taps, unit switching by tap.
  Landscape unchanged.

**In-game Menu button (0.34).** The world map Menu button is now part of
the game picture, built only from the game's own resources (`map_menu.rs`):
the menu window's bar, translucent body (BLDALPHA) and item gem tiles, which
stay in BG VRAM on the world map (char base 0x8000, tiles 1 and 9-c, palettes
12/13), and "Menu" in the game's menu font (USA ROM 0x438630: 16x16 cells,
digits, A-Z, a-z; the same tiles the game copies for "Party", "Area List").
The text is white like menu items and the highlighted item's orange while a
menu is open. A tap on it presses Start, or B while a menu is open. Font and
tiles are checked against measured bytes; if they differ (other ROMs), the
app's own overlay button below is used instead.

**World map Menu button (0.33).** Touch-only portrait: a Menu button (Start)
in the bottom HUD row between Clan Funds (native x 3-77) and the date
(152-240), where the HUD is pinned below the map (`Engine::map_menu_button`,
`MapMenuButton`). Shown only while the map waits for input: hidden while a
menu or list is open (BG2/BG3 drawing in rows 32-128, `world_hud.menu`;
Start does not close the game's menu) and during fades (the game fades by
rewriting the palette, so the HUD's brightness must be unchanged for 8
frames; BLDY fades too). Landscape keeps Start on the touch-only bar (the
same spot lies over the map there).

**Portrait Party screens (0.32).** Roster, unit status, Equip/Items, Pick
Abilities, Change Jobs and the roster's Item List (gba-core `menu_screen.rs`,
gba-android `menu_layout.rs`):
- BG0 is a checkered pattern (period 32x16) over its whole 256x512 map; the
  tall view is filled with it (BG0 only, wrapping like the hardware).
- Per native row the PPU composes the background alone and two groups over
  it; a group's pixels are where it differs from the background, so
  translucent panels keep their blending. The groups move by whole pattern
  periods (16 rows), where the background underneath is identical.
- Unit screens (BG2CNT 2b0a): upper group = BG1 above row 61 (header and its
  separator) and sprite pictures centred above it; BG2/BG3 (stats, menus,
  Equip/Items/ability panels) always in the lower group, so a panel that
  overlaps the separator moves with its list and the separator stays whole.
  Roster (2b09): bar and unit grid above row 100, unit panel below. Item List
  (BG3 title bar in tile rows 0-1): one group, kept together until a unit
  header appears (it erases its title bar first when closing).
- Sprite pictures = same-priority objects whose boxes overlap or touch;
  each goes to the group of its centre row, so portraits are never split.
- The upper group is docked under the top bar, the lower group to the bottom
  (menus in thumb reach). Taps undo the group's move; taps in the gap do
  nothing.
- Audited frame by frame (1080x2400): fade in/out, roster cursor, roster to
  status, Equip, Items, back, Pick Abilities and its list, Change Jobs and
  the job wheel, Left/Right unit switching, Item List in and out, Sort.
  Fixed: body rows jumping from under the header while the header slid in
  (template now from BG2CNT), black rows above the screen (BG0 wraps),
  Item List icons flying to the top while it closed.
- Not done: tap targets for the Party menus (a tap still presses A, as in
  landscape).

**World map margins (0.31).** Above and below the map the parchment
continues instead of a dark margin: the map's outermost 6 rows are mirrored
outward in tiles (each shifted sideways, two tilings cross-faded half a tile
apart so no seam shows) and blend into their average colour within 48 rows.
They are rendered pixels, so fades and transitions darken them with the map
(`world_map.rs extend_paper`).

**World map taps (0.30).** A tap on a place other than the cursor's only
moves the cursor there (the game shows its name); a tap within 12 pixels of
the cursor moves Marche there (`ffta_battle_mouse::world_select`).
Measured: first tap on Marche's town moves the cursor, second tap travels.

## Measured with the headless runner (USA ROM, new game, 1080x2400)

- Opening scene and the snowball-fight battle render the extended map above
  and below the 240x160 area; the whole small map is visible.
- The battle stats panel is a detached UI layer and appears complete once
  its slide-in animation ends. Frames captured mid-slide show it clipped in
  every aspect, including on the hardware frame; that is not a portrait bug.
- (0.26) Title menu taps confirmed the highlighted row; fixed in 0.27.

## Not measured

- Anything on a real phone: performance in portrait (about 2.7x the pixels
  of a 20:9 landscape view), gestures, the bar, rotation while playing.
- World map, towns, menus, shops and later battles in portrait.

## Next

1. Tap adapters for the Resume Battle / Load menu and save slots, then other
   unsupported screens (needs the user's save).
2. Status/party/equipment screens: per-screen portrait recipes (header on top,
   panels stacked and enlarged); town menu on the world map in thumb reach.
3. Drag on the battle map pans the game's camera instead of moving the cursor
   (needs the camera variables reverse-engineered).
4. World map and towns: extend or frame them for portrait.
