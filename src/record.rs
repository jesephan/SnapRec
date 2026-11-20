//! Screen recording via the bundled `snaprec-helper` (ScreenCaptureKit).
//! Video (+ optional computer audio) is streamed straight to disk as a
//! fragmented MP4, so recordings have no length limit and survive a crash.
//! GIFs are converted from that MP4 with ffmpeg when recording stops.
use crate::config::{Config, VideoFormat};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

pub const BUNDLE_ID: &str = "com.snaprec.app";

pub fn helper_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join("snaprec-helper");
    p.is_file().then_some(p)
}

pub fn ffmpeg_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = vec![
        "/opt/homebrew/bin/ffmpeg".into(),
        "/usr/local/bin/ffmpeg".into(),
    ];
    if let Ok(path) = std::env::var("PATH") {
        candidates.extend(path.split(':').map(|d| Path::new(d).join("ffmpeg")));
    }
    candidates.into_iter().find(|p| p.is_file())
}

#[derive(Clone, Debug)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub main: bool,
}

pub fn list_displays(helper: &Path) -> Vec<Display> {
    let Ok(out) = Command::new(helper).arg("list-displays").output() else { return vec![] };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|v| Display {
            id: v["id"].as_u64().unwrap_or(0) as u32,
            name: v["name"].as_str().unwrap_or("Display").to_string(),
            width: v["width"].as_u64().unwrap_or(0) as u32,
            height: v["height"].as_u64().unwrap_or(0) as u32,
            main: v["main"].as_bool().unwrap_or(false),
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub display: u32,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

/// Shows a drag-to-select overlay. Blocking; None if cancelled.
pub fn select_region(helper: &Path) -> Option<Region> {
    let out = Command::new(helper).arg("select").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(Region {
        display: v["display"].as_u64()? as u32,
        x: v["x"].as_i64()?,
        y: v["y"].as_i64()?,
        w: v["w"].as_i64()?,
        h: v["h"].as_i64()?,
    })
}

pub struct Recording {
    child: Child,
    pub started: Instant,
    /// File being written right now
    pub writing_path: PathBuf,
    pub final_path: PathBuf,
    pub log_path: PathBuf,
    pub format: VideoFormat,
    pub region: Option<Region>,
    gif_fps: u32,
    gif_width: u32,
}

fn cache_dir() -> PathBuf {
    let d = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("SnapRec");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn log_tail(path: &Path) -> String {
    let s = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = s
        .lines()
        .filter(|l| !l.trim().is_empty() && *l != "started" && *l != "saved")
        .collect();
    lines[lines.len().saturating_sub(4)..].join("\n")
}

pub fn start(
    helper: &Path,
    cfg: &Config,
    display: Option<u32>,
    region: Option<Region>,
) -> Result<Recording, String> {
    std::fs::create_dir_all(&cfg.video_dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let (final_path, writing_path) = match cfg.video_format {
        VideoFormat::Mp4 => {
            let p = cfg.video_dir.join(format!("Recording_{stamp}.mp4"));
            (p.clone(), p)
        }
        VideoFormat::Gif => (
            cfg.video_dir.join(format!("Recording_{stamp}.gif")),
            cfg.video_dir.join(format!("Recording_{stamp}.recording.mp4")),
        ),
    };
    let log_path = cache_dir().join("record.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;

    let fps = match cfg.video_format {
        VideoFormat::Mp4 => cfg.mp4_fps.clamp(1, 60),
        VideoFormat::Gif => cfg.gif_fps.clamp(1, 30),
    };
    let mut cmd = Command::new(helper);
    cmd.arg("record")
        .arg("--out")
        .arg(&writing_path)
        .args(["--fps", &fps.to_string()])
        .args(["--exclude-bundle", BUNDLE_ID]);
    let display = region.map(|r| r.display).or(display);
    if let Some(d) = display {
        cmd.args(["--display", &d.to_string()]);
    }
    if let Some(r) = region {
        cmd.args(["--rect", &format!("{},{},{},{}", r.x, r.y, r.w, r.h)]);
    }
    if cfg.record_audio && cfg.video_format == VideoFormat::Mp4 {
        cmd.arg("--audio");
    }
    if !cfg.capture_cursor {
        cmd.arg("--no-cursor");
    }
    if cfg.half_resolution {
        cmd.arg("--half");
    }
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log2))
        .spawn()
        .map_err(|e| format!("Failed to start recorder: {e}"))?;

    Ok(Recording {
        child,
        started: Instant::now(),
        writing_path,
        final_path,
        log_path,
        format: cfg.video_format,
        region,
        gif_fps: cfg.gif_fps.clamp(1, 30),
        gif_width: cfg.gif_width,
    })
}

impl Recording {
    /// Some(..) if the recorder exited on its own (usually an error).
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(_)) => Some(format!("Recording stopped unexpectedly.\n{}", log_tail(&self.log_path))),
            _ => None,
        }
    }

    fn stop_child(&mut self) -> bool {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        drop(self.child.stdin.take());
        self.child.wait().map(|s| s.success()).unwrap_or(false)
    }

    /// Stop and delete what was recorded.
    pub fn discard(mut self) {
        self.stop_child();
        let _ = std::fs::remove_file(&self.writing_path);
    }

    /// Stops recording and finalizes the file. Blocking: call from a worker thread.
    pub fn finish(mut self, progress: impl Fn(String)) -> Result<PathBuf, String> {
        let ok = self.stop_child();
        if !self.writing_path.exists() {
            return Err(format!("No video was written.\n{}", log_tail(&self.log_path)));
        }
        if !ok {
            progress(format!("Recorder reported a problem: {}", log_tail(&self.log_path)));
        }
        if self.format == VideoFormat::Mp4 {
            return Ok(self.final_path);
        }

        let Some(ffmpeg) = ffmpeg_path() else {
            return Err(format!(
                "ffmpeg not found (brew install ffmpeg) — GIF not created. Video kept at {}",
                self.writing_path.display()
            ));
        };
        let scale = if self.gif_width > 0 {
            format!(",scale='min({},iw)':-2:flags=lanczos", self.gif_width)
        } else {
            String::new()
        };
        let base = format!("fps={}{}", self.gif_fps, scale);
        let palette = cache_dir().join("palette.png");

        progress("Building GIF palette…".into());
        let ok = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&self.writing_path)
            .args(["-vf", &format!("{base},palettegen=stats_mode=diff")])
            .arg(&palette)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err(format!("GIF palette step failed. Video kept at {}", self.writing_path.display()));
        }

        progress("Encoding GIF…".into());
        let ok = Command::new(&ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&self.writing_path)
            .arg("-i")
            .arg(&palette)
            .args([
                "-lavfi",
                &format!("{base}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle"),
                "-loop",
                "0",
            ])
            .arg(&self.final_path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let _ = std::fs::remove_file(&palette);
        if !ok {
            return Err(format!("GIF encoding failed. Video kept at {}", self.writing_path.display()));
        }
        let _ = std::fs::remove_file(&self.writing_path);
        Ok(self.final_path)
    }
}
