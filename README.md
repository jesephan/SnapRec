# SnapRec

A simple macOS screenshot annotator and screen recorder, written in Rust ([egui](https://github.com/emilk/egui)).

## Features

- **Region screenshots**: drag to select (Space for window mode, Esc to cancel).
- **Annotate**: arrows, boxes and text, with preset colors plus a color picker, adjustable width and text size, and undo.
- **Export**: copy to the clipboard, *Save As…* PNG/JPG to any folder or drive, or *Quick Save* to a default folder.
- **Save locations** for screenshots and recordings can be changed in the Screenshot tab, Record tab, Settings, or the *Record as…* dialog.
- **Menu bar app**: a black & white panda icon in the top-right menu bar (it blinks and shows a pulsing red dot while recording) with every action. Start/stop/restart recording, take a screenshot, toggle audio, open folders, start at login, and quit.
- **Global keyboard shortcuts** for every action. Assign them in Settings by clicking a shortcut and pressing the keys.
  Defaults: ⌃⇧1 screenshot · ⌃⇧2 start/stop recording · ⌃⇧3 record selected area · ⌃⇧4 record entire screen · ⌃⇧5 restart recording · ⌃⇧0 open window.
- **Screen recording without a time limit**, using ScreenCaptureKit. Video streams straight to disk (fragmented MP4, crash-safe).
  - **Entire screen** (default) or **a selected area** that you drag out on screen.
  - **Record as… prompt** every time a recording starts (window button, menu bar or shortcut):
    - **Video** → **MP4** (hardware H.264, HEVC above 4K) or **WebM** (VP9 + Opus). Frame rate 5–60 fps is set in the Record tab.
    - **GIF** → **quality** (Low 640 px · Medium 960 px · High 1280 px · Best original size) and **frames per second** (5–30). Uses a two-pass palette.
    - **File name** and **save folder** are asked too; if the file already exists you're asked to confirm replacing it before recording starts.
    - Your last choice is pre-selected next time. *Restart Recording* reuses the current format without asking.
  - **Computer & app audio** (default on, MP4/WebM only), so no virtual audio driver is needed.
- **Start at login**: launches quietly into the menu bar.

## Requirements

- macOS 13+ (Apple Silicon)
- [ffmpeg](https://ffmpeg.org), only for WebM and GIF output: `brew install ffmpeg`
- To build: Rust (https://rustup.rs) and Xcode Command Line Tools (`xcode-select --install`)

## Build & install

```bash
./bundle.sh                      # builds dist/SnapRec.app and dist/SnapRec.dmg
./bundle.sh --install            # …and installs to /Applications and launches it
```

The build signs SnapRec with a local self-signed identity ("SnapRec Local Signing", created
once in your login keychain) so the Screen Recording permission survives rebuilds. If that
isn't possible (e.g. the keychain is locked) it falls back to ad-hoc signing.

### "Apple could not verify SnapRec is free of malware"

SnapRec isn't notarized by Apple, so macOS blocks it when the app or DMG carries the
quarantine flag (downloaded, AirDropped or copied from another Mac). Locally built copies
installed with `./bundle.sh --install` have the flag removed. Otherwise either:

```bash
xattr -dr com.apple.quarantine /Applications/SnapRec.app
```

or open it once, then go to System Settings → Privacy & Security and click **Open Anyway**.

On first use, grant **Screen Recording** permission in
System Settings → Privacy & Security → Screen & System Audio Recording, then restart SnapRec.

## Usage notes

- Closing the window keeps SnapRec running in the menu bar. Use **Quit SnapRec** from the panda menu to exit.
- WebM and GIF recordings are captured as MP4 first, then converted when you stop.
- Project layout: `src/` (Rust app), `helper/main.swift` (ScreenCaptureKit recorder, area picker and "Record as…" prompt), `tools/make_icons.swift` (generates `assets/`).
- Settings are stored in `~/Library/Application Support/SnapRec/config.json`.

## License

MIT
