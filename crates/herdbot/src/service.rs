//! `herdbot service install|disable|enable|uninstall|status`: control the per-user launchd agent
//! that starts at login and restarts if it exits.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::home::{self, Result};

const LABEL: &str = "dev.herdbot.daemon";

pub fn plist_path() -> PathBuf {
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

/// A rebuild would swap a build output's bytes under the running daemon.
pub fn reject_build_output(exe: &Path) -> Result<()> {
    if exe.components().any(|c| c.as_os_str() == "target") {
        return Err(format!(
            "{} is a build output; a rebuild would swap the daemon's binary under it. Run `cargo install --path crates/herdbot` and install from ~/.cargo/bin/herdbot",
            exe.display()
        )
        .into());
    }
    Ok(())
}

/// Restart the daemon. A disabled or unloaded job cannot be kickstarted, so load the installed
/// plist first; `setup --only restart-daemon` then also brings back a daemon the user disabled.
pub fn restart() -> Result<String> {
    let domain = domain()?;
    let job = target(&domain);
    if launchctl(&["print", &job]).is_err() {
        let path = plist_path();
        if path.is_file() {
            let _ = launchctl(&["enable", &job]);
            launchctl(&["bootstrap", &domain, &path.to_string_lossy()])?;
        }
    }
    launchctl(&["kickstart", "-k", &job])?;
    Ok("restarted the daemon".into())
}

fn target(domain: &str) -> String {
    format!("{domain}/{LABEL}")
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
    // Keep the path as invoked so a stable link remains the plist's program path.
    let exe = std::env::current_exe()?;
    reject_build_output(&exe)?;
    // launchd and the menu bar app start with a bare PATH, so record the effective PATH.
    let mut env = vec![
        ("PATH".to_owned(), crate::shellenv::effective_path(true)),
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
    let _ = launchctl(&["bootout", &target(&domain)]);
    fs::write(&path, plist)?;
    // Clear a `service disable` override, or launchd refuses to bootstrap the new plist.
    let _ = launchctl(&["enable", &target(&domain)]);
    launchctl(&["bootstrap", &domain, &path.to_string_lossy()])?;
    Ok(format!(
        "installed {} (logs: {})",
        path.display(),
        log.display()
    ))
}

pub fn uninstall() -> Result<String> {
    let _ = launchctl(&["bootout", &target(&domain()?)]);
    let path = plist_path();
    let _ = fs::remove_file(&path);
    Ok(format!("removed {}", path.display()))
}

/// `launchctl disable` persists in launchd's override database, so a disabled daemon stays off across
/// logins; `bootout` stops it now. The plist and the CLI link stay, so `enable` reloads this install.
fn disable_with(domain: &str, mut run: impl FnMut(&[&str]) -> Result<String>) -> Result<String> {
    let job = target(domain);
    run(&["disable", &job])?;
    if let Err(error) = run(&["bootout", &job]) {
        // "No such process" means the job was already unloaded; that is the wanted state. A failure
        // that leaves the job loaded is real and stays visible.
        if run(&["print", &job]).is_ok() {
            return Err(error);
        }
    }
    Ok("disabled the daemon; the service stays installed, so enable starts it again".into())
}

pub fn disable() -> Result<String> {
    disable_with(&domain()?, launchctl)
}

/// Clear the disable override and load the installed plist unless launchd already has the job.
fn enable_with(
    domain: &str,
    path: &Path,
    mut run: impl FnMut(&[&str]) -> Result<String>,
) -> Result<String> {
    if !path.is_file() {
        return Err(format!(
            "service plist not found at {}; install the service first",
            path.display()
        )
        .into());
    }
    let job = target(domain);
    run(&["enable", &job])?;
    if run(&["print", &job]).is_ok() {
        return Ok("enabled the daemon (already loaded)".into());
    }
    run(&["bootstrap", domain, &path.to_string_lossy()])?;
    Ok("enabled the daemon".into())
}

pub fn enable() -> Result<String> {
    enable_with(&domain()?, &plist_path(), launchctl)
}

/// `state`/`pid`/`last exit code` when launchd has the job, else whether the service is installed.
pub fn status() -> Result<String> {
    status_with(&domain()?, plist_path().is_file(), launchctl)
}

fn status_with(
    domain: &str,
    installed: bool,
    mut run: impl FnMut(&[&str]) -> Result<String>,
) -> Result<String> {
    let Ok(out) = run(&["print", &target(domain)]) else {
        // Installed but unloaded is `service disable` or a job launchd has not loaded yet; the plist
        // is still there, so `service enable` or setup starts it again.
        return Ok(if installed {
            "installed but not running".to_owned()
        } else {
            "not installed".to_owned()
        });
    };
    // First of each: nested sections (endpoints) repeat "state =".
    let mut seen = Vec::new();
    let lines: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| {
            let key = l.split(" =").next().unwrap_or_default();
            ["state", "pid", "last exit code"].contains(&key) && !seen.contains(&key) && {
                seen.push(key);
                true
            }
        })
        .collect();
    Ok(if lines.is_empty() {
        "loaded".to_owned()
    } else {
        lines.join("\n")
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn disable_sets_the_persistent_override_then_stops_the_job() {
        // The install must survive a disable: the plist is what enable loads again, and the CLI link
        // is how the user gets back to `herdbot service enable`.
        let root =
            std::env::temp_dir().join(format!("herdbot-service-disable-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let plist = root.join("dev.herdbot.daemon.plist");
        let link = root.join("herdbot");
        std::fs::write(&plist, "plist").unwrap();
        std::os::unix::fs::symlink("/bin/herdbot", &link).unwrap();

        let mut calls: Vec<Vec<String>> = Vec::new();
        let message = super::disable_with("gui/501", |args| {
            calls.push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(String::new())
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                vec!["disable", "gui/501/dev.herdbot.daemon"],
                vec!["bootout", "gui/501/dev.herdbot.daemon"],
            ]
        );
        assert!(message.contains("stays installed"), "{message}");
        assert!(plist.exists());
        assert!(std::fs::symlink_metadata(&link).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn disable_tolerates_an_unloaded_job_but_reports_a_refused_override() {
        // bootout fails when the job is already out; the persistent override is what disables it.
        let message = super::disable_with("gui/501", |args| match args[0] {
            "bootout" | "print" => Err("Boot-out failed: 3: No such process".into()),
            _ => Ok(String::new()),
        })
        .unwrap();
        assert!(message.contains("stays installed"), "{message}");
        // A launchd refusal of the override itself is a real failure and stays visible.
        let error = super::disable_with("gui/501", |args| {
            if args[0] == "disable" {
                Err("launchctl disable: Operation not permitted".into())
            } else {
                Ok(String::new())
            }
        })
        .unwrap_err();
        assert!(error.to_string().contains("Operation not permitted"));
        // A bootout that fails and leaves the job loaded is not a disable.
        let error = super::disable_with("gui/501", |args| match args[0] {
            "bootout" => Err("Boot-out failed: 1: Operation not permitted".into()),
            _ => Ok(String::new()),
        })
        .unwrap_err();
        assert!(error.to_string().contains("Boot-out failed"));
    }

    #[test]
    fn enable_clears_the_override_and_loads_the_installed_plist() {
        let root =
            std::env::temp_dir().join(format!("herdbot-service-enable-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let plist = root.join("dev.herdbot.daemon.plist");
        std::fs::write(&plist, "plist").unwrap();

        let mut calls: Vec<Vec<String>> = Vec::new();
        let message = super::enable_with("gui/501", &plist, |args| {
            calls.push(args.iter().map(|arg| arg.to_string()).collect());
            match args[0] {
                "print" => Err("Could not find service".into()),
                _ => Ok(String::new()),
            }
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                vec!["enable", "gui/501/dev.herdbot.daemon"],
                vec!["print", "gui/501/dev.herdbot.daemon"],
                vec!["bootstrap", "gui/501", plist.to_str().unwrap()],
            ]
        );
        assert_eq!(message, "enabled the daemon");

        // Already loaded: enable is still cleared, but no second bootstrap that launchd would reject.
        calls.clear();
        let message = super::enable_with("gui/501", &plist, |args| {
            calls.push(args.iter().map(|arg| arg.to_string()).collect());
            Ok(String::new())
        })
        .unwrap();
        assert_eq!(
            calls,
            [
                vec!["enable", "gui/501/dev.herdbot.daemon"],
                vec!["print", "gui/501/dev.herdbot.daemon"],
            ]
        );
        assert!(message.contains("already loaded"), "{message}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enable_needs_the_installed_plist_and_reports_launchctl_errors() {
        let mut calls = 0;
        let error = super::enable_with(
            "gui/501",
            std::path::Path::new("/nonexistent/dev.herdbot.daemon.plist"),
            |_| {
                calls += 1;
                Ok(String::new())
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("install the service first"));
        assert_eq!(calls, 0, "no launchctl call without a plist");

        let plist =
            std::env::temp_dir().join(format!("herdbot-enable-error-{}.plist", std::process::id()));
        std::fs::write(&plist, "plist").unwrap();
        let error = super::enable_with("gui/501", &plist, |args| match args[0] {
            "print" => Err("Could not find service".into()),
            _ => Err("Bootstrap failed: 5: Input/output error".into()),
        })
        .unwrap_err();
        assert!(error.to_string().contains("Bootstrap failed"));
        let _ = std::fs::remove_file(plist);
    }

    #[test]
    fn status_reports_installed_state_and_the_first_of_each_launchd_key() {
        let unloaded = |_args: &[&str]| Err("Could not find service".into());
        assert_eq!(
            super::status_with("gui/501", true, unloaded).unwrap(),
            "installed but not running"
        );
        assert_eq!(
            super::status_with("gui/501", false, unloaded).unwrap(),
            "not installed"
        );
        let printed = "\tstate = running\n\tpid = 42\n\tstate = running\n\tlast exit code = 0\n";
        let out = super::status_with("gui/501", true, |args| {
            assert_eq!(args, ["print", "gui/501/dev.herdbot.daemon"]);
            Ok(printed.to_owned())
        })
        .unwrap();
        assert_eq!(out, "state = running\npid = 42\nlast exit code = 0");
    }

    #[test]
    fn build_outputs_are_rejected_installed_paths_are_not() {
        assert!(super::reject_build_output(std::path::Path::new("/Users/a/Code/herdbot/target/debug/herdbot")).is_err());
        assert!(super::reject_build_output(std::path::Path::new("/Applications/Herdbot.app/Contents/Helpers/herdbot")).is_ok());
        assert!(super::reject_build_output(std::path::Path::new("/opt/homebrew/bin/herdbot")).is_ok());
    }
}
