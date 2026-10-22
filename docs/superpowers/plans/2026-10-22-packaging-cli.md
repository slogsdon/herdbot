# Packaging CLI (plan 1 of 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `herdbot --version`, a stable recorded binary path, and the `doctor`, `setup` and `uninstall` commands, so a fresh install is diagnosed and fixed from the CLI.

**Architecture:** Spec: `docs/superpowers/specs/2026-10-22-packaging-design.md` (read its "CLI additions"). `doctor.rs` has a pure `evaluate(&Facts) -> Vec<Check>` plus an I/O `gather()`, so the rules are unit-tested with no machine state. `setup.rs` runs fixes named by the checks (`home`, `cli-link`, `service`, `restart-daemon`). The daemon writes `.state/daemon.json`. Plans 2 (app) and 3 (release) consume `doctor --json` and `setup --only`.

**Tech Stack:** Rust 2024, the existing `herdbot`, `herdbot-core`, `herdr-client` crates, `serde_json`. No new dependencies. Tests are inline `#[cfg(test)]` over pure functions (the repo style); no environment mutation in tests.

## Global Constraints

- macOS only; no new crates.
- `doctor --json` shape: `{"v":1,"version":"<semver>","checks":[{"id","status":"ok|warn|fail","message","fix"?}]}`. `fix` is omitted when none. `v` stays 1; new fields are additive.
- Check ids (fixed): `herdr`, `harnesses`, `home`, `service`, `daemon`, `cli-link`, `location`. Fix ids (fixed): `home`, `cli-link`, `service`, `restart-daemon`. (`login-item` is app-only and not in the CLI.)
- A harness the user doesn't use is never reported; only "none found" is a `fail`.
- The recorded binary path is `std::env::current_exe()` unresolved (verified on macOS: it returns the invoked absolute path, symlink not resolved). Never `canonicalize` it, except to compare two paths.
- `service install` still refuses a path with a `target` component.
- **Live checks must never run `service install`, `service uninstall`, `setup` without `--only`, or `uninstall` against the real launchd.** They would replace the running daemon `dev.herdbot.daemon`. Live checks use `setup --only home` and `--only cli-link` with a temp `HOME`, `HERDBOT_HOME` and `HERDBOT_LINK_DIR`.
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.

**Branch/worktree:** `git -C ~/Code/herdbot worktree add .worktrees/packaging-cli -b feature/packaging-cli` (from `main`). Run all commands from that worktree.

## File map

- Modify `crates/herdbot-core/src/home.rs`: `DaemonInfo`, its path and read/write.
- Modify `crates/herdbot/src/service.rs`: no canonicalize, `reject_build_output`, `pub plist_path`, `restart`.
- Modify `crates/herdbot/src/daemon.rs`: write `daemon.json` after taking the lock.
- Create `crates/herdbot/src/doctor.rs`: `Facts`, `Check`, `evaluate`, `gather`, `render`, `to_json`, `plist_program`.
- Create `crates/herdbot/src/setup.rs`: templates, `link_dir`, fixes, `run`, `uninstall`, `create_bot`.
- Modify `crates/herdbot/src/main.rs`: `mod doctor; mod setup;`, USAGE lines, the `--version`, `doctor`, `setup`, `uninstall` arms, `new` uses `setup::create_bot`.
- Modify `README.md`: Quick start and command list.

---

### Task 1: `--version`, stable recorded path, `daemon.json`

**Files:**
- Modify: `crates/herdbot-core/src/home.rs` (append after `read_inflight`, before `#[cfg(test)]`)
- Modify: `crates/herdbot/src/service.rs:40-49`, `crates/herdbot/src/service.rs:11` (`plist_path` pub)
- Modify: `crates/herdbot/src/daemon.rs:41`
- Modify: `crates/herdbot/src/main.rs` (USAGE, match)

**Interfaces:**
- Produces: `home::DaemonInfo { version: String, exe: String, pid: u32 }`, `home::daemon_info_path() -> PathBuf`, `home::write_daemon_info(&DaemonInfo) -> Result<()>`, `home::read_daemon_info() -> Option<DaemonInfo>`; `service::reject_build_output(&Path) -> Result<()>`; `service::plist_path() -> PathBuf` (now `pub`); `service::restart() -> Result<String>`.

- [ ] **Step 1: Failing test for `reject_build_output`.** Append to `service.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_outputs_are_rejected_installed_paths_are_not() {
        assert!(reject_build_output(Path::new("/Users/a/Code/herdbot/target/debug/herdbot")).is_err());
        assert!(reject_build_output(Path::new("/Applications/Herdbot.app/Contents/Helpers/herdbot")).is_ok());
        assert!(reject_build_output(Path::new("/opt/homebrew/bin/herdbot")).is_ok());
    }
}
```

