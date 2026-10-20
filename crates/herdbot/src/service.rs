//! `herdbot service install|uninstall|status`: run the daemon as a per-user launchd agent
//! that starts at login and restarts if it exits.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::home::{self, Result};

const LABEL: &str = "dev.herdbot.daemon";

fn plist_path() -> PathBuf {
    home::home_dir().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

fn domain() -> Result<String> {
    let uid = Command::new("id").arg("-u").output()?;
    Ok(format!("gui/{}", String::from_utf8(uid.stdout)?.trim()))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn launchctl(args: &[&str]) -> Result<String> {
    let out = Command::new("launchctl").args(args).output()?;
    if !out.status.success() {
        return Err(format!(
            "launchctl {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn install() -> Result<String> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Err(format!(
            "{} is a build output; a rebuild would swap the daemon's binary under it. Run `cargo install --path crates/herdbot` and install from ~/.cargo/bin/herdbot",
            exe.display()
        )
        .into());
    }
    // launchd starts agents with a bare PATH; the harness CLIs need this shell's.
    let mut env = vec![
        ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".to_owned(), home::home_dir().to_string_lossy().into_owned()),
        ("HERDBOT_HOME".to_owned(), home::root().to_string_lossy().into_owned()),
    ];
    if let Ok(socket) = std::env::var("HERDR_SOCKET") {
        env.push(("HERDR_SOCKET".to_owned(), socket));
    }
    let log = home::root().join(".state/daemon.log");
    fs::create_dir_all(log.parent().unwrap())?;
    let env_xml: String = env
        .iter()
        .map(|(k, v)| format!("    <key>{k}</key><string>{}</string>\n", escape(v)))
        .collect();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>daemon</string></array>
  <key>EnvironmentVariables</key>
  <dict>
{env_xml}  </dict>
  <key>WorkingDirectory</key><string>{root}</string>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        exe = escape(&exe.to_string_lossy()),
        root = escape(&home::root().to_string_lossy()),
        log = escape(&log.to_string_lossy()),
    );
    let path = plist_path();
    fs::create_dir_all(path.parent().unwrap())?;
    let domain = domain()?;
    // Replace a loaded copy so a reinstall picks up the new plist.
    let _ = launchctl(&["bootout", &format!("{domain}/{LABEL}")]);
    fs::write(&path, plist)?;
    launchctl(&["bootstrap", &domain, &path.to_string_lossy()])?;
    Ok(format!(
        "installed {} (logs: {})",
        path.display(),
        log.display()
    ))
}

pub fn uninstall() -> Result<String> {
    let _ = launchctl(&["bootout", &format!("{}/{LABEL}", domain()?)]);
    let path = plist_path();
    let _ = fs::remove_file(&path);
    Ok(format!("removed {}", path.display()))
}

pub fn status() -> Result<String> {
    let Ok(out) = launchctl(&["print", &format!("{}/{LABEL}", domain()?)]) else {
        return Ok("not installed".into());
    };
    // First of each: nested sections (endpoints) repeat "state =".
    let mut seen = Vec::new();
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|l| {
            let key = l.split(" =").next().unwrap_or_default();
            ["state", "pid", "last exit code"].contains(&key) && !seen.contains(&key) && {
                seen.push(key);
                true
            }
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
