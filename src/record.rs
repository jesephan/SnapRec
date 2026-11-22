//! Screen recording via the bundled `snaprec-helper` (ScreenCaptureKit).
//! Video (+ optional computer audio) is streamed straight to disk as a
//! fragmented MP4, so recordings have no length limit and survive a crash.
//! WebM and GIF files are converted from that MP4 with ffmpeg when recording stops.
use crate::config::{Config, GifQuality, VideoFormat};
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

/// What the user picked in the "Record as…" prompt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecordSpec {
    pub format: VideoFormat,
    /// Capture frame rate (for GIF this is also the GIF frame rate)
    pub fps: u32,
    pub gif_quality: GifQuality,
}

impl RecordSpec {
    pub fn summary(&self) -> String {
        match self.format {
            VideoFormat::Gif => format!("GIF · {} fps · {:?} quality", self.fps, self.gif_quality),
            f => format!("{} · {} fps", f.label(), self.fps),
        }
    }
}

/// Shows the native "Record as…" prompt (GIF or Video → MP4/WebM, or GIF
/// quality + fps), pre-filled with the last choice saved in `cfg`. Blocking; None if cancelled.
pub fn choose_format(helper: &Path, cfg: &Config, ffmpeg: bool) -> Option<RecordSpec> {
    let video_fps = cfg.mp4_fps.clamp(1, 60);
    let gif_fps = cfg.gif_fps.clamp(1, 30);
    let mut cmd = Command::new(helper);
    cmd.arg("choose-format")
        .args(["--kind", if cfg.video_format == VideoFormat::Gif { "gif" } else { "video" }])
        .args(["--container", if cfg.video_format == VideoFormat::WebM { "webm" } else { "mp4" }])
        .args(["--quality", cfg.gif_quality.id()])
        .args(["--gif-fps", &gif_fps.to_string()]);
    if !ffmpeg {
        cmd.arg("--no-ffmpeg");
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let format = match v["format"].as_str()? {
        "mp4" => VideoFormat::Mp4,
        "webm" => VideoFormat::WebM,
        "gif" => VideoFormat::Gif,
        _ => return None,
    };
    let spec = match format {
        VideoFormat::Gif => RecordSpec {
            format,
            fps: v["fps"].as_u64().map(|f| f as u32).unwrap_or(gif_fps).clamp(1, 30),
            gif_quality: v["quality"].as_str().and_then(GifQuality::from_id).unwrap_or(cfg.gif_quality),
        },
        _ => RecordSpec { format, fps: video_fps, gif_quality: cfg.gif_quality },
    };
    Some(spec)
}

pub struct Recording {
    child: Child,
    pub started: Instant,
    /// File being written right now
    pub writing_path: PathBuf,
    pub final_path: PathBuf,
    pub log_path: PathBuf,
    pub spec: RecordSpec,
    pub region: Option<Region>,
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
    spec: RecordSpec,
    display: Option<u32>,
    region: Option<Region>,
) -> Result<Recording, String> {
    std::fs::create_dir_all(&cfg.video_dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let final_path = cfg.video_dir.join(format!("Recording_{stamp}.{}", spec.format.ext()));
    let writing_path = if spec.format.needs_ffmpeg() {
        cfg.video_dir.join(format!("Recording_{stamp}.recording.mp4"))
    } else {
        final_path.clone()
    };
    let log_path = cache_dir().join("record.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;

    let fps = match spec.format {
        VideoFormat::Gif => spec.fps.clamp(1, 30),
        _ => spec.fps.clamp(1, 60),
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
    if cfg.record_audio && spec.format != VideoFormat::Gif {
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
        spec,
        region,
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
        let ffmpeg = match self.spec.format {
            VideoFormat::Mp4 => return Ok(self.final_path),
            f => ffmpeg_path().ok_or_else(|| {
                format!(
                    "ffmpeg not found (brew install ffmpeg) — {} not created. Video kept at {}",
                    f.label(),
                    self.writing_path.display()
                )
            })?,
        };
        match self.spec.format {
            VideoFormat::WebM => self.to_webm(&ffmpeg, &progress)?,
            _ => self.to_gif(&ffmpeg, &progress)?,
        }
        let _ = std::fs::remove_file(&self.writing_path);
        Ok(self.final_path)
    }

    fn to_webm(&self, ffmpeg: &Path, progress: &impl Fn(String)) -> Result<(), String> {
        progress("Encoding WebM (VP9)…".into());
        let ok = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&self.writing_path)
            .args([
                "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32",
                "-deadline", "realtime", "-cpu-used", "8", "-row-mt", "1",
                "-pix_fmt", "yuv420p", "-c:a", "libopus", "-b:a", "128k",
            ])
            .arg(&self.final_path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            let _ = std::fs::remove_file(&self.final_path);
            return Err(format!("WebM encoding failed. Video kept at {}", self.writing_path.display()));
        }
        Ok(())
    }

    fn to_gif(&self, ffmpeg: &Path, progress: &impl Fn(String)) -> Result<(), String> {
        let q = self.spec.gif_quality;
        let scale = match q.max_width() {
            0 => String::new(),
            w => format!(",scale='min({w},iw)':-2:flags=lanczos"),
        };
        let base = format!("fps={}{}", self.spec.fps.clamp(1, 30), scale);
        let palette = cache_dir().join("palette.png");

        progress("Building GIF palette…".into());
        let ok = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&self.writing_path)
            .args(["-vf", &format!("{base},palettegen=max_colors={}:stats_mode=diff", q.max_colors())])
            .arg(&palette)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err(format!("GIF palette step failed. Video kept at {}", self.writing_path.display()));
        }

        progress("Encoding GIF…".into());
        let ok = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&self.writing_path)
            .arg("-i")
            .arg(&palette)
            .args([
                "-lavfi",
                &format!("{base}[x];[x][1:v]paletteuse=dither={}:diff_mode=rectangle", q.dither()),
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
        Ok(())
    }
}