Run: `cargo test -p herdbot service::tests` → FAIL (not defined).

- [ ] **Step 2: Implement.** In `service.rs` add `use std::path::{Path, PathBuf};` (replace the `PathBuf` import), make `fn plist_path` `pub fn plist_path`, and replace the top of `install()`:

```rust
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

pub fn restart() -> Result<String> {
    launchctl(&["kickstart", "-k", &format!("{}/{LABEL}", domain()?)])?;
    Ok("restarted the daemon".into())
}

pub fn install() -> Result<String> {
    // The path as invoked (a link such as /opt/homebrew/bin/herdbot stays a link), so an update
    // that replaces the target does not change what the plist and the panes recorded.
    let exe = std::env::current_exe()?;
    reject_build_output(&exe)?;
```

(delete the old `canonicalize` and inline `target` check). Run `cargo test -p herdbot service::tests` → PASS.

- [ ] **Step 3: `DaemonInfo` in core.** Append to `home.rs` before the tests module:

```rust
/// What the running daemon reports about itself; `doctor` compares `version` with the binary on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonInfo {
    pub version: String,
    pub exe: String,
    pub pid: u32,
}

pub fn daemon_info_path() -> PathBuf {
    root().join(".state").join("daemon.json")
}

pub fn write_daemon_info(info: &DaemonInfo) -> Result<()> {
    write_json(&daemon_info_path(), info)
}

pub fn read_daemon_info() -> Option<DaemonInfo> {
    read_json(&daemon_info_path())
}
```

Add a test in `home.rs` `mod tests`: round-trip `serde_json::from_str::<DaemonInfo>(&serde_json::to_string(&info).unwrap())` equals `info`. Run `cargo test -p herdbot-core` → PASS.

- [ ] **Step 4: daemon writes it.** In `daemon.rs::run`, directly after `let _lock = take_lock()?;`:

```rust
    if let Err(e) = home::write_daemon_info(&home::DaemonInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        exe: std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        pid: std::process::id(),
    }) {
        eprintln!("daemon.json: {e}");
    }
```

- [ ] **Step 5: `--version`.** In `main.rs` add to the `match command.as_str()`: `"--version" | "-V" | "version" => { println!("herdbot {}", env!("CARGO_PKG_VERSION")); Ok(()) }`, and a USAGE line `  herdbot --version`.

