//! Menu bar (status bar) bird icon with all actions.
use crate::actions::Action;
use std::collections::HashMap;
use std::str::FromStr;
use tray_icon::menu::{accelerator::Accelerator, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub const ID_AUDIO: &str = "opt_audio";
pub const ID_FULL: &str = "opt_full_screen";
pub const ID_LOGIN: &str = "opt_login";

fn icon(png: &[u8]) -> Icon {
    let img = image::load_from_memory(png).expect("icon png").to_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).expect("icon rgba")
}

pub struct Tray {
    tray: TrayIcon,
    items: HashMap<Action, MenuItem>,
    audio: CheckMenuItem,
    full: CheckMenuItem,
    login: CheckMenuItem,
    idle_icon: Icon,
    rec_icon: Icon,
    recording: bool,
}

impl Tray {
    pub fn new(audio_on: bool, full_on: bool, login_on: bool) -> Result<Self, String> {
        let menu = Menu::new();
        let mut items = HashMap::new();
        let mk = |a: Action, items: &mut HashMap<Action, MenuItem>| {
            let label = match a {
                Action::ToggleRecording => "Start Recording",
                _ => a.label(),
            };
            let it = MenuItem::with_id(a.id(), label, true, None);
            items.insert(a, it.clone());
            it
        };
        let sep = PredefinedMenuItem::separator;
        let audio = CheckMenuItem::with_id(ID_AUDIO, "Record Computer Audio", true, audio_on, None);
        let full = CheckMenuItem::with_id(ID_FULL, "Record Entire Screen by Default", true, full_on, None);
        let login = CheckMenuItem::with_id(ID_LOGIN, "Start at Login", true, login_on, None);

        menu.append_items(&[
            &mk(Action::Screenshot, &mut items),
            &sep(),
            &mk(Action::ToggleRecording, &mut items),
            &mk(Action::RecordFullScreen, &mut items),
            &mk(Action::RecordRegion, &mut items),
            &mk(Action::RestartRecording, &mut items),
            &sep(),
            &audio,
            &full,
            &sep(),
            &mk(Action::ShowWindow, &mut items),
            &mk(Action::OpenScreenshots, &mut items),
            &mk(Action::OpenRecordings, &mut items),
            &login,
            &sep(),
            &mk(Action::Quit, &mut items),
        ])
        .map_err(|e| e.to_string())?;
        // ToggleAudio is handled by the check item; keep a hidden handle for shortcuts.
        items.insert(Action::ToggleAudio, MenuItem::with_id("_unused_audio", "", false, None));

        let idle_icon = icon(include_bytes!("../assets/tray.png"));
        let rec_icon = icon(include_bytes!("../assets/tray_rec.png"));
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(idle_icon.clone())
            .with_icon_as_template(true)
            .with_tooltip("SnapRec")
            .build()
            .map_err(|e| e.to_string())?;

        let t = Self { tray, items, audio, full, login, idle_icon, rec_icon, recording: false };
        t.set_recording(false, None);
        Ok(t)
    }

    pub fn set_recording(&self, recording: bool, elapsed_secs: Option<u64>) {
        if let Some(it) = self.items.get(&Action::ToggleRecording) {
            it.set_text(match elapsed_secs {
                Some(s) if recording => format!("Stop Recording  ({:02}:{:02}:{:02})", s / 3600, (s / 60) % 60, s % 60),
                _ if recording => "Stop Recording".into(),
                _ => "Start Recording".into(),
            });
        }
        for a in [Action::RecordFullScreen, Action::RecordRegion] {
            if let Some(it) = self.items.get(&a) {
                it.set_enabled(!recording);
            }
        }
        if let Some(it) = self.items.get(&Action::RestartRecording) {
            it.set_enabled(recording);
        }
    }

    /// Swap the icon only when the state changes.
    pub fn set_recording_icon(&mut self, recording: bool) {
        if self.recording == recording {
            return;
        }
        self.recording = recording;
        let _ = if recording {
            self.tray.set_icon_with_as_template(Some(self.rec_icon.clone()), false)
        } else {
            self.tray.set_icon_with_as_template(Some(self.idle_icon.clone()), true)
        };
        let _ = self.tray.set_tooltip(Some(if recording { "SnapRec — recording" } else { "SnapRec" }));
    }

    pub fn set_shortcut(&self, a: Action, shortcut: Option<&str>) {
        let accel = shortcut.and_then(|s| Accelerator::from_str(s).ok());
        if a == Action::ToggleAudio {
            let _ = self.audio.set_accelerator(accel);
        } else if let Some(it) = self.items.get(&a) {
            let _ = it.set_accelerator(accel);
        }
    }

    pub fn set_audio(&self, on: bool) {
        self.audio.set_checked(on);
    }
    pub fn set_full(&self, on: bool) {
        self.full.set_checked(on);
    }
    pub fn set_login(&self, on: bool) {
        self.login.set_checked(on);
    }
}
