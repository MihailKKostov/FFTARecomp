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

## 0.48-android-preview

**The laws move on with every day of travel.** The game's ▶ in its law
list marks where its rotation is; it moves on one row a day and the law in
force follows it once ▶ is on it (measured USA: day 1, ▶ on the 20th row,
Items, Shell in force; day 2 ▶ on Shell, still in force; day 4 Protect in
force and marked). The table read on the map keeps the marked row and the
day of the month (EWRAM 0x2fbb). Each day that passes while travelling moves
the shown laws on from it at once, scrolling (a row in 16 frames, faster
when further behind) while the party keeps moving: nothing waits for it,
and the hidden read of the list now starts only 90 frames after the day
last changed, once the map waits for input. Read anew, the list is matched
against the old one turned on: the shown laws keep their place and only
move if the guess was wrong. Measured: day 1 to day 4, Shell, then Poison
as day 3 begins, Protect as day 4 begins; the list read on arrival agrees
(no correction); frame time while travelling unchanged (median 11.8 ms
against 12.0 on the idle map, headless interpreter).

## 0.47-android-preview

**The laws stay shown, and move on.** The laws read last stay on the world
map while the date changes (travelling); before, they were hidden until
read anew for the new date. When the list read anew is the old one turned
on by some laws (most rows match the old list turned by k, k > 0), the
shown laws scroll up through the laws that went to the new law in force,
one native row a frame, each row's indent following its place (none in
the first row, 16 columns from the second). Measured: travelling from day
1 to day 4, Shell, Poison, then Protect in force, 32 frames.

The laws are kept in the resume state, so they are shown from the first
frame after resuming (states without them load as before).

## 0.46-android-preview

**World map laws, rearranged.**
- Three laws are always shown under the top of the frame: the one in
  force, flush left, and the next two, indented 16 columns (as the game
  indents its marked row). The game's own ▶ mark and indent are taken out
  of the rows read, so only the indent tells the law in force. The open
  list continues under them, all indented.
- Forbidden laws are red, recommended ones blue: the pills' palette banks
  (USA, BG2/BG3: 4 dark blue and 10 red for the law in force, 5 cyan and 9
  tan for the others; text in entries 1-3, the pill in 10-15) trade places,
  read from the game's palette during the silent run. The cyan forbidden
  pills become the red one lightened halfway to white, with the tan pill's
  text.
- The Laws button sits under the shown laws (or the open list) on the left.
- Touch-only portrait: the "L Law" and "R Info" hints are not shown (no
  shoulder buttons on the screen), and the map is moved down so its content
  starts under the laws (its first 24 rows are plain parchment and may stay
  under them), never so far that its content goes under the frame's bottom.
  Taps follow the map where it is drawn.

## 0.45-android-preview

**The laws on the world map** (`laws.rs`). Under the top of the portrait
world map, between "L Law" and "R Info", the law in force is always shown
as the game shows it: the law list's title bar ("FORBIDDEN", "RECOMMEND")
and its first row, the forbidden law and the recommended one. A Laws
button above it (game style, highlighted while open) shows the whole list
under it: every law of the rotation in order, each row with both its
forbidden and recommended law (the game shows the recommended one only for
the selected row). A drag scrolls it where it does not fit; a tap off it
(on the map) closes it, a tap on it does nothing.

How the laws are read:
- The game's list (L on the map, USA) is a BG2/BG3 window: title bar
  on native rows 20-31, rows 16 high from row 32, the selected row with its
  recommended law beside it, the pointer a 16x8 sprite (tile 0, priority 0)
  four rows into it. Down walks the rotation and wraps to the first row.
- In a silent run from a snapshot (as the shop reads its rows: 4 frames a
  frame, then the real machine back), L opens the list; each selected row is
  taken once it has been still for 3 frames and differs from the row taken
  before (the list scrolls a few frames after the pointer reaches its last
  row), Down moves on (again after 40 frames if nothing moved), until the
  first row comes back. Only BG2/BG3 are read (`ppu::native_layers`), so the
  rows come without the map. Measured: 20 laws, about 70 frames shown.