- [ ] **Step 6: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`; `./target/debug/herdbot --version` prints `herdbot 0.1.0`.

- [ ] **Step 7: Commit.**

```bash
git add crates/herdbot-core/src/home.rs crates/herdbot/src/service.rs crates/herdbot/src/daemon.rs crates/herdbot/src/main.rs
git commit -m "feat: --version, unresolved recorded binary path and daemon.json"
```

---

### Task 2: `herdbot doctor`

**Files:**
- Create: `crates/herdbot/src/doctor.rs`
- Modify: `crates/herdbot/src/main.rs` (`mod doctor;`, USAGE, arm)

**Interfaces:**
- Consumes: `service::plist_path()`, `home::read_daemon_info()`, `home::daemon_lock()`, `herdr_client::Client::ping()`.
- Produces: `doctor::{Status, Check, Link, Facts}`, `doctor::evaluate(&Facts) -> Vec<Check>`, `doctor::gather().await -> Facts`, `doctor::render(&[Check]) -> String`, `doctor::to_json(&[Check]) -> serde_json::Value`, `doctor::plist_program(&str) -> Option<String>`, `doctor::HARNESSES`.

- [ ] **Step 1: Failing tests.** Create `doctor.rs` with only the types, `evaluate` as `todo!()`, and this test module (the `healthy()` helper is the all-ok baseline each test mutates):

```rust
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
        assert_eq!(ids, ["herdr", "harnesses", "home", "service", "daemon", "cli-link", "location"]);
    }

    #[test]
    fn herdr_down_fails_without_a_fix() {
        let checks = evaluate(&Facts { herdr: Err("connection refused".into()), ..healthy() });
        let c = find(&checks, "herdr");
        assert_eq!((c.status, c.fix), (Status::Fail, None));
        assert!(c.message.contains("connection refused"));
    }

    #[test]
    fn no_harness_fails_but_a_subset_is_fine() {
        let none = evaluate(&Facts { harnesses: vec![], ..healthy() });
        assert_eq!(find(&none, "harnesses").status, Status::Fail);
        let one = evaluate(&healthy());
        assert_eq!(find(&one, "harnesses").status, Status::Ok);
    }

    #[test]
    fn missing_home_or_toml_warns_with_the_home_fix() {
        for facts in [Facts { home_exists: false, ..healthy() }, Facts { toml_exists: false, ..healthy() }] {
            let checks = evaluate(&facts);
            let c = find(&checks, "home");
            assert_eq!((c.status, c.fix), (Status::Warn, Some("home")));
        }
    }

    #[test]
    fn service_missing_or_pointing_elsewhere_fails_with_the_service_fix() {
        for plist_exe in [None, Some("/old/path/herdbot".to_owned())] {
            let checks = evaluate(&Facts { plist_exe, ..healthy() });
            let c = find(&checks, "service");
            assert_eq!((c.status, c.fix), (Status::Fail, Some("service")));
        }
    }

    #[test]
    fn daemon_down_or_stale_is_fixed_by_restart_or_service() {
        let stopped = evaluate(&Facts { daemon_running: false, ..healthy() });
        assert_eq!((find(&stopped, "daemon").status, find(&stopped, "daemon").fix), (Status::Fail, Some("restart-daemon")));
        let no_service = evaluate(&Facts { daemon_running: false, plist_exe: None, ..healthy() });
        assert_eq!(find(&no_service, "daemon").fix, Some("service"));
        let stale = evaluate(&Facts { daemon_version: Some("0.0.9".into()), ..healthy() });
        assert_eq!((find(&stale, "daemon").status, find(&stale, "daemon").fix), (Status::Fail, Some("restart-daemon")));
        let old_daemon = evaluate(&Facts { daemon_version: None, ..healthy() });
        assert_eq!(find(&old_daemon, "daemon").status, Status::Fail);
    }

    #[test]
    fn cli_link_missing_or_other_warns_with_the_link_fix() {
        for link in [Link::Missing, Link::Other(PathBuf::from("/usr/local/bin/herdbot"))] {
            let checks = evaluate(&Facts { link, ..healthy() });
            let c = find(&checks, "cli-link");
            assert_eq!((c.status, c.fix), (Status::Warn, Some("cli-link")));
        }
    }

    #[test]
    fn running_from_a_build_output_warns() {
        let checks = evaluate(&Facts { exe: PathBuf::from("/r/herdbot/target/debug/herdbot"), ..healthy() });
        assert_eq!(find(&checks, "location").status, Status::Warn);
    }

    #[test]
    fn plist_program_reads_what_service_install_writes() {
        let plist = "<key>ProgramArguments</key>\n  <array><string>/Apps/A &amp; B/herdbot</string><string>daemon</string></array>";
        assert_eq!(plist_program(plist).as_deref(), Some("/Apps/A & B/herdbot"));
        assert_eq!(plist_program("<plist/>"), None);
    }

    #[test]
    fn json_has_v_version_and_omits_a_missing_fix() {
        let value = to_json(&evaluate(&healthy()));
        assert_eq!(value["v"], 1);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert!(value["checks"][0].get("fix").is_none());
    }
}
```

Run: `cargo test -p herdbot doctor::tests` → FAIL.

- [ ] **Step 2: Types and `evaluate`.**

```rust
//! `herdbot doctor`: first-run and health checks. `evaluate` is pure over `Facts`, so the rules are
//! unit-tested; `gather` probes the machine. The app reads `doctor --json` (spec: packaging design).

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

fn check(id: &'static str, status: Status, message: impl Into<String>, fix: Option<&'static str>) -> Check {
    Check { id, status, message: message.into(), fix }
}

