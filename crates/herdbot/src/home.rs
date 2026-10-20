//! On-disk layout under `$HERDBOT_HOME` (default `~/Bots`). Plain files, no database:
//!
//! ```text
//! routing.toml              routing table (written from the built-in default on first use)
//! herdbot.toml              [paths] rewrites applied to imported instructions
//! <slug>/bot.toml           Bot definition
//! <slug>/BOT.md             instructions (owned by Grok Bot when source = "grok")
//! <slug>/BOT.local.md       local additions, appended after BOT.md, never overwritten
//! <slug>/memory/ skills/    the bot's own files
//! .state/<slug>.json        live herdr ids for the bot
//! .runs/<id>.json           one subagent run
//! .queue/<agent>.jsonl      prompts waiting for <agent> to go idle (daemon flushes)
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn root() -> PathBuf {
    std::env::var_os("HERDBOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join("Bots"))
}

pub fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn sub(dir: &str) -> PathBuf {
    let path = root().join(dir);
    let _ = fs::create_dir_all(&path);
    path
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Bot {
    pub name: String,
    #[serde(default)]
    pub title: String,
    /// Working directory for the bot's panes; defaults to the bot's own folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Routing class for the bot's main agent (default: "default").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// "grok" when BOT.md is synced from Grok Bot; "local" otherwise.
    #[serde(default = "local")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedules: Vec<Schedule>,
}

fn local() -> String {
    "local".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    /// Cron with seconds optional, local time zone, e.g. "0 9 * * 1-5".
    pub cron: String,
    pub prompt: String,
    /// Imported schedules start disabled so Grok Bot and herdbot never both fire them.
    #[serde(default)]
    pub enabled: bool,
}

/// `[a-z][a-z0-9-]*`, at most 20 chars so subagent names (`<slug>-<run>`) fit herdr's 32.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut out = out.trim_end_matches('-').to_owned();
    if !out.starts_with(|c: char| c.is_ascii_lowercase()) {
        out.insert(0, 'b');
    }
    out.truncate(20);
    out.trim_end_matches('-').to_owned()
}

pub fn bot_dir(slug: &str) -> PathBuf {
    root().join(slug)
}

pub fn load_bot(slug: &str) -> Result<Bot> {
    let path = bot_dir(slug).join("bot.toml");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("no bot {slug:?} ({}): {e}", path.display()))?;
    Ok(toml::from_str(&text)?)
}

pub fn save_bot(slug: &str, bot: &Bot) -> Result<()> {
    let dir = bot_dir(slug);
    for d in ["memory", "skills"] {
        fs::create_dir_all(dir.join(d))?;
    }
    fs::write(dir.join("bot.toml"), toml::to_string_pretty(bot)?)?;
    Ok(())
}

pub fn list_bots() -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("bot.toml").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

/// BOT.md followed by BOT.local.md.
pub fn instructions(slug: &str) -> String {
    let dir = bot_dir(slug);
    ["BOT.md", "BOT.local.md"]
        .iter()
        .filter_map(|f| fs::read_to_string(dir.join(f)).ok())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn cwd(slug: &str, bot: &Bot) -> String {
    bot.cwd
        .as_deref()
        .map(|c| c.replacen('~', &home_dir().to_string_lossy(), 1))
        .unwrap_or_else(|| bot_dir(slug).to_string_lossy().into_owned())
}

/// Live herdr ids for a running bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotState {
    pub workspace_id: String,
    pub root_pane: String,
    pub agent: String,
}

pub fn load_state(slug: &str) -> Option<BotState> {
    read_json(&sub(".state").join(format!("{slug}.json")))
}

pub fn save_state(slug: &str, state: &BotState) -> Result<()> {
    write_json(&sub(".state").join(format!("{slug}.json")), state)
}

pub fn clear_state(slug: &str) {
    let _ = fs::remove_file(sub(".state").join(format!("{slug}.json")));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub bot: String,
    /// herdr agent name that gets woken with the result.
    pub parent_agent: String,
    pub agent: String,
    pub pane: String,
    pub class: String,
    pub route: String,
    pub description: String,
    /// running | done | failed
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    pub started_at: i64,
    #[serde(default)]
    pub pane_closed: bool,
    #[serde(default)]
    pub notified_blocked: bool,
}

pub fn run_path(id: &str) -> PathBuf {
    sub(".runs").join(format!("{id}.json"))
}

pub fn load_run(id: &str) -> Option<Run> {
    read_json(&run_path(id))
}

pub fn save_run(run: &Run) -> Result<()> {
    write_json(&run_path(&run.id), run)
}

pub fn list_runs() -> Vec<Run> {
    fs::read_dir(sub(".runs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| read_json(&e.path()))
        .collect()
}

/// Six base36 chars from the clock and pid; unique enough for one machine's runs.
pub fn run_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut n = (nanos ^ ((std::process::id() as u128) << 40)) % 36u128.pow(6);
    let mut id = String::new();
    for _ in 0..6 {
        id.push(std::char::from_digit((n % 36) as u32, 36).unwrap());
        n /= 36;
    }
    id
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Queue a prompt for `agent`. Only the daemon delivers, and only once the agent is idle:
/// herdr's agent.prompt steers a busy pi turn and drops its in-flight answer.
pub fn enqueue(agent: &str, text: &str) -> Result<()> {
    let line = serde_json::to_string(&serde_json::json!({ "text": text, "at": now() }))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(sub(".queue").join(format!("{agent}.jsonl")))?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub fn queue_dir() -> PathBuf {
    sub(".queue")
}

pub fn paths_rewrites() -> BTreeMap<String, String> {
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        paths: BTreeMap<String, String>,
    }
    fs::read_to_string(root().join("herdbot.toml"))
        .ok()
        .and_then(|t| toml::from_str::<Config>(&t).ok())
        .unwrap_or_default()
        .paths
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Write via rename so concurrent readers never see a half-written file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_fit_agent_names() {
        assert_eq!(slug("Side Hustle CEO"), "side-hustle-ceo");
        assert_eq!(slug("dr eggbot"), "dr-eggbot");
        assert_eq!(slug("Grok Bot"), "grok-bot");
        assert_eq!(slug("42 things!!"), "b42-things");
        assert!(slug("an extremely long bot name that keeps going").len() <= 20);
        assert_eq!(run_id().len(), 6);
    }
}
