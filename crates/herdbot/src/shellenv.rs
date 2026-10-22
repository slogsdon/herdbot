//! Resolve PATH for processes started without a terminal, such as the menu bar app.
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use crate::home;
const MARK: &str = "__HERDBOT_PATH__";
const CACHE_SECS: i64 = 600;
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Serialize, Deserialize)]
struct Cached { at: i64, path: String }
pub fn parse_marked(output: &str) -> Option<String> {
    let start = output.find(MARK)? + MARK.len();
    let end = start + output[start..].find(MARK)?;
    let path = &output[start..end];
    (!path.is_empty()).then(|| path.to_owned())
}
pub fn merge_paths(current: &str, shell: Option<&str>, extras: &[PathBuf]) -> String {
    let mut seen: Vec<PathBuf> = Vec::new();
    let dirs = std::env::split_paths(current)
        .chain(shell.into_iter().flat_map(std::env::split_paths))
        .chain(extras.iter().cloned());
    for dir in dirs {
        if !dir.as_os_str().is_empty() && !seen.contains(&dir) { seen.push(dir); }
    }
    std::env::join_paths(seen).map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
}
pub fn cache_fresh(at: i64, now: i64) -> bool { at <= now && now - at <= CACHE_SECS }
fn supports_ilc(shell: &str) -> bool {
    matches!(std::path::Path::new(shell).file_name().and_then(|n| n.to_str()), Some("zsh" | "bash"))
}
pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    if !supports_ilc(&shell) { return None; }
    let mut child = Command::new(&shell).args(["-ilc", &format!("printf '{MARK}%s{MARK}' \"$PATH\"")])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || { let mut text = String::new(); let _ = stdout.read_to_string(&mut text); text });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < SHELL_TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            _ => { let _ = child.kill(); let _ = child.wait(); break; }
        }
    }
    parse_marked(&reader.join().ok()?)
}
fn extras() -> Vec<PathBuf> {
    let home = home::home_dir();
    [PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/opt/homebrew/sbin"), PathBuf::from("/usr/local/bin"), home.join(".local/bin"), home.join(".cargo/bin")]
        .into_iter().filter(|dir| dir.is_dir()).collect()
}
fn cache_path() -> PathBuf { home::root().join(".state").join("shell-path.json") }
pub fn effective_path(fresh: bool) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    let now = home::now();
    let cached = (!fresh).then(|| home::read_json::<Cached>(&cache_path())).flatten()
        .filter(|c| cache_fresh(c.at, now)).map(|c| c.path);
    let shell = cached.or_else(|| {
        let found = login_shell_path();
        if let Some(path) = &found { let _ = home::write_json(&cache_path(), &Cached { at: now, path: path.clone() }); }
        found
    });
    merge_paths(&current, shell.as_deref(), &extras())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn the_marked_value_survives_noise_around_it() {
        let out = "Last login: Thu\nwelcome!\n__HERDBOT_PATH__/a/bin:/b/bin__HERDBOT_PATH__\nbye\n";
        assert_eq!(parse_marked(out).as_deref(), Some("/a/bin:/b/bin"));
        assert_eq!(parse_marked("no markers here"), None);
        assert_eq!(parse_marked("__HERDBOT_PATH____HERDBOT_PATH__"), None);
    }
    #[test] fn merge_keeps_order_and_drops_duplicates() {
        let extras = [PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/bin")];
        let merged = merge_paths("/usr/bin:/bin", Some("/Users/u/.local/bin:/usr/bin:/opt/homebrew/bin"), &extras);
        assert_eq!(merged, "/usr/bin:/bin:/Users/u/.local/bin:/opt/homebrew/bin");
    }
    #[test] fn merge_without_a_shell_path_still_adds_extras() {
        assert_eq!(merge_paths("/usr/bin", None, &[PathBuf::from("/opt/homebrew/bin")]), "/usr/bin:/opt/homebrew/bin");
        assert_eq!(merge_paths("", None, &[]), "");
    }
    #[test] fn the_cache_lives_ten_minutes_and_ignores_the_future() {
        assert!(cache_fresh(1_000, 1_000)); assert!(cache_fresh(1_000, 1_599));
        assert!(!cache_fresh(1_000, 1_601)); assert!(!cache_fresh(2_000, 1_000));
    }
    #[test] fn only_shells_that_accept_ilc_are_asked() {
        assert!(supports_ilc("/bin/zsh")); assert!(supports_ilc("/opt/homebrew/bin/bash"));
        assert!(!supports_ilc("/opt/homebrew/bin/fish")); assert!(!supports_ilc("/usr/bin/nu")); assert!(!supports_ilc(""));
    }
}
