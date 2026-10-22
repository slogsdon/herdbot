//! `herdbot doctor`: first-run and health checks. The evaluator is pure over `Facts`.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use herdr_client::Client;
use serde::Serialize;

use crate::{home, service};

pub const HARNESSES: [&str; 4] = ["claude", "codex", "cursor-agent", "pi"];

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Fail,
}
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: &'static str,
    pub status: Status,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<&'static str>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Missing,
    Other(PathBuf),
    This,
}
#[derive(Debug, Clone)]
pub struct Facts {
    pub herdr: std::result::Result<String, String>,
    pub harnesses: Vec<String>,
    pub home_exists: bool,
    pub toml_exists: bool,
    pub exe: PathBuf,
    pub plist_exe: Option<String>,
    pub daemon_running: bool,
    pub daemon_version: Option<String>,
    pub version: String,
    pub link: Link,
}
pub fn evaluate(f: &Facts) -> Vec<Check> {
    use Status::{Fail, Ok as Good, Warn};
    let exe = f.exe.to_string_lossy();
    let check = |id, status, message, fix| Check {
        id,
        status,
        message,
        fix,
    };
    let herdr = match &f.herdr {
        std::result::Result::Ok(version) => check("herdr", Good, format!("herdr {version}"), None),
        Err(error) => check(
            "herdr",
            Fail,
            format!("herdr is not reachable ({error}); start herdr"),
            None,
        ),
    };
    let harnesses = if f.harnesses.is_empty() {
        check(
            "harnesses",
            Fail,
            format!("none of {} is on PATH", HARNESSES.join(", ")),
            None,
        )
    } else {
        check(
            "harnesses",
            Good,
            format!("found {}", f.harnesses.join(", ")),
            None,
        )
    };
    let home_check = if f.home_exists && f.toml_exists {
        check("home", Good, home::root().display().to_string(), None)
    } else {
        check(
            "home",
            Warn,
            "the herdbot home or herdbot.toml is missing".into(),
            Some("home"),
        )
    };
    let service_check = match &f.plist_exe {
        None => check(
            "service",
            Fail,
            "the launchd service is not installed".into(),
            Some("service"),
        ),
        Some(path) if path != exe.as_ref() => check(
            "service",
            Fail,
            format!("the service runs {path}, but this binary is {exe}"),
            Some("service"),
        ),
        Some(_) => check("service", Good, "installed".into(), None),
    };
    let daemon_fix = if f.plist_exe.is_some() {
        "restart-daemon"
    } else {
        "service"
    };
    let daemon = match (f.daemon_running, &f.daemon_version) {
        (false, _) => check(
            "daemon",
            Fail,
            "the daemon is not running".into(),
            Some(daemon_fix),
        ),
        (true, Some(version)) if version == &f.version => {
            check("daemon", Good, format!("running {version}"), None)
        }
        (true, Some(version)) => check(
            "daemon",
            Fail,
            format!(
                "the daemon runs {version}, the binary on disk is {}",
                f.version
            ),
            Some("restart-daemon"),
        ),
        (true, None) => check(
            "daemon",
            Fail,
            "the daemon predates version reporting".into(),
            Some("restart-daemon"),
        ),
    };
    let link = match &f.link {
        Link::This => check(
            "cli-link",
            Good,
            "herdbot on PATH is this binary".into(),
            None,
        ),
        Link::Missing => check(
            "cli-link",
            Warn,
            "no herdbot on PATH".into(),
            Some("cli-link"),
        ),
        Link::Other(path) => check(
            "cli-link",
            Warn,
            format!("herdbot on PATH is {}, not this binary", path.display()),
            Some("cli-link"),
        ),
    };
    let location = if f.exe.components().any(|c| c.as_os_str() == "target") {
        check("location", Warn, format!("{exe} is a build output"), None)
    } else {
        check("location", Good, exe.into_owned(), None)
    };
    vec![
        herdr,
        harnesses,
        home_check,
        service_check,
        daemon,
        link,
        location,
    ]
}