pub fn evaluate(f: &Facts) -> Vec<Check> {
    use Status::*;
    let exe = f.exe.to_string_lossy();
    let herdr = match &f.herdr {
        Ok(v) => check("herdr", Ok, format!("herdr {v}"), None),
        Err(e) => check("herdr", Fail, format!("herdr is not reachable ({e}); start herdr"), None),
    };
    let harnesses = if f.harnesses.is_empty() {
        check("harnesses", Fail, format!("none of {} is on PATH", HARNESSES.join(", ")), None)
    } else {
        check("harnesses", Ok, format!("found {}", f.harnesses.join(", ")), None)
    };
    let home = if f.home_exists && f.toml_exists {
        check("home", Ok, format!("{}", home::root().display()), None)
    } else {
        check("home", Warn, "the herdbot home or herdbot.toml is missing", Some("home"))
    };
    let service = match &f.plist_exe {
        None => check("service", Fail, "the launchd service is not installed", Some("service")),
        Some(p) if p.as_str() != exe => check(
            "service",
            Fail,
            format!("the service runs {p}, but this binary is {exe}"),
            Some("service"),
        ),
        Some(_) => check("service", Ok, "installed", None),
    };
    let restart_or_install = if f.plist_exe.is_some() { "restart-daemon" } else { "service" };
    let daemon = match (f.daemon_running, &f.daemon_version) {
        (false, _) => check("daemon", Fail, "the daemon is not running", Some(restart_or_install)),
        (true, Some(v)) if *v == f.version => check("daemon", Ok, format!("running {v}"), None),
        (true, Some(v)) => check(
            "daemon",
            Fail,
            format!("the daemon runs {v}, the binary on disk is {}", f.version),
            Some("restart-daemon"),
        ),
        (true, None) => check("daemon", Fail, "the daemon predates version reporting", Some("restart-daemon")),
    };
    let link = match &f.link {
        Link::This => check("cli-link", Ok, "herdbot on PATH is this binary", None),
        Link::Missing => check("cli-link", Warn, "no herdbot on PATH", Some("cli-link")),
        Link::Other(p) => check(
            "cli-link",
            Warn,
            format!("herdbot on PATH is {}, not this binary", p.display()),
            Some("cli-link"),
        ),
    };
    let location = if f.exe.components().any(|c| c.as_os_str() == "target") {
        check("location", Warn, format!("{exe} is a build output"), None)
    } else {
        check("location", Ok, exe.into_owned(), None)
    };
    vec![herdr, harnesses, home, service, daemon, link, location]
}
```

(`use Status::*` shadows `Ok` inside the function; if the compiler complains about `Ok(v)` in the `match &f.herdr`, write `Result::Ok`/`Status::Ok` explicitly. Keep the behaviour, fix the spelling.)

- [ ] **Step 3: `plist_program`, `render`, `to_json`.**

```rust
/// The first `ProgramArguments` entry of the plist `service install` writes.
pub fn plist_program(text: &str) -> Option<String> {
    let after = text.split("<key>ProgramArguments</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = start + after[start..].find("</string>")?;
    Some(after[start..end].replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
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
            let fix = c.fix.map(|f| format!("  (fix: herdbot setup --only {f})")).unwrap_or_default();
            format!("{mark} {:10} {}{fix}\n", c.id, c.message)
        })
        .collect()
}

