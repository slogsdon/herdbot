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
//! .state/cooldown/<h>.json  harness <h> hit a quota or auth failure; routing skips it until then
//! .state/rooms/<slug>.json  the daemon's routing state for the room
//! .runs/<id>.json           one subagent run
//! .queue/<agent>.jsonl      prompts waiting for <agent> to go idle (daemon flushes)
//! rooms/<slug>/room.toml    a room: members, mode, limits (see rooms.rs)
//! rooms/<slug>/transcript.jsonl / .md   the conversation (append-only) and its rendering
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
    /// OptChat memory (see memory.rs). Off unless set.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub memory: bool,
    /// The memory view's byte budget; memory::DEFAULT_VIEW when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_view_bytes: Option<usize>,
    /// "ask" or "auto"; overrides `[approvals] mode` in herdbot.toml for this bot's agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals: Option<String>,
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

/// Grok Bot's sandbox path for the Obsidian vault. Unmapped, it becomes the bot's cwd.
pub const GROK_VAULT: &str = "/workspace/obsidian-vault";

/// BOT.md followed by BOT.local.md, with `[paths]` rewrites applied. Rewriting at launch
/// keeps BOT.md identical to Grok Bot's text and makes herdbot.toml edits apply on restart.
pub fn instructions(slug: &str, bot: &Bot) -> String {
    let dir = bot_dir(slug);
    let text = ["BOT.md", "BOT.local.md"]
        .iter()
        .filter_map(|f| fs::read_to_string(dir.join(f)).ok())
        .collect::<Vec<_>>()
        .join("\n\n");
    rewrite_paths(text, &paths_rewrites(), &cwd(slug, bot))
}

fn rewrite_paths(mut text: String, rewrites: &BTreeMap<String, String>, cwd: &str) -> String {
    // Longest first, so "/workspace/obsidian-vault" wins over a "/workspace" mapping.
    let mut pairs: Vec<(&str, String)> = rewrites
        .iter()
        .map(|(from, to)| (from.as_str(), expand_tilde(to)))
        .collect();
    if !rewrites.contains_key(GROK_VAULT) {
        pairs.push((GROK_VAULT, cwd.to_owned()));
    }
    pairs.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    for (from, to) in pairs {
        text = text.replace(from, &to);
    }
    text
}

/// A leading `~` only: paths like `iCloud~md~obsidian` keep theirs.
pub fn expand_tilde(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) => format!("{}{rest}", home_dir().to_string_lossy()),
        None => path.to_owned(),
    }
}

pub fn cwd(slug: &str, bot: &Bot) -> String {
    bot.cwd
        .as_deref()
        .map(expand_tilde)
        .unwrap_or_else(|| bot_dir(slug).to_string_lossy().into_owned())
}

/// Live herdr ids for a running bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotState {
    pub workspace_id: String,
    pub root_pane: String,
    pub agent: String,
    /// Route the main agent was started on ("harness:model[:effort]").
    #[serde(default)]
    pub route: String,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// The task as given, so a run can be re-queued on another route.
    #[serde(default)]
    pub prompt: String,
    /// Where `result` came from: "report" (report_result), "hook" (the harness's turn-end
    /// hook) or "tail" (pane text). Empty for runs from before hooks.
    #[serde(default)]
    pub result_source: String,
    /// When the daemon first saw the agent settled; cleared while it works.
    #[serde(default)]
    pub settled_at: Option<i64>,
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

/// The daemon's single-instance lock; a live daemon accepts connections on it.
pub fn daemon_lock() -> PathBuf {
    root().join(".state").join("daemon.lock")
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

/// `[cua]` in herdbot.toml: where computer_use subagents reach cua-driver. No host means this
/// Mac; `command` defaults to `cua-driver` on PATH.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Cua {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

pub fn parse_cua(text: &str) -> Cua {
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        cua: Cua,
    }
    toml::from_str::<Config>(text).unwrap_or_default().cua
}