- The run is made when the date shown (the HUD's date box) has been the
  same for 8 frames and differs from the one the list was read on, while
  the map waits for input (no game menu, no fade). While the date differs
  nothing is shown (travel). Measured: Kingmoon 1, "Shell"/"Silence";
  after travelling to day 4, "Protect"/"Poison" (as the game's own list).
- Hidden while a game menu or list is open (the game's own L list
  included) and off the world map; fades with the map.

## 0.44-android-preview

**A border between the party and the unit.** Across the screen above the
unit's screen runs the game's own rule from under a unit's header (BG1 tile
0xa8, palette 12: a yellow-green line in a dark outline, read from VRAM,
where it is loaded on every Party screen). The party grid ends above it.

**The Party screens open on the first unit.** Opened from the map, the
roster's cursor goes to unit No. 1 once the roster has settled and A opens
its status screen (a status screen reached first, as when resuming there,
cancels it). Their first frames still read as a unit screen, so opening is
told by the screens appearing, not by the roster.

**Return closes them.** On a unit's status screen (its MENU is the game's
cursor graph of four entries at x 143; Equip and Abilities have theirs at
x 0) the Return button presses B and, once the roster is back, B again.
On Equip, Abilities and the other unit screens Return still goes back to
the status screen, and the device's own B still steps back one screen.

**The roster's hint bar from a status screen.** L Change, Start Item List
and R Sort go back to the roster (B) and press their key once it has
settled for 24 frames (measured: Start ignored 30 frames after B, taken
at 45).

**Swipes between units turn like pages.** On a status screen the engine
follows the finger: a sideways swipe shows the next unit (swiped left) or
the one before (swiped right), once per swipe, with the game's own Right
and Left; a finger lifted without moving is a tap. Before, a drag held the
D-pad direction it pointed to (right showed the next unit). A second finger
ends the swipe and still makes a two-finger tap B (also on shop lists).

## 0.43-android-preview

**Category tabs work after item Info closes.** Item Info leaves WIN1 on but
empty (240-240) when it closes, and the shop took any WIN1 for a window
over the list (the quantity window), so tapping or swiping to another
category did nothing until the shop was left. An empty WIN1 no longer
counts. Measured: Info on and off, then a tap on the Items tab and a swipe
to Armour, both change the list.

**Item Info slides cleanly at the top.** While the window slides, the game
moves it during the frame, its top lines while they are drawn, so a few
of them showed it a step behind (a notch at its top corners, an edge
beside the text). Once the frame is drawn, the panel is redrawn from its
lower rows:
- only their shown columns;
- the top border is the bottom one mirrored (the frame's corner and border
  tiles are vertical flips, with no text);
- rows still showing an edge where the lower rows are cut by WIN1 take
  the lower row's pixels there.

## 0.42-android-preview

**Tapping an item selects it at once.** A tap on a shop list row moves the
game's cursor straight there on the next frame (highlight, stats, Info and
sound are the game's own); a tap on the selected row buys or sells it.
The list handler (USA 0x08029f7c, list owner in r9, cursor at r9+0x4c)
computes the next row into r6 for Down and Up and then sets the cursor
from it; where it does, the tapped row is put in r6 instead. It has two
ways: lists that scroll (0x08029fe2, 0x0802a06c) and lists that fit and
wrap around (0x0802a118, 0x0802a172). Before, the jump only worked until
the handler was compiled (its blocks then start at its entry, now
interpreted from there) and never in lists that fit, so the cursor walked
there a row at a time. Catching up after a fling uses the same jump.
Measured: Bronze Helm to Green Beret and back, Potion to X-Potion, each on
the frame after the tap.

**Item Info opens and closes only at the top.** The Info window slides in
and out over 4 frames, and the game redraws its tiles partway down the
frame, a step ahead of WIN1. Now:
- the list under the window is drawn from the kept maps over the whole
  range the window can cover (x 120-240), also on the first and last
  frames of the slide (Buy/Sell steps 61 and 63) when its tiles are not
  drawn yet or are already gone;