pub fn to_json(checks: &[Check]) -> serde_json::Value {
    serde_json::json!({ "v": 1, "version": env!("CARGO_PKG_VERSION"), "checks": checks })
}
```

Run `cargo test -p herdbot doctor::tests` → PASS.

- [ ] **Step 4: `gather`.** Add (not unit-tested; it only reads the machine, and Step 6 exercises it):

```rust
fn is_executable(path: &Path) -> bool {
    path.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

pub async fn gather() -> Facts {
    let exe = std::env::current_exe().unwrap_or_default();
    let herdr = Client::from_env().ping().await.map_err(|e| e.to_string());
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let on_path = |name: &str| {
        std::env::split_paths(&path_var).map(|dir| dir.join(name)).find(|p| is_executable(p))
    };
    let harnesses = HARNESSES.iter().filter(|h| on_path(h).is_some()).map(|h| (*h).to_owned()).collect();
    let link = match on_path("herdbot") {
        None => Link::Missing,
        Some(p) if p.canonicalize().ok() == exe.canonicalize().ok() => Link::This,
        Some(p) => Link::Other(p),
    };
    let daemon_running = std::os::unix::net::UnixStream::connect(home::daemon_lock()).is_ok();
    Facts {
        herdr,
        harnesses,
        home_exists: home::root().is_dir(),
        toml_exists: home::root().join("herdbot.toml").is_file(),
        exe,
        plist_exe: std::fs::read_to_string(service::plist_path()).ok().and_then(|t| plist_program(&t)),
        daemon_running,
        daemon_version: home::read_daemon_info().map(|i| i.version),
        version: env!("CARGO_PKG_VERSION").into(),
        link,
    }
}
```

- [ ] **Step 5: CLI arm.** In `main.rs`: `mod doctor;`, USAGE line `  herdbot doctor [--json]               first-run and health checks (exit 1 if any fail)`, and:

```rust
        "doctor" => {
            let checks = doctor::evaluate(&doctor::gather().await);
            if args.iter().any(|a| a == "--json") {
                println!("{}", doctor::to_json(&checks));
            } else {
                print!("{}", doctor::render(&checks));
            }
            if checks.iter().any(|c| c.status == doctor::Status::Fail) {
                std::process::exit(1);
            }
            Ok(())
        }
```

- [ ] **Step 6: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`. Then, read-only against the real machine (no fixes): `./target/debug/herdbot doctor` and `./target/debug/herdbot doctor --json | python3 -m json.tool`. Expected on the dev machine: `herdr` ok, `harnesses` ok, `location` warn (build output), `service` fail (the plist points at `~/.cargo/bin/herdbot`, not this build), `cli-link` warn. Report what it prints.

- [ ] **Step 7: Commit.**

```bash
git add crates/herdbot/src/doctor.rs crates/herdbot/src/main.rs
git commit -m "feat: herdbot doctor"
```

---

### Task 3: `herdbot setup`

**Files:**
- Create: `crates/herdbot/src/setup.rs`
- Modify: `crates/herdbot/src/main.rs` (`mod setup;`, USAGE, arm, `new` arm)

**Interfaces:**
- Consumes: `doctor::{gather, evaluate, Check, Status}`, `service::{install, restart}`, `route::table()`, `home::{root, bot_dir, save_bot, Bot, list_bots, slug}`.
- Produces: `setup::create_bot(name: &str, cwd: Option<String>, class: Option<String>, instructions: &str) -> Result<PathBuf>`, `setup::herdbot_toml_template() -> &'static str`, `setup::link_dir(...)`, `setup::run(only: Option<String>, yes: bool).await -> Result<()>`.

- [ ] **Step 1: Failing tests.** Create `setup.rs` with the test module and stubbed functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toml_template_parses_and_enables_nothing() {
        let table: toml::Table = herdbot_toml_template().parse().unwrap();
        assert!(table.is_empty(), "every setting ships commented out: {table:?}");
        assert!(herdbot_toml_template().contains("[approvals]"));
    }

    #[test]
    fn link_dir_prefers_a_writable_dir_on_path() {
        let candidates = [PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin"), PathBuf::from("/home/u/.local/bin")];
        let writable = |p: &Path| p != Path::new("/opt/homebrew/bin");
        assert_eq!(
            link_dir(&candidates, "/usr/bin:/usr/local/bin", writable),
            Some((PathBuf::from("/usr/local/bin"), true))
        );
        // none on PATH: the first writable one, flagged off-PATH
        assert_eq!(
            link_dir(&candidates, "/usr/bin", writable),
            Some((PathBuf::from("/usr/local/bin"), false))
        );
        assert_eq!(link_dir(&candidates, "/usr/bin", |_| false), None);
    }

    #[test]
    fn fix_order_and_dedup() {
        let checks = vec![
            Check { id: "daemon", status: Status::Fail, message: String::new(), fix: Some("restart-daemon") },
            Check { id: "service", status: Status::Fail, message: String::new(), fix: Some("service") },
            Check { id: "home", status: Status::Warn, message: String::new(), fix: Some("home") },
            Check { id: "cli-link", status: Status::Ok, message: String::new(), fix: None },
        ];
        assert_eq!(pending_fixes(&checks), ["home", "service", "restart-daemon"]);
    }

    #[test]
    fn create_bot_writes_the_template_and_refuses_a_duplicate() {
        let root = std::env::temp_dir().join(format!("herdbot-setup-test-{}", std::process::id()));
        let dir = bot_files(&root, "helper", "Be helpful.").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("BOT.md")).unwrap(), "Be helpful.");
        assert!(bot_files(&root, "helper", "x").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
```

`bot_files(root, slug, instructions)` is the pure-ish core of `create_bot` (creates `<root>/<slug>/BOT.md`, errors if `<root>/<slug>/bot.toml` exists is NOT used here: it errors if `<root>/<slug>/BOT.md` exists); `create_bot` wraps it with `home::save_bot`. Run `cargo test -p herdbot setup::tests` → FAIL.

- [ ] **Step 2: Implement the pure parts.**

```rust
//! `herdbot setup` / `uninstall`: run the fixes `doctor` names (spec: packaging design).

use std::path::{Path, PathBuf};

use crate::doctor::{self, Check, Link, Status};
use crate::home::{self, Result};
use crate::{route, service};

const FIX_ORDER: [&str; 4] = ["home", "cli-link", "service", "restart-daemon"];

pub fn herdbot_toml_template() -> &'static str {
    "\
# herdbot settings. Everything here is optional; uncomment to change a default.

# Rewrite strings in imported instructions to local paths ('~' expands, longest match wins).
# [paths]
# \"/workspace/obsidian-vault\" = \"~/Documents/vault\"

# Launch agents without per-action prompts: \"ask\" (default) or \"auto\".
# [approvals]
# mode = \"auto\"

# Let codex agents write anywhere your user can (off by default).
# [codex]
# sandbox = \"none\"

# How many memory compactor runs may run at once across bots.
# [memory]
# max_compactors = 2
"
}

pub const STARTER_BOT_MD: &str = "\
You are a helpful assistant running locally under herdbot.
Keep answers short. Say what you did and what you didn't verify.
Edit this file to change how this bot behaves; it applies the next time the bot starts.
";

/// The first writable candidate on `PATH`, else the first writable one (flagged off-PATH).
pub fn link_dir(candidates: &[PathBuf], path_var: &str, writable: impl Fn(&Path) -> bool) -> Option<(PathBuf, bool)> {
    let on_path: Vec<PathBuf> = std::env::split_paths(path_var).collect();
    candidates
        .iter()
        .find(|c| writable(c) && on_path.contains(c))
        .map(|c| (c.clone(), true))
        .or_else(|| candidates.iter().find(|c| writable(c)).map(|c| (c.clone(), false)))
}

/// Fix ids the failing or warning checks ask for, once each, in the order they should run.
pub fn pending_fixes(checks: &[Check]) -> Vec<&'static str> {
    FIX_ORDER
        .into_iter()
        .filter(|fix| checks.iter().any(|c| c.status != Status::Ok && c.fix == Some(*fix)))
        .collect()
}