pub fn cua_config() -> Cua {
    parse_cua(&fs::read_to_string(root().join("herdbot.toml")).unwrap_or_default())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Compactor subagents running at once, across all bots.
    pub max_compactors: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        MemoryConfig { max_compactors: 2 }
    }
}

pub fn parse_memory_config(text: &str) -> MemoryConfig {
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        memory: MemoryConfig,
    }
    toml::from_str::<Config>(text).unwrap_or_default().memory
}

pub fn memory_config() -> MemoryConfig {
    parse_memory_config(&fs::read_to_string(root().join("herdbot.toml")).unwrap_or_default())
}

/// How a bot's agents handle tool approvals. Auto uses each harness's own automatic review,
/// never a bypass (route::approval_args).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Approvals {
    #[default]
    Ask,
    Auto,
}

pub fn parse_approvals(value: &str) -> Approvals {
    match value {
        "auto" => Approvals::Auto,
        "ask" => Approvals::Ask,
        other => {
            eprintln!("approvals: unknown mode {other:?}; using ask");
            Approvals::Ask
        }
    }
}

pub fn parse_approvals_default(herdbot_toml: &str) -> Approvals {
    #[derive(Deserialize, Default)]
    struct Section {
        mode: Option<String>,
    }
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        approvals: Section,
    }
    toml::from_str::<Config>(herdbot_toml)
        .unwrap_or_default()
        .approvals
        .mode
        .as_deref()
        .map_or(Approvals::Ask, parse_approvals)
}

/// The bot's own setting, else herdbot.toml's `[approvals] mode`, else ask.
pub fn approvals(bot: &Bot) -> Approvals {
    match bot.approvals.as_deref() {
        Some(mode) => parse_approvals(mode),
        None => parse_approvals_default(&fs::read_to_string(root().join("herdbot.toml")).unwrap_or_default()),
    }
}

/// Whether codex agents should run outside the workspace-write sandbox.
/// `[codex] sandbox = "none"` opts into `danger-full-access`; anything else keeps the
/// default sandboxed behaviour.
pub fn parse_codex_unsandboxed(herdbot_toml: &str) -> bool {
    #[derive(Deserialize, Default)]
    struct Section {
        sandbox: Option<String>,
    }
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        codex: Section,
    }
    match toml::from_str::<Config>(herdbot_toml) {
        Ok(c) => match c.codex.sandbox.as_deref() {
            Some("none") => true,
            Some("default") => false,
            Some(other) => {
                eprintln!("codex: unknown sandbox mode {other:?}; using default");
                false
            }
            None => false,
        },
        Err(_) => false,
    }
}

pub fn codex_unsandboxed() -> bool {
    parse_codex_unsandboxed(&fs::read_to_string(root().join("herdbot.toml")).unwrap_or_default())
}

pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// Write via rename so concurrent readers never see a half-written file.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    fs::rename(tmp, path)?;
    Ok(())
}

/// A delivered prompt whose turn has not ended yet. `turns.jsonl` only gets the prompt with its
/// reply, so this is how a UI shows the message while the bot works.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inflight {
    pub prompt: String,
    pub at: i64,
}

/// A marker older than this is a crash's leftover, not a bot still working.
pub const INFLIGHT_STALE_SECS: i64 = 3600;

fn inflight_path(bot: &str) -> PathBuf {
    root().join(".state").join("inflight").join(format!("{bot}.json"))
}

/// Mark `prompt` as delivered to `bot`'s main agent. A later delivery replaces it.
pub fn write_inflight(bot: &str, prompt: &str) -> Result<()> {
    let path = inflight_path(bot);
    fs::create_dir_all(path.parent().unwrap())?;
    write_json(&path, &Inflight { prompt: prompt.to_owned(), at: now() })
}

pub fn clear_inflight(bot: &str) {
    let _ = fs::remove_file(inflight_path(bot));
}

