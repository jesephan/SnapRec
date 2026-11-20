# SnapRec

A simple macOS screenshot annotator and screen recorder, written in Rust ([egui](https://github.com/emilk/egui)).

## Features

- **Region screenshots**: drag to select (Space for window mode, Esc to cancel).
- **Annotate**: arrows, boxes and text, with preset colors plus a color picker, adjustable width and text size, and undo.
- **Export**: copy to the clipboard, *Save As…* PNG/JPG to any folder or drive, or *Quick Save* to a default folder.
- **Screen recording without a time limit**: video streams straight to disk (fragmented MP4, crash-safe).
  - **MP4**: 5–60 fps, hardware H.264 encoding.
  - **GIF**: choose 1–30 fps to cut frames, and an optional width cap. Uses a two-pass palette for good quality.
- **Start at login**: one checkbox, which adds a per-user LaunchAgent.

## Requirements

- macOS 11+
- [ffmpeg](https://ffmpeg.org) for recording: `brew install ffmpeg`
- Rust (to build): https://rustup.rs

## Build & install

```bash
./bundle.sh                      # builds dist/SnapRec.app and dist/SnapRec.dmg
cp -R dist/SnapRec.app /Applications/
```

On first use, grant **Screen Recording** permission in
System Settings → Privacy & Security → Screen & System Audio Recording, then restart SnapRec.

## Usage notes

- While recording, SnapRec minimizes itself (optional). Click its Dock icon and press **Stop**.
- GIF recordings are captured as MP4 first, then converted when you stop.
- Settings are stored in `~/Library/Application Support/SnapRec/config.json`.

## License

MIT
