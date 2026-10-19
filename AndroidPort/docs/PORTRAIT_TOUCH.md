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
    can be tapped; measured: a tap on the culled flower town sends Marche.
  - Landscape: 2131-frame world map session hash-identical before/after.

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
