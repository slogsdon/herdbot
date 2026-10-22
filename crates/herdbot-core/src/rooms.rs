//! A room's files, as the daemon and the UIs read them: `rooms/<slug>/room.toml` and
//! `.state/rooms/<slug>.json`. Routing and the transcript live in the herdbot crate.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Members speak only when named.
    #[default]
    Channel,
    /// Each of your messages starts a round: every member once, in order.
    RoundRobin,
    /// Everything goes to the moderator; the moderator names who speaks.
    Moderated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Room {
    pub name: String,
    /// Bot slugs.
    pub members: Vec<String>,
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderator: Option<String>,
    /// Bot posts in a row, with no message from you, before the room pauses.
    pub max_turns: u32,
    /// A member is not called again this soon after its own post.
    pub member_cooldown_secs: i64,
    /// round_robin: a called member that has not posted by then is skipped.
    pub turn_timeout_secs: i64,
}

impl Default for Room {
    fn default() -> Self {
        Room {
            name: String::new(),
            members: Vec::new(),
            mode: Mode::Channel,
            moderator: None,
            max_turns: 6,
            member_cooldown_secs: 30,
            turn_timeout_secs: 300,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Round {
    /// Index into `members` of the member whose turn it is.
    pub next: usize,
    pub called_at: i64,
}

/// Written by the daemon only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Last transcript line routed.
    pub cursor: usize,
    /// Last line delivered to each member.
    pub last_seen: BTreeMap<String, usize>,
    pub last_post: BTreeMap<String, i64>,
    /// Called but still inside their cooldown.
    pub pending: BTreeSet<String>,
    pub bot_turns: u32,
    pub paused: bool,
    pub round: Option<Round>,
    pub missing_notified: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub at: i64,
    /// "you", a bot slug, or "herdbot".
    pub speaker: String,
    pub text: String,
}

pub fn parse_room(text: &str) -> Result<Room> {
    Ok(toml::from_str(text)?)
}

pub fn room_dir(slug: &str) -> PathBuf {
    home::root().join("rooms").join(slug)
}

pub fn load_room(slug: &str) -> Result<Room> {
    let path = room_dir(slug).join("room.toml");
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("no room {slug:?} ({}): {e}", path.display()))?;
    parse_room(&text)
}

pub fn list_rooms() -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(home::root().join("rooms"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().join("room.toml").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

pub fn state_path(slug: &str) -> PathBuf {
    home::root().join(".state").join("rooms").join(format!("{slug}.json"))
}

pub fn load_state(slug: &str) -> State {
    home::read_json(&state_path(slug)).unwrap_or_default()
}

/// Every transcript line, `None` for a malformed one, so index `i` is always sequence `i + 1`.
pub fn read_entries(slug: &str) -> Vec<Option<Entry>> {
    fs::read_to_string(room_dir(slug).join("transcript.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).ok())
        .collect()
}
