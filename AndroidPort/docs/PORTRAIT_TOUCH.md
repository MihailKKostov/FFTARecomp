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

## Measured with the headless runner (USA ROM, new game, 1080x2400)

- Opening scene and the snowball-fight battle render the extended map above
  and below the 240x160 area; the whole small map is visible.
- The battle stats panel is a detached UI layer and appears complete once
  its slide-in animation ends. Frames captured mid-slide show it clipped in
  every aspect, including on the hardware frame; that is not a portrait bug.
- Title screen and the New Game / Saved Game menu have no tap adapter: a tap
  is sent as A, so it confirms the highlighted row, not the tapped row.

## Not measured

- Anything on a real phone: performance in portrait (about 2.7x the pixels
  of a 20:9 landscape view), gestures, the bar, rotation while playing.
- World map, towns, menus, shops and later battles in portrait.

## Next

1. Tap adapters for the title menu, then other unsupported screens.
2. Portrait battle layout: dock dialogue and stats panels to the bottom of
   the screen instead of the middle 240x160 band (`ui_layout.rs`, `touch.rs`).
3. Drag on the battle map pans the game's camera instead of moving the cursor
   (needs the camera variables reverse-engineered).
4. World map and towns: extend or frame them for portrait.
