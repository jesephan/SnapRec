//! Every user-triggerable action, with labels and default global shortcuts.
//! Shortcut strings use the global-hotkey / muda syntax, e.g. "Ctrl+Shift+Digit1".

use eframe::egui;
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Screenshot,
    ToggleRecording,
    RecordFullScreen,
    RecordRegion,
    RestartRecording,
    ToggleAudio,
    ShowWindow,
    OpenScreenshots,
    OpenRecordings,
    Quit,
}

impl Action {
    pub const ALL: &'static [Action] = &[
        Action::Screenshot,
        Action::ToggleRecording,
        Action::RecordFullScreen,
        Action::RecordRegion,
        Action::RestartRecording,
        Action::ToggleAudio,
        Action::ShowWindow,
        Action::OpenScreenshots,
        Action::OpenRecordings,
        Action::Quit,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Action::Screenshot => "screenshot",
            Action::ToggleRecording => "toggle_recording",
            Action::RecordFullScreen => "record_full",
            Action::RecordRegion => "record_region",
            Action::RestartRecording => "restart_recording",
            Action::ToggleAudio => "toggle_audio",
            Action::ShowWindow => "show_window",
            Action::OpenScreenshots => "open_screenshots",
            Action::OpenRecordings => "open_recordings",
            Action::Quit => "quit",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Screenshot => "Take Screenshot",
            Action::ToggleRecording => "Start / Stop Recording",
            Action::RecordFullScreen => "Record Entire Screen",
            Action::RecordRegion => "Record Selected Area…",
            Action::RestartRecording => "Restart Recording (discard current)",
            Action::ToggleAudio => "Record Computer Audio (on/off)",
            Action::ShowWindow => "Open SnapRec Window",
            Action::OpenScreenshots => "Open Screenshots Folder",
            Action::OpenRecordings => "Open Recordings Folder",
            Action::Quit => "Quit SnapRec",
        }
    }

    pub fn default_shortcut(self) -> Option<&'static str> {
        match self {
            Action::Screenshot => Some("Ctrl+Shift+Digit1"),
            Action::ToggleRecording => Some("Ctrl+Shift+Digit2"),
            Action::RecordRegion => Some("Ctrl+Shift+Digit3"),
            Action::RecordFullScreen => Some("Ctrl+Shift+Digit4"),
            Action::RestartRecording => Some("Ctrl+Shift+Digit5"),
            Action::ShowWindow => Some("Ctrl+Shift+Digit0"),
            _ => None,
        }
    }
}

/// Human-friendly form: "Ctrl+Shift+Digit1" -> "⌃⇧1"
pub fn pretty(shortcut: &str) -> String {
    let mut mods = String::new();
    let mut key = String::new();
    for part in shortcut.split('+') {
        match part.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => mods.push('⌃'),
            "ALT" | "OPTION" => mods.push('⌥'),
            "SHIFT" => mods.push('⇧'),
            "CMD" | "COMMAND" | "SUPER" => mods.push('⌘'),
            _ => {
                key = match part {
                    "ArrowUp" => "↑".into(),
                    "ArrowDown" => "↓".into(),
                    "ArrowLeft" => "←".into(),
                    "ArrowRight" => "→".into(),
                    "Space" => "Space".into(),
                    p => p.trim_start_matches("Key").trim_start_matches("Digit").to_string(),
                }
            }
        }
    }
    // macOS order: ⌃⌥⇧⌘
    let order = ['⌃', '⌥', '⇧', '⌘'];
    let mut sorted: String = order.iter().filter(|c| mods.contains(**c)).collect();
    sorted.push_str(&key);
    sorted
}

/// Convert an egui key + modifiers into a shortcut string. Requires at least
/// one modifier unless the key is a function key.
pub fn from_egui(key: egui::Key, m: egui::Modifiers) -> Option<String> {
    let name = key.name();
    let token = if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) {
        format!("Key{name}")
    } else if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) {
        format!("Digit{name}")
    } else if name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit()) && name.len() > 1 {
        name.to_string()
    } else {
        use egui::Key::*;
        match key {
            Space => "Space",
            ArrowUp => "ArrowUp",
            ArrowDown => "ArrowDown",
            ArrowLeft => "ArrowLeft",
            ArrowRight => "ArrowRight",
            Minus => "Minus",
            Equals => "Equal",
            Comma => "Comma",
            Period => "Period",
            Slash => "Slash",
            Backslash => "Backslash",
            Semicolon => "Semicolon",
            Quote => "Quote",
            OpenBracket => "BracketLeft",
            CloseBracket => "BracketRight",
            Backtick => "Backquote",
            Enter => "Enter",
            _ => return None,
        }
        .to_string()
    };
    let is_fkey = token.starts_with('F') && token.len() > 1 && token[1..].chars().all(|c| c.is_ascii_digit());
    let has_mod = m.ctrl || m.alt || m.mac_cmd || m.command && !cfg!(target_os = "macos");
    if !has_mod && !is_fkey {
        return None;
    }
    let mut parts = vec![];
    if m.ctrl {
        parts.push("Ctrl");
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    if m.mac_cmd {
        parts.push("Cmd");
    }
    parts.push(&token);
    Some(parts.join("+"))
}