fn bot_files(root: &Path, slug: &str, instructions: &str) -> Result<PathBuf> {
    let dir = root.join(slug);
    if dir.join("BOT.md").exists() {
        return Err(format!("bot {slug} already exists").into());
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("BOT.md"), instructions)?;
    Ok(dir)
}

pub fn create_bot(name: &str, cwd: Option<String>, class: Option<String>, instructions: &str) -> Result<PathBuf> {
    let slug = home::slug(name);
    if home::bot_dir(&slug).join("bot.toml").exists() {
        return Err(format!("bot {slug} already exists").into());
    }
    home::save_bot(
        &slug,
        &home::Bot { name: name.to_owned(), cwd, class, source: "local".into(), ..Default::default() },
    )?;
    bot_files(&home::root(), &slug, instructions)
}
```

(Check `home::bot_dir(slug)` is `root().join(slug)`; if `save_bot` already creates the dir and `BOT.md` is missing, `bot_files` still works. Run the tests to confirm.) `cargo test -p herdbot setup::tests` → PASS.

- [ ] **Step 3: Fix runners and `run`.**

```rust
fn link_candidates() -> Vec<PathBuf> {
    // HERDBOT_LINK_DIR is a test hook: the live checks must not touch /opt/homebrew/bin.
    if let Some(dir) = std::env::var_os("HERDBOT_LINK_DIR") {
        return vec![PathBuf::from(dir)];
    }
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home::home_dir().join(".local/bin"),
    ]
}

fn writable(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    dir.metadata().map(|m| m.is_dir() && m.permissions().mode() & 0o200 != 0).unwrap_or(false)
        && std::fs::File::create(dir.join(".herdbot-write-test")).is_ok_and(|_| {
            let _ = std::fs::remove_file(dir.join(".herdbot-write-test"));
            true
        })
}

fn fix_home() -> Result<String> {
    std::fs::create_dir_all(home::root())?;
    let toml = home::root().join("herdbot.toml");
    if !toml.exists() {
        std::fs::write(&toml, herdbot_toml_template())?;
    }
    route::table()?; // writes routing.toml from the built-in default when absent
    Ok(format!("home ready at {}", home::root().display()))
}

fn fix_cli_link() -> Result<String> {
    let exe = std::env::current_exe()?;
    let path_var = std::env::var("PATH").unwrap_or_default();
    let (dir, on_path) = link_dir(&link_candidates(), &path_var, writable)
        .ok_or("no writable directory for the herdbot link (tried /opt/homebrew/bin, /usr/local/bin, ~/.local/bin)")?;
    let link = dir.join("herdbot");
    match std::fs::symlink_metadata(&link) {
        Ok(m) if m.file_type().is_symlink() => std::fs::remove_file(&link)?,
        Ok(_) => return Err(format!("{} is a regular file; not replacing it", link.display()).into()),
        Err(_) => {}
    }
    std::os::unix::fs::symlink(&exe, &link)?;
    let note = if on_path { "" } else { " (not on PATH: add the directory to your PATH)" };
    Ok(format!("linked {} -> {}{note}", link.display(), exe.display()))
}

fn run_fix(fix: &str) -> Result<String> {
    match fix {
        "home" => fix_home(),
        "cli-link" => fix_cli_link(),
        "service" => service::install(),
        "restart-daemon" => service::restart(),
        other => Err(format!("unknown fix {other}; one of {}", FIX_ORDER.join(", ")).into()),
    }
}

fn confirm(question: &str) -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim().eq_ignore_ascii_case("y")
}