- the top panel takes only window tiles (BG3 palette 11) inside WIN1, and
  window text (BG2 palette 11);
- while it closes, rows still behind the top row are left out;
- its height is read row by row between each row's own edges, so it stays
  the window's text rows while sliding.

Before, the window flashed at its native place over the list as it
opened, and list fragments showed in the top panel as it closed.

**Reviewed, unchanged.**
- The Sell list's "equipped" message breaks up the stats panel for a few
  frames as it slides out; the original does the same.
- The quantity digit highlight stays on the units digit, since touch
  never moves it.
- The scroll arrows hide while the list moves and come back with the
  game's own blinking once it settles.

## 0.41-android-preview

**Quantity arrows.** In the quantity window the game's menu-font `<` and `>`
(cells 92, 93 of its font) sit either side of the number, highlighted while
used. A tap is one up or down; holding repeats after 18 frames, by 1, then
5 (after 75 frames), then 10 (after 150). The number is set through the
window's own Up/Down: where it stores the new number (USA 0x0802a7a4,
0x0802a7e6; r6 its address, shop manager +0x395c +0x79), the wanted number
is stored instead, and the game keeps it within 1 and its most (stock,
Clan Funds at +0x7a) and plays its sound. Measured: taps 1, 2, 3, 2; held
5, 9, then the most (10).

**Shop lists scroll like phone lists** (`shop_scroll.rs`). The game shows
four rows and scrolls a row per Up or Down. Every row is kept as the game
draws it: a silent look-ahead run (machine snapshot, one list step of
hidden frames per frame with item Info closed, restore; in-memory
`savestate::snapshot`/`restore` without the ROM digest) collects the rows
the game has not shown. The list is drawn from them at any offset, following
the finger, flung with friction, rubber-banded past the ends, settled on a
whole row. Settled, the game follows with its own Up/Down, a step every 4
frames, its cursor sound (effect 0x6d at 0x08141540) skipped, the other
panels (stats, Info) held as they were until it is there; its selection is
kept when still shown, else it is the row its cursor reaches first. Buttons
pressed meanwhile wait for it. With item Info open the list's rows come
from what was kept (the Info window overwrites the list's map). Measured:
a fling to the end of an 11-item list, the game there 40 frames later.
The rows start a line above the game's row origin (glyph outlines).

**Swipe between categories.** A sideways swipe on the list (once the finger
clearly moves sideways, where there is a category that way) takes the rows
with the finger; let go past a quarter of the list or fast, the game's own
category change runs, the rows slide out and the new list slides in, the
list window's fill behind; otherwise they spring back. Tapping a category
still works.

**Finger tracking.** The app sends a touch-only finger to the engine
(`gbaPointerDown`/`Move`/`Up`) when the engine claims it on down: shop list
rows, quantity arrows, the party grid. Lifted without moving it is a tap.

**Fixes.** The shop list is recognised while WIN1 stays on after item Info
closes (taps on items work again after turning Info off).

## 0.40-android-preview

**Shop safety (audit phase 1).** The shop's state is read from the game
(gba-core `ffta_shop_mouse::phase`): manager [0x0200f428] +0x148c holds the
running handler (menu 0x08068c11, Buy 0x08069659, Sell 0x0806b6d1; cleared
to 0 when the shop is left) and +0x1490 its step. Resting steps (USA): menu
5; list 23 (60-63 while the game toggles item Info); quantity 43; price
confirmation 102; a message waiting for a button 103 (Thanks) and 123
(cannot sell equipped); anything else is a transition.
- A tap on nothing no longer presses A in portrait shops (it chose items,
  confirmed quantities and completed purchases): only a waiting message
  continues on a tap anywhere. Landscape, without an A button, keeps A.
- Buttons follow the state, never the blinking pointer: list Buy/Sell and
  Return (Info setting at the top right), quantity Buy/Sell and Return,
  price confirmation OK and Cancel, messages OK; none in the menu or while
  it changes.
- Shop list taps use only the game's own list (direct selection); the old
  Up/Down stepping fallback, which could keep pressing for 10 seconds into
  the quantity and confirmation, is gone. Stepping goals end with the Party
  screens they belong to.
- Category tabs work while item Info is open (the Info window's WIN1 is no
  longer taken for the quantity window).
- Measured: 45 stray taps each in the list, the quantity and the price
  confirmation never changed Clan Funds; buttons steady over 200 frames of
  selecting, switching categories and toggling Info.

## 0.39-android-preview

**No "R Info" hint in touch-only shops.** The game's hint (OAM tiles
0x2d4-0x2d9, priority 0, beside the shop picture) is not drawn in portrait
touch-only shops, where the Info button replaces it; the button still
appears exactly when the game offers Info (the hint is read from OAM).