pub fn plist_program(text: &str) -> Option<String> {
    let after = text.split("<key>ProgramArguments</key>").nth(1)?;
    let arguments = after.split("</array>").next()?;
    let mut strings = arguments.split("<string>").skip(1).filter_map(|entry| {
        let end = entry.find("</string>")?;
        Some(
            entry[..end]
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        )
    });
    let first = strings.next()?;
    if first.ends_with("/herdbot") {
        return Some(first);
    }
    strings.find_map(|argument| {
        argument
            .split_whitespace()
            .find(|part| part.ends_with("/herdbot"))
            .map(str::to_owned)
    })
}

pub fn render(checks: &[Check]) -> String {
    checks
        .iter()
        .map(|c| {
            let mark = match c.status {
                Status::Ok => "ok  ",
                Status::Warn => "warn",
                Status::Fail => "FAIL",
            };
            let fix = c
                .fix
                .map(|f| format!("  (fix: herdbot setup --only {f})"))
                .unwrap_or_default();
            format!("{mark} {:10} {}{fix}\n", c.id, c.message)
        })
        .collect()
}

pub fn to_json(checks: &[Check]) -> serde_json::Value {
    serde_json::json!({ "v": 1, "version": env!("CARGO_PKG_VERSION"), "checks": checks })
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub async fn gather() -> Facts {
    let exe = std::env::current_exe().unwrap_or_default();
    let herdr = Client::from_env().ping().await.map_err(|e| e.to_string());
    let path_var = crate::shellenv::effective_path(false);
    let on_path = |name: &str| {
        std::env::split_paths(&path_var)
            .map(|dir| dir.join(name))
            .find(|p| is_executable(p))
    };
    let harnesses = HARNESSES
        .iter()
        .filter(|name| on_path(name).is_some())
        .map(|name| (*name).to_owned())
        .collect();
    let link = match on_path("herdbot") {
        None => Link::Missing,
        Some(path) if path.canonicalize().ok() == exe.canonicalize().ok() => Link::This,
        Some(path) => Link::Other(path),
    };
    let daemon_running = std::os::unix::net::UnixStream::connect(home::daemon_lock()).is_ok();
    Facts {
        herdr,
        harnesses,
        home_exists: home::root().is_dir(),
        toml_exists: home::root().join("herdbot.toml").is_file(),
        exe,
        plist_exe: std::fs::read_to_string(service::plist_path())
            .ok()
            .and_then(|text| plist_program(&text)),
        daemon_running,
        daemon_version: home::read_daemon_info().map(|info| info.version),
        version: env!("CARGO_PKG_VERSION").into(),
        link,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn healthy() -> Facts {
        Facts {
            herdr: Ok("0.9.1".into()),
            harnesses: vec!["claude".into()],
            home_exists: true,
            toml_exists: true,
            exe: PathBuf::from("/Applications/Herdbot.app/Contents/Helpers/herdbot"),
            plist_exe: Some("/Applications/Herdbot.app/Contents/Helpers/herdbot".into()),
            daemon_running: true,
            daemon_version: Some("0.1.0".into()),
            version: "0.1.0".into(),
            link: Link::This,
        }
    }
    fn find<'a>(checks: &'a [Check], id: &str) -> &'a Check {
        checks.iter().find(|c| c.id == id).unwrap()
    }
    #[test]
    fn a_healthy_machine_is_all_ok() {
        let checks = evaluate(&healthy());
        assert!(checks.iter().all(|c| c.status == Status::Ok), "{checks:?}");
        let ids: Vec<_> = checks.iter().map(|c| c.id).collect();
        assert_eq!(
            ids,
            [
                "herdr",
                "harnesses",
                "home",
                "service",
                "daemon",
                "cli-link",
                "location"
            ]
        );
    }
    #[test]
    fn herdr_down_fails_without_a_fix() {
        let checks = evaluate(&Facts {
            herdr: Err("connection refused".into()),
            ..healthy()
        });
        let c = find(&checks, "herdr");
        assert_eq!((c.status, c.fix), (Status::Fail, None));
        assert!(c.message.contains("connection refused"));
    }
    #[test]
    fn no_harness_fails_but_a_subset_is_fine() {
        let none = evaluate(&Facts {
            harnesses: vec![],
            ..healthy()
        });
        assert_eq!(find(&none, "harnesses").status, Status::Fail);
        assert_eq!(find(&evaluate(&healthy()), "harnesses").status, Status::Ok);
    }
    #[test]
    fn missing_home_or_toml_warns_with_the_home_fix() {
        for facts in [
            Facts {
                home_exists: false,
                ..healthy()
            },
            Facts {
                toml_exists: false,
                ..healthy()
            },
        ] {
            let checks = evaluate(&facts);
            let c = find(&checks, "home");
            assert_eq!((c.status, c.fix), (Status::Warn, Some("home")));
        }
    }
    #[test]
    fn service_missing_or_pointing_elsewhere_fails_with_the_service_fix() {
        for plist_exe in [None, Some("/old/path/herdbot".to_owned())] {
            let checks = evaluate(&Facts {
                plist_exe,
                ..healthy()
            });
            let c = find(&checks, "service");
            assert_eq!((c.status, c.fix), (Status::Fail, Some("service")));
        }
    }
    #[test]
    fn daemon_down_or_stale_is_fixed_by_restart_or_service() {
        let stopped = evaluate(&Facts {
            daemon_running: false,
            ..healthy()
        });
        assert_eq!(
            (
                find(&stopped, "daemon").status,
                find(&stopped, "daemon").fix
            ),
            (Status::Fail, Some("restart-daemon"))
        );
        let no_service = evaluate(&Facts {
            daemon_running: false,
            plist_exe: None,
            ..healthy()
        });
        assert_eq!(find(&no_service, "daemon").fix, Some("service"));
        let stale = evaluate(&Facts {
            daemon_version: Some("0.0.9".into()),
            ..healthy()
        });
        assert_eq!(
            (find(&stale, "daemon").status, find(&stale, "daemon").fix),
            (Status::Fail, Some("restart-daemon"))
        );
        let old_daemon = evaluate(&Facts {
            daemon_version: None,
            ..healthy()
        });
        assert_eq!(find(&old_daemon, "daemon").status, Status::Fail);
    }
    #[test]
    fn cli_link_missing_or_other_warns_with_the_link_fix() {
        for link in [
            Link::Missing,
            Link::Other(PathBuf::from("/usr/local/bin/herdbot")),
        ] {
            let checks = evaluate(&Facts { link, ..healthy() });
            let c = find(&checks, "cli-link");
            assert_eq!((c.status, c.fix), (Status::Warn, Some("cli-link")));
        }
    }
    #[test]
    fn running_from_a_build_output_warns() {
        let checks = evaluate(&Facts {
            exe: PathBuf::from("/r/herdbot/target/debug/herdbot"),
            ..healthy()
        });
        assert_eq!(find(&checks, "location").status, Status::Warn);
    }
    #[test]
    fn plist_program_reads_what_service_install_writes() {
        let plist = "<key>ProgramArguments</key>\n  <array><string>/Apps/A &amp; B/herdbot</string><string>daemon</string></array>";
        assert_eq!(plist_program(plist).as_deref(), Some("/Apps/A & B/herdbot"));
        assert_eq!(plist_program("<plist/>"), None);
    }
    #[test]
    fn plist_program_reads_a_legacy_shell_wrapper() {
        let plist = "<key>ProgramArguments</key><array><string>/bin/zsh</string><string>-c</string><string>source ~/.zshenv; exec /Users/a/.cargo/bin/herdbot daemon</string></array>";
        assert_eq!(
            plist_program(plist).as_deref(),
            Some("/Users/a/.cargo/bin/herdbot")
        );
    }
    #[test]
    fn json_has_v_version_and_omits_a_missing_fix() {
        let value = to_json(&evaluate(&healthy()));
        assert_eq!(value["v"], 1);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert!(value["checks"][0].get("fix").is_none());
    }
}
