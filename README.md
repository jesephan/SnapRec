# SnapRec

A simple macOS screenshot annotator and screen recorder, written in Rust ([egui](https://github.com/emilk/egui)).

## Features

- **Region screenshots**: drag to select (Space for window mode, Esc to cancel).
- **Annotate**: arrows, boxes and text, with preset colors plus a color picker, adjustable width and text size, and undo.
- **Export**: copy to the clipboard, *Save As…* PNG/JPG to any folder or drive, or *Quick Save* to a default folder.
- **Menu bar app**: a bird icon in the top-right menu bar (it turns red while recording) with every action. Start/stop/restart recording, take a screenshot, toggle audio, open folders, start at login, and quit.
- **Global keyboard shortcuts** for every action. Assign them in Settings by clicking a shortcut and pressing the keys.
  Defaults: ⌃⇧1 screenshot · ⌃⇧2 start/stop recording · ⌃⇧3 record selected area · ⌃⇧4 record entire screen · ⌃⇧5 restart recording · ⌃⇧0 open window.
- **Screen recording without a time limit**, using ScreenCaptureKit. Video streams straight to disk (fragmented MP4, crash-safe).
  - **Entire screen** (default) or **a selected area** that you drag out on screen.
  - **Computer & app audio** (default on), so no virtual audio driver is needed.
  - **MP4**: 5–60 fps, hardware H.264 (HEVC above 4K).
  - **GIF**: choose 1–30 fps to cut frames, and an optional width cap. Uses a two-pass palette for good quality.
- **Start at login**: launches quietly into the menu bar.

## Requirements

- macOS 13+ (Apple Silicon)
- [ffmpeg](https://ffmpeg.org), only for GIF output: `brew install ffmpeg`
- To build: Rust (https://rustup.rs) and Xcode Command Line Tools (`xcode-select --install`)

## Build & install

```bash
./bundle.sh                      # builds dist/SnapRec.app and dist/SnapRec.dmg
cp -R dist/SnapRec.app /Applications/
```

On first use, grant **Screen Recording** permission in
System Settings → Privacy & Security → Screen & System Audio Recording, then restart SnapRec.

## Usage notes

- Closing the window keeps SnapRec running in the menu bar. Use **Quit SnapRec** from the bird menu to exit.
- GIF recordings are captured as MP4 first, then converted when you stop.
- Project layout: `src/` (Rust app), `helper/main.swift` (ScreenCaptureKit recorder and area picker), `tools/make_icons.swift` (generates `assets/`).
- Settings are stored in `~/Library/Application Support/SnapRec/config.json`.

## License

MIT
