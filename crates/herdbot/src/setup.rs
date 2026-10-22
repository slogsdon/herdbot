//! `herdbot setup`: run the fixes `doctor` names (spec: packaging design).

use std::path::{Path, PathBuf};

use crate::doctor::{self, Check, Status};
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
    candidates.iter().find(|candidate| writable(candidate) && on_path.contains(candidate))
        .map(|candidate| (candidate.clone(), true))
        .or_else(|| candidates.iter().find(|candidate| writable(candidate)).map(|candidate| (candidate.clone(), false)))
}

/// Fix ids the failing or warning checks ask for, once each, in execution order.
pub fn pending_fixes(checks: &[Check]) -> Vec<&'static str> {
    FIX_ORDER.into_iter().filter(|fix| checks.iter().any(|check| check.status != Status::Ok && check.fix == Some(*fix))).collect()
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
    home::save_bot(&slug, &home::Bot { name: name.to_owned(), cwd, class, source: "local".into(), ..Default::default() })?;
    bot_files(&home::root(), &slug, instructions)
}

fn link_candidates() -> Vec<PathBuf> {
    // Test hook: live setup checks must not touch /opt/homebrew/bin.
    if let Some(dir) = std::env::var_os("HERDBOT_LINK_DIR") {
        return vec![PathBuf::from(dir)];
    }
    vec![PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin"), home::home_dir().join(".local/bin")]
}

fn writable(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    dir.metadata().map(|metadata| metadata.is_dir() && metadata.permissions().mode() & 0o200 != 0).unwrap_or(false)
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
    route::table()?;
    Ok(format!("home ready at {}", home::root().display()))
}

fn fix_cli_link() -> Result<String> {
    let exe = std::env::current_exe()?;
    let path_var = std::env::var("PATH").unwrap_or_default();
    let (dir, on_path) = link_dir(&link_candidates(), &path_var, writable)
        .ok_or("no writable directory for the herdbot link (tried /opt/homebrew/bin, /usr/local/bin, ~/.local/bin)")?;
    let link = dir.join("herdbot");
    match std::fs::symlink_metadata(&link) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::remove_file(&link)?,
        Ok(_) => return Err(format!("{} is a regular file; not replacing it", link.display()).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
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

fn apply(fix: &str, failed: &mut bool) {
    match run_fix(fix) {
        Ok(message) => println!("{fix}: {message}"),
        Err(error) => {
            *failed = true;
            eprintln!("{fix}: {error}");
        }
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
    if let Some(fix) = only {
        apply(&fix, &mut failed);
        return if failed { Err("fix failed".into()) } else { Ok(()) };
    }
    for fix in pending_fixes(&doctor::evaluate(&doctor::gather().await)) {
        if fix != "restart-daemon" {
            apply(fix, &mut failed);
        }
    }
    if pending_fixes(&doctor::evaluate(&doctor::gather().await)).contains(&"restart-daemon") {
        apply("restart-daemon", &mut failed);
    }
    if home::list_bots().is_empty() && (yes || confirm("No bots yet. Create a starter bot named helper?")) {
        match create_bot("helper", None, None, STARTER_BOT_MD) {
            Ok(dir) => println!("created {} (edit BOT.md; then: herdbot send helper \"hello\")", dir.display()),
            Err(error) => {
                failed = true;
                eprintln!("starter bot: {error}");
            }
        }
    }
    print!("{}", doctor::render(&doctor::evaluate(&doctor::gather().await)));
    if failed { Err("some fixes failed".into()) } else { Ok(()) }
}

/// `herdbot` links in the candidate directories that point at this exact binary path.
pub fn own_links(candidates: &[PathBuf], exe: &Path) -> Vec<PathBuf> {
    candidates.iter().map(|dir| dir.join("herdbot")).filter(|link| {
        std::fs::symlink_metadata(link).is_ok_and(|metadata| metadata.file_type().is_symlink())
            && std::fs::read_link(link).is_ok_and(|target| target == exe)
    }).collect()
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
        let writable = |path: &Path| path != Path::new("/opt/homebrew/bin");
        assert_eq!(link_dir(&candidates, "/usr/bin:/usr/local/bin", writable), Some((PathBuf::from("/usr/local/bin"), true)));
        assert_eq!(link_dir(&candidates, "/usr/bin", writable), Some((PathBuf::from("/usr/local/bin"), false)));
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
        for candidate in [&mine, &theirs, &plain] { std::fs::create_dir_all(candidate).unwrap(); }
        std::os::unix::fs::symlink(&exe, mine.join("herdbot")).unwrap();
        std::os::unix::fs::symlink(&other, theirs.join("herdbot")).unwrap();
        std::fs::write(plain.join("herdbot"), "").unwrap();
        let found = own_links(&[mine.clone(), theirs, plain], &exe);
        assert_eq!(found, vec![mine.join("herdbot")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