pub async fn run(only: Option<String>, yes: bool) -> Result<()> {
    let mut failed = false;
    let mut apply = |fix: &str| match run_fix(fix) {
        Ok(msg) => println!("{fix}: {msg}"),
        Err(e) => {
            failed = true;
            eprintln!("{fix}: {e}");
        }
    };
    if let Some(fix) = only {
        apply(&fix);
        return if failed { Err("fix failed".into()) } else { Ok(()) };
    }
    for fix in pending_fixes(&doctor::evaluate(&doctor::gather().await)) {
        if fix != "restart-daemon" {
            apply(fix);
        }
    }
    // After a service install the daemon may already be current; ask again before restarting.
    if pending_fixes(&doctor::evaluate(&doctor::gather().await)).contains(&"restart-daemon") {
        apply("restart-daemon");
    }
    if home::list_bots().is_empty() && (yes || confirm("No bots yet. Create a starter bot named helper?")) {
        match create_bot("helper", None, None, STARTER_BOT_MD) {
            Ok(dir) => println!("created {} (edit BOT.md; then: herdbot send helper \"hello\")", dir.display()),
            Err(e) => {
                failed = true;
                eprintln!("starter bot: {e}");
            }
        }
    }
    print!("{}", doctor::render(&doctor::evaluate(&doctor::gather().await)));
    if failed { Err("some fixes failed".into()) } else { Ok(()) }
}
```

Borrow-checker note: `apply` mutably borrows `failed`; if the closure form fights the borrow checker, replace it with a small `fn apply(fix: &str, failed: &mut bool)`. Remove the unused `Link` import if clippy flags it.

- [ ] **Step 4: Wire `main.rs`.** `mod setup;`; USAGE: `  herdbot setup [--yes] [--only home|cli-link|service|restart-daemon]   fix what doctor reports`; arm:

```rust
        "setup" => {
            let only = flag(&mut args, "--only");
            let yes = args.iter().any(|a| a == "--yes" || a == "-y");
            setup::run(only, yes).await
        }
```

Replace the body of the `"new"` arm after the empty-name check with:

```rust
            let dir = setup::create_bot(&name, cwd, class, "")?;
            println!("created {} (edit BOT.md for its instructions)", dir.display());
            Ok(())
```

(keep the `cwd`/`class` flag parsing and the empty-name check). Behaviour of `herdbot new` must not change: it still creates an empty `BOT.md`.

- [ ] **Step 5: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`. Live check in a throwaway environment, never against launchd:

```bash
T=$(mktemp -d) && mkdir "$T/home" "$T/links" \
&& HOME="$T/home" HERDBOT_HOME="$T/bots" HERDBOT_LINK_DIR="$T/links" ./target/debug/herdbot setup --only home \
&& test -s "$T/bots/herdbot.toml" && test -s "$T/bots/routing.toml" \
&& HOME="$T/home" HERDBOT_HOME="$T/bots" HERDBOT_LINK_DIR="$T/links" ./target/debug/herdbot setup --only cli-link \
&& test -L "$T/links/herdbot" && readlink "$T/links/herdbot" \
&& HOME="$T/home" HERDBOT_HOME="$T/bots" HERDBOT_LINK_DIR="$T/links" PATH="$T/links:$PATH" ./target/debug/herdbot doctor | grep -E 'cli-link|home'
```

Expected: `cli-link` ok and `home` ok in the last line. Also `HERDBOT_HOME="$T/bots" ./target/debug/herdbot new demo && test -f "$T/bots/demo/BOT.md"` (empty BOT.md, bot.toml present). Remove `$T` afterwards.

- [ ] **Step 6: Commit.**

```bash
git add crates/herdbot/src/setup.rs crates/herdbot/src/main.rs
git commit -m "feat: herdbot setup"
```

---

### Task 4: `herdbot uninstall`

**Files:**
- Modify: `crates/herdbot/src/setup.rs` (append `uninstall`, `own_links`), `crates/herdbot/src/main.rs`

**Interfaces:**
- Produces: `setup::own_links(candidates: &[PathBuf], exe: &Path) -> Vec<PathBuf>` (pure over the filesystem it is given), `setup::uninstall(purge: bool) -> Result<()>`.

- [ ] **Step 1: Failing test.**

