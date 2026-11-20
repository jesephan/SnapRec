use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum VideoFormat {
    Mp4,
    Gif,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub image_dir: PathBuf,
    pub video_dir: PathBuf,
    pub color: [u8; 4],
    pub stroke: f32,
    pub text_size: f32,
    pub video_format: VideoFormat,
    pub mp4_fps: u32,
    pub gif_fps: u32,
    /// 0 = keep original width
    pub gif_width: u32,
    pub half_resolution: bool,
    pub capture_cursor: bool,
    pub minimize_on_record: bool,
    /// Record computer / application audio (MP4 only)
    pub record_audio: bool,
    /// Record the entire screen; when false, ask for an area each time
    pub full_screen: bool,
    pub display_id: Option<u32>,
    /// action id -> shortcut ("" = none)
    pub hotkeys: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            image_dir: dirs::picture_dir().unwrap_or_else(|| home.join("Desktop")),
            video_dir: dirs::video_dir().unwrap_or_else(|| home.join("Movies")),
            color: [230, 40, 40, 255],
            stroke: 6.0,
            text_size: 40.0,
            video_format: VideoFormat::Mp4,
            mp4_fps: 30,
            gif_fps: 10,
            gif_width: 960,
            half_resolution: false,
            capture_cursor: true,
            minimize_on_record: true,
            record_audio: true,
            full_screen: true,
            display_id: None,
            hotkeys: crate::actions::Action::ALL
                .iter()
                .map(|a| (a.id().to_string(), a.default_shortcut().unwrap_or("").to_string()))
                .collect(),
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("SnapRec")
        .join("config.json")
}

impl Config {
    pub fn shortcut(&self, a: crate::actions::Action) -> Option<&str> {
        self.hotkeys.get(a.id()).map(|s| s.as_str()).filter(|s| !s.is_empty())
    }

    pub fn load() -> Self {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = config_path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }
}
