//! Launch-at-login via a per-user LaunchAgent (~/Library/LaunchAgents).
use std::path::PathBuf;

const LABEL: &str = "com.snaprec.app";

fn plist_path() -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join("Library/LaunchAgents")
            .join(format!("{LABEL}.plist")),
    )
}

pub fn is_enabled() -> bool {
    plist_path().map(|p| p.exists()).unwrap_or(false)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Path of the enclosing .app bundle, if we are running from one.
pub fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .find(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        .map(|p| p.to_path_buf())
}

pub fn set_enabled(on: bool) -> Result<(), String> {
    let path = plist_path().ok_or("cannot find home directory")?;
    if !on {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let args: Vec<String> = match bundle_path() {
        Some(app) => vec![
            "/usr/bin/open".into(),
            "-a".into(),
            app.display().to_string(),
            "--args".into(),
            "--hidden".into(),
        ],
        None => vec![std::env::current_exe()
            .map_err(|e| e.to_string())?
            .display()
            .to_string(), "--hidden".into()],
    };
    let args_xml: String = args
        .iter()
        .map(|a| format!("        <string>{}</string>\n", xml_escape(a)))
        .collect();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
{args_xml}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, plist).map_err(|e| e.to_string())
}