```rust
    #[test]
    fn own_links_are_only_symlinks_to_this_binary() {
        let dir = std::env::temp_dir().join(format!("herdbot-links-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("real-herdbot");
        std::fs::write(&exe, "").unwrap();
        let other = dir.join("other-herdbot");
        std::fs::write(&other, "").unwrap();
        let mine = dir.join("a");
        let theirs = dir.join("b");
        let plain = dir.join("c");
        for d in [&mine, &theirs, &plain] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::os::unix::fs::symlink(&exe, mine.join("herdbot")).unwrap();
        std::os::unix::fs::symlink(&other, theirs.join("herdbot")).unwrap();
        std::fs::write(plain.join("herdbot"), "").unwrap();
        let found = own_links(&[mine.clone(), theirs, plain], &exe);
        assert_eq!(found, vec![mine.join("herdbot")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

Run → FAIL.

- [ ] **Step 2: Implement.**

```rust
/// `herdbot` links in `candidates` that point at `exe`; anything else is not ours to remove.
pub fn own_links(candidates: &[PathBuf], exe: &Path) -> Vec<PathBuf> {
    candidates
        .iter()
        .map(|dir| dir.join("herdbot"))
        .filter(|link| {
            std::fs::symlink_metadata(link).is_ok_and(|m| m.file_type().is_symlink())
                && std::fs::read_link(link).is_ok_and(|target| target == exe)
        })
        .collect()
}

pub fn uninstall(purge: bool) -> Result<()> {
    println!("{}", service::uninstall()?);
    for link in own_links(&link_candidates(), &std::env::current_exe()?) {
        std::fs::remove_file(&link)?;
        println!("removed {}", link.display());
    }
    if purge {
        use std::io::{BufRead, Write};
        let root = home::root();
        print!("This deletes every bot, memory and room under {}. Type the path to confirm: ", root.display());
        std::io::stdout().flush()?;
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line)?;
        if line.trim() != root.to_string_lossy() {
            return Err("path did not match; nothing deleted".into());
        }
        std::fs::remove_dir_all(&root)?;
        println!("removed {}", root.display());
    } else {
        println!("kept {} (bots and memory); --purge removes it", home::root().display());
    }
    Ok(())
}
```

`own_links` compares `read_link` to `exe` literally, which matches what `fix_cli_link` wrote (`symlink(&exe, …)` with the unresolved `current_exe`).

- [ ] **Step 3: Wire.** USAGE: `  herdbot uninstall [--purge]            remove the service and the herdbot link; --purge also deletes the home`; arm: `"uninstall" => setup::uninstall(args.iter().any(|a| a == "--purge"))`.

- [ ] **Step 4: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`. **Do not run `uninstall` for real**: it boots out the running daemon. Confirm only that `./target/debug/herdbot --help`-style usage lists it.

- [ ] **Step 5: Commit.**

```bash
git add crates/herdbot/src/setup.rs crates/herdbot/src/main.rs
git commit -m "feat: herdbot uninstall"
```

---

### Task 5: Docs

**Files:**
- Modify: `README.md` (Requirements, Quick start, command list, "The service" paragraph)

- [ ] **Step 1: Edit.**
  - Quick start: after `cargo install --path crates/herdbot`, replace `herdbot service install` with `herdbot setup` and say it checks herdr and the harnesses, creates `~/Bots` and `herdbot.toml`, links the CLI, installs the service and offers a starter bot. Keep `herdbot service install` documented under "Other commands" for the manual path.
  - Command list: add `herdbot --version`, `herdbot doctor [--json]`, `herdbot setup [--yes] [--only <fix>]`, `herdbot uninstall [--purge]` (same text as USAGE).
  - "The service" paragraph: say the plist records the binary path as invoked, not resolved, and that `doctor` flags a daemon older than the binary on disk (fix: `herdbot setup --only restart-daemon`).
  - Mention `HERDBOT_LINK_DIR` only as a test hook in one clause.

- [ ] **Step 2: Verify** every command in the README Quick start exists: `for c in doctor setup uninstall; do ./target/debug/herdbot | grep -c "herdbot $c"; done` prints 1 for each.

- [ ] **Step 3: Commit.**

```bash
git add README.md
git commit -m "docs: document setup, doctor and uninstall"
```

---

## Self-review (done)

- Spec coverage: `--version` (T1), recorded path and `daemon.json` (T1), `doctor` and its checks except `login-item` (T2, app-only by spec), `setup` incl. `--only`, `--yes`, link dir, starter bot, `restart-daemon` (T3), `uninstall` and `--purge` with typed confirmation (T4), docs (T5). `service install` from inside an app bundle needs no change: it only refuses `target`.
- Deviation from the spec, already written into it: a harness the user doesn't use is not reported.
- Names are consistent across tasks: `Facts`, `Check`, `Link`, `Status`, `pending_fixes`, `link_dir`, `create_bot`, `bot_files`, `own_links`, fix ids.
- Not covered here (plans 2 and 3): the app, the release script, the cask, and the fresh-user test.