## 0.38-android-preview

**Taps jump straight to their entry, everywhere they used to step.** The
game moves its cursors one entry per press; a tap now makes one press and
has the game's own cursor routine land on the tapped entry, so its sound,
highlight, stats and redraw are the game's own for that entry (measured:
the target is selected 4 frames after the tap).
- Cursor graphs (gba-core `ffta_graph_menu.rs`, USA routine 0x08088fec,
  called every frame with the menu): the unit MENU, the EQUIP slots and
  the ability slots. Entries list their cursor position and Left/Right/
  Up/Down neighbours; where the routine stores the neighbour it moved to,
  the tapped entry is stored. Entries are placed on the screen through the
  visible cursor.
- Text lists of the Party screens (`ffta_row_list.rs`): items (0x08082254)
  and abilities (0x080845b4) move their cursor to a visible row through one
  routine, entered with the tapped row. The lists are read from the game's
  list manager and windows (IWRAM 0x2818, 0x9e0), which also places lists
  without a pointer (the ability sets show only a highlighted row).
- Shop lists use the existing direct selection (`MenuScene::shop_inventory`,
  also while the Info window is open); a tap on the selected item buys.
- Stepping remains only as a fallback where no routine applies.

**Shop Info at the top, as a setting.** An Info button at the top right of
shop lists (in the game's button style, highlighted while on) turns item
Info on or off for every item; it is saved next to the save (`ffta.prefs`)
and applies whenever a shop list is shown. While on, the game's own Info
(R) is kept open: its window (WIN1, BG3 frame corners 0xb00d/0xb00e,
104x144 at 128,8) is captured as its own panel and docked under the
button, down to its last text. The game writes the window over the list's
map, so the list under it is drawn from the BG2/BG3 maps kept while the
window was closed (list rows take their live highlight palette), prices
included. The Move/Jump/Evade panel, whose digits the game removes while
Info is open, stays hidden. Return closes Info before going back. Only in
portrait docking; landscape is unchanged.

**Party grid at the top.** The bar and units start at the top of the frame
(under the touch buttons) instead of being centred.

## 0.37-android-preview

**Return only where the menu has no Leave.** Town scenes show no button row on
their root menus (town_layout, e.g. the shop's Buy/Sell/Leave), which have Leave;
lists and sub-screens keep Return (B). Party screens keep Return.

**Shop lists by touch.** A tap on an item moves the game's cursor to it (Up or
Down until the pointer, OAM tile 0x120, rests on the row) without choosing
it, so its stats and Info show that item; a tap on the selected item chooses
it (A) and the game asks the quantity. The row is Buy (Sell in the sell list,
A on the selected item and then on the quantity), Info (R, while the game
shows its "R Info" hint) and Return. Measured: tap Green Beret, its stats
show; Info opens and closes its abilities; Buy, quantity, purchase
(5000 -> 4200).

**Party grid as on the roster.** The units keep the roster's own spacing: four
to a row 48 apart, rows 40 apart, every other row 24 to the left, under the
hint bar, the bar and units centred in the upper half. A party taller than the
half scrolls. Cells overlap as on the roster; a tap on a "No." label picks its
unit, elsewhere the unit whose sprite is nearest.

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
  row (four beyond nine units), centred and spread over the upper half
  (0.37: the roster's own spacing, see above).
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
