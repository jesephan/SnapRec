//! Screen Recording permission (TCC) helpers.

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// True if SnapRec is currently allowed to capture the screen. Never prompts.
pub fn has_screen_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Asks macOS for access (shows the system prompt the first time only).
pub fn request_screen_access() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

pub fn open_settings() {
    let _ = std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn();
}

/// Removes SnapRec's (possibly stale) Screen Recording entry so macOS asks fresh.
pub fn reset() -> Result<(), String> {
    let out = std::process::Command::new("/usr/bin/tccutil")
        .args(["reset", "ScreenCapture", crate::record::BUNDLE_ID])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Launch a fresh copy of the app after this one exits.
pub fn relaunch() {
    if let Some(app) = crate::autostart::bundle_path() {
        let _ = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("sleep 1; /usr/bin/open -n \"$0\"")
            .arg(app)
            .spawn();
    }
}
