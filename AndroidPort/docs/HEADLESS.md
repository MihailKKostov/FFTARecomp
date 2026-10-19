# Headless runner (Linux/macOS host)

`crates/gba-android/examples/headless.rs` runs the same `Engine` as the APK on
the host, plays a scripted input file and writes PNG screenshots composed the
way `GameSurface` presents them: the world texture scaled into the screen plus
the two native UI layers. It is a development tool only; it is not packaged.

    cargo run --release -p gba-android --example headless -- \
        --rom ffta.gba --sav ffta.sav --script input.txt --out shots \
        --screen 1080x2400 --zoom 0 --touch

`--screen` is the simulated phone surface in pixels (portrait when height is
larger), using the Device aspect mode. Without a native compiler the runner
uses the interpreter (about 100 frames per second on a desktop core). Set
`FFTA_ANDROID_TCC_LIBRARY` to a host-built libtcc to use native blocks.

Script lines, with absolute frame numbers:

    press <frame> <BUTTON[+BUTTON...]> [frames=6]   # A B SELECT START RIGHT LEFT UP DOWN R L
    tap <frame> <x> <y> [back]                       # x/y are 0..1 fractions of the screen
    shot <frame> <name>                              # writes <out>/<name>.png
    end <frame>

ROM, saves, scripts and screenshots are private inputs: keep them out of Git.