/// The marker, unless it is stale.
pub fn read_inflight(bot: &str) -> Option<Inflight> {
    read_json::<Inflight>(&inflight_path(bot)).filter(|m| now() - m.at < INFLIGHT_STALE_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cua_table_is_optional() {
        assert_eq!(parse_cua(""), Cua::default());
        assert_eq!(parse_cua("[paths]\n\"/a\" = \"/b\"\n"), Cua::default());
        assert_eq!(
            parse_cua(
                "[cua]\nhost = \"shane@studio.local\"\ncommand = \"~/.local/bin/cua-driver\"\n"
            ),
            Cua {
                host: Some("shane@studio.local".into()),
                command: Some("~/.local/bin/cua-driver".into())
            }
        );
    }

    #[test]
    fn memory_settings_default_off() {
        let bot: Bot = toml::from_str("name = \"x\"\n").unwrap();
        assert!(!bot.memory);
        assert_eq!(bot.memory_view_bytes, None);
        let on: Bot = toml::from_str(
            "name = \"x\"\nmemory = true\nmemory_view_bytes = 32000\n",
        )
        .unwrap();
        assert!(on.memory);
        assert_eq!(on.memory_view_bytes, Some(32000));
        assert_eq!(parse_memory_config("").max_compactors, 2);
        assert_eq!(parse_memory_config("[memory]\nmax_compactors = 1\n").max_compactors, 1);
    }

    #[test]
    fn approvals_default_to_ask_and_bots_override() {
        assert_eq!(parse_approvals_default(""), Approvals::Ask);
        assert_eq!(parse_approvals_default("[approvals]\nmode = \"auto\"\n"), Approvals::Auto);
        assert_eq!(parse_approvals("auto"), Approvals::Auto);
        assert_eq!(parse_approvals("yolo"), Approvals::Ask); // unknown → ask
        let bot: Bot = toml::from_str("name = \"x\"\napprovals = \"auto\"\n").unwrap();
        assert_eq!(bot.approvals.as_deref(), Some("auto"));
        let plain: Bot = toml::from_str("name = \"x\"\n").unwrap();
        assert_eq!(plain.approvals, None);
    }

    #[test]
    fn codex_sandbox_default_unless_none() {
        assert!(!parse_codex_unsandboxed(""));
        assert!(parse_codex_unsandboxed("[codex]\nsandbox = \"none\"\n"));
        assert!(!parse_codex_unsandboxed("[codex]\nsandbox = \"default\"\n"));
        assert!(!parse_codex_unsandboxed("[codex]\nsandbox = \"yolo\"\n")); // unknown → default
    }

    #[test]
    fn slugs_fit_agent_names() {
        assert_eq!(slug("Side Hustle CEO"), "side-hustle-ceo");
        assert_eq!(slug("dr eggbot"), "dr-eggbot");
        assert_eq!(slug("Grok Bot"), "grok-bot");
        assert_eq!(slug("42 things!!"), "b42-things");
        assert!(slug("an extremely long bot name that keeps going").len() <= 20);
        assert_eq!(run_id().len(), 6);
    }

    #[test]
    fn vault_path_defaults_to_cwd() {
        let text = "notes in /workspace/obsidian-vault/Daily and /workspace/x".to_owned();
        let none = BTreeMap::new();
        assert_eq!(
            rewrite_paths(text.clone(), &none, "/w/bot"),
            "notes in /w/bot/Daily and /workspace/x"
        );
        let mapped = BTreeMap::from([
            ("/workspace".to_owned(), "/w".to_owned()),
            (GROK_VAULT.to_owned(), "/vault".to_owned()),
        ]);
        assert_eq!(
            rewrite_paths(text, &mapped, "/w/bot"),
            "notes in /vault/Daily and /w/x"
        );
        let icloud = "/Users/x/Library/Mobile Documents/iCloud~md~obsidian/Documents/Personal";
        assert_eq!(expand_tilde(icloud), icloud);
        assert_eq!(expand_tilde("~/a~b"), format!("{}/a~b", home_dir().display()));
    }
}
