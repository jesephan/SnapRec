//! Screen recording via ffmpeg (avfoundation). Video is streamed straight to
//! disk as fragmented MP4, so recordings have no length limit and survive a
//! crash. On stop it is remuxed (MP4) or converted (GIF).
use crate::config::{Config, VideoFormat};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

pub fn ffmpeg_path() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    // Bundled copy inside SnapRec.app/Contents/Resources (optional)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(macos) = exe.parent() {
            candidates.push(macos.join("../Resources/ffmpeg"));
        }
    }
    candidates.push("/opt/homebrew/bin/ffmpeg".into());
    candidates.push("/usr/local/bin/ffmpeg".into());
    if let Ok(path) = std::env::var("PATH") {
        for d in path.split(':') {
            candidates.push(Path::new(d).join("ffmpeg"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Returns (avfoundation index, name) for each capturable screen.
pub fn list_screens(ffmpeg: &Path) -> Vec<(String, String)> {
    let out = Command::new(ffmpeg)
        .args(["-hide_banner", "-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .stdin(Stdio::null())
        .output();
    let Ok(out) = out else { return vec![] };
    let text = String::from_utf8_lossy(&out.stderr);
    let mut screens = Vec::new();
    for line in text.lines() {
        if let Some(pos) = line.find("Capture screen") {
            let head = &line[..pos];
            if let (Some(l), Some(r)) = (head.rfind('['), head.rfind(']')) {
                if l < r {
                    let idx = head[l + 1..r].trim().to_string();
                    if idx.chars().all(|c| c.is_ascii_digit()) {
                        screens.push((idx, line[pos..].trim().to_string()));
                    }
                }
            }
        }
    }
    screens
}

pub struct Recording {
    child: Child,
    pub started: Instant,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub log_path: PathBuf,
    pub format: VideoFormat,
    gif_fps: u32,
    gif_width: u32,
    ffmpeg: PathBuf,
}

fn log_dir() -> PathBuf {
    let d = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("SnapRec");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn log_tail(path: &Path) -> String {
    let s = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(4)..].join("\n")
}

pub fn start(ffmpeg: &Path, cfg: &Config, screen_idx: &str) -> Result<Recording, String> {
    std::fs::create_dir_all(&cfg.video_dir).map_err(|e| format!("Cannot create folder: {e}"))?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let ext = match cfg.video_format {
        VideoFormat::Mp4 => "mp4",
        VideoFormat::Gif => "gif",
    };
    let final_path = cfg.video_dir.join(format!("Recording_{stamp}.{ext}"));
    let temp_path = cfg.video_dir.join(format!("Recording_{stamp}.recording.mp4"));
    let log_path = log_dir().join("ffmpeg-record.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;

    let fps = match cfg.video_format {
        VideoFormat::Mp4 => cfg.mp4_fps.clamp(1, 60),
        VideoFormat::Gif => cfg.gif_fps.clamp(1, 30),
    };
    let scale = if cfg.half_resolution {
        "scale=trunc(iw/4)*2:trunc(ih/4)*2"
    } else {
        "scale=trunc(iw/2)*2:trunc(ih/2)*2"
    };
    let vf = format!("fps={fps},{scale},format=yuv420p");
    let input_rate = if fps > 30 { "60" } else { "30" };

    let child = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(["-f", "avfoundation", "-pixel_format", "nv12"])
        .args(["-capture_cursor", if cfg.capture_cursor { "1" } else { "0" }])
        .args(["-capture_mouse_clicks", if cfg.capture_cursor { "1" } else { "0" }])
        .args(["-framerate", input_rate])
        .args(["-i", &format!("{screen_idx}:none")])
        .args(["-vf", &vf])
        .args(["-c:v", "h264_videotoolbox", "-b:v", "12M", "-g", &(fps * 2).to_string()])
        .args(["-movflags", "+frag_keyframe+empty_moov+default_base_moof"])
        .arg(&temp_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("Failed to start ffmpeg: {e}"))?;

    Ok(Recording {
        child,
        started: Instant::now(),
        temp_path,
        final_path,
        log_path,
        format: cfg.video_format,
        gif_fps: cfg.gif_fps.clamp(1, 30),
        gif_width: cfg.gif_width,
        ffmpeg: ffmpeg.to_path_buf(),
    })
}

impl Recording {
    /// Some(..) if ffmpeg exited on its own (usually an error).
    pub fn exited(&mut self) -> Option<String> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(format!(
                "Recording stopped unexpectedly ({status}).\n{}",
                log_tail(&self.log_path)
            )),
            _ => None,
        }
    }

    /// Stops ffmpeg and finalizes the file. Blocking: call from a worker thread.
    /// `progress` receives status text.
    pub fn finish(mut self, progress: impl Fn(String)) -> Result<PathBuf, String> {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q");
            let _ = stdin.flush();
        }
        drop(self.child.stdin.take());
        let _ = self.child.wait();

        if !self.temp_path.exists() {
            return Err(format!("No video was written.\n{}", log_tail(&self.log_path)));
        }

        match self.format {
            VideoFormat::Mp4 => {
                progress("Finalizing MP4…".into());
                let ok = Command::new(&self.ffmpeg)
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(&self.temp_path)
                    .args(["-c", "copy", "-movflags", "+faststart"])
                    .arg(&self.final_path)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    let _ = std::fs::remove_file(&self.temp_path);
                } else {
                    // Fragmented MP4 is still a valid, playable file.
                    std::fs::rename(&self.temp_path, &self.final_path).map_err(|e| e.to_string())?;
                }
                Ok(self.final_path)
            }
            VideoFormat::Gif => {
                let scale = if self.gif_width > 0 {
                    format!(",scale='min({},iw)':-1:flags=lanczos", self.gif_width)
                } else {
                    String::new()
                };
                let base = format!("fps={}{}", self.gif_fps, scale);
                let palette = log_dir().join("palette.png");

                progress("Building GIF palette…".into());
                let ok = Command::new(&self.ffmpeg)
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(&self.temp_path)
                    .args(["-vf", &format!("{base},palettegen=stats_mode=diff")])
                    .arg(&palette)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
                if !ok {
                    return Err(format!(
                        "GIF palette step failed. Raw video kept at {}",
                        self.temp_path.display()
                    ));
                }

                progress("Encoding GIF…".into());
                let ok = Command::new(&self.ffmpeg)
                    .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                    .arg(&self.temp_path)
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
                    return Err(format!(
                        "GIF encoding failed. Raw video kept at {}",
                        self.temp_path.display()
                    ));
                }
                let _ = std::fs::remove_file(&self.temp_path);
                Ok(self.final_path)
            }
        }
    }
}
