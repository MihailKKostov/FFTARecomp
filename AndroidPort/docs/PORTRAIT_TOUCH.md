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
2. World map in portrait: extend the map vertically like battles (needs a save).
3. Drag on the battle map pans the game's camera instead of moving the cursor
   (needs the camera variables reverse-engineered).
4. World map and towns: extend or frame them for portrait.
