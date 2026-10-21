//! Rooms: group conversations between bots and Shane. Plain files under `rooms/<slug>/`; each
//! daemon tick routes new transcript lines to members' queues. Design:
//! docs/superpowers/specs/2026-10-20-rooms-design.md.
// Until Task 5 wires `tick` into the daemon, routing is only reached from tests.
#![cfg_attr(not(test), allow(dead_code))]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
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
pub struct Entry {
    pub at: i64,
    /// "you", a bot slug, or "herdbot".
    pub speaker: String,
    pub text: String,
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

pub fn parse_room(text: &str) -> Result<Room> {
    Ok(toml::from_str(text)?)
}

pub fn validate(room: &Room, bots: &[String]) -> std::result::Result<(), String> {
    if room.members.len() < 2 {
        return Err("a room needs at least two members".into());
    }
    if let Some(m) = room.members.iter().find(|m| !bots.contains(m)) {
        return Err(format!("no bot {m:?}; known: {}", bots.join(", ")));
    }
    if room.mode == Mode::Moderated
        && !room.moderator.as_ref().is_some_and(|m| room.members.contains(m))
    {
        return Err("a moderated room needs --moderator, one of its members".into());
    }
    Ok(())
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

pub fn save_room(slug: &str, room: &Room) -> Result<()> {
    fs::create_dir_all(room_dir(slug))?;
    fs::write(room_dir(slug).join("room.toml"), toml::to_string_pretty(room)?)?;
    Ok(())
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

/// One JSON line. serde_json escapes newlines in `text`, so an entry is always one line.
fn entry_line(speaker: &str, text: &str, at: i64) -> String {
    let entry = Entry { at, speaker: speaker.to_owned(), text: text.to_owned() };
    format!("{}\n", serde_json::to_string(&entry).unwrap_or_default())
}

/// Append one entry. A single write on an append-mode file, so concurrent writers (room send,
/// room_post, the daemon's notices) never interleave; line numbers are the sequence.
pub fn append(slug: &str, speaker: &str, text: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(room_dir(slug).join("transcript.jsonl"))?;
    file.write_all(entry_line(speaker, text, home::now()).as_bytes())?;
    Ok(())
}

/// Every line, `None` for a malformed one, so index `i` is always sequence `i + 1`.
pub fn read_entries(slug: &str) -> Vec<Option<Entry>> {
    fs::read_to_string(room_dir(slug).join("transcript.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn state_path(slug: &str) -> PathBuf {
    home::root().join(".state").join("rooms").join(format!("{slug}.json"))
}

pub fn load_state(slug: &str) -> State {
    home::read_json(&state_path(slug)).unwrap_or_default()
}

pub fn save_state(slug: &str, state: &State) -> Result<()> {
    fs::create_dir_all(state_path(slug).parent().unwrap())?;
    home::write_json(&state_path(slug), state)
}

/// transcript.md: what the `room up` pane tails and the GUI will read.
pub fn render_md(entries: &[Option<Entry>]) -> String {
    entries
        .iter()
        .flatten()
        .map(|e| {
            let time = chrono::DateTime::from_timestamp(e.at, 0)
                .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
                .unwrap_or_default();
            format!("**{}** · {time}\n{}\n\n", e.speaker, e.text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_toml_defaults_and_validation() {
        let room = parse_room("name = \"Crew\"\nmembers = [\"ann\", \"bob\"]\n").unwrap();
        assert_eq!(room.mode, Mode::Channel);
        assert_eq!((room.max_turns, room.member_cooldown_secs, room.turn_timeout_secs), (6, 30, 300));
        let rr = parse_room("name = \"X\"\nmembers = [\"a\", \"b\"]\nmode = \"round_robin\"\n").unwrap();
        assert_eq!(rr.mode, Mode::RoundRobin);
        assert!(parse_room("mode = \"chaos\"").is_err());

        let bots = vec!["ann".to_owned(), "bob".to_owned()];
        assert_eq!(validate(&room, &bots), Ok(()));
        let solo = Room { members: vec!["ann".into()], ..room.clone() };
        assert!(validate(&solo, &bots).unwrap_err().contains("two"));
        let stranger = Room { members: vec!["ann".into(), "zed".into()], ..room.clone() };
        assert!(validate(&stranger, &bots).unwrap_err().contains("zed"));
        let unmoderated = Room { mode: Mode::Moderated, ..room.clone() };
        assert!(validate(&unmoderated, &bots).unwrap_err().contains("moderator"));
        let moderated = Room { mode: Mode::Moderated, moderator: Some("bob".into()), ..room };
        assert_eq!(validate(&moderated, &bots), Ok(()));
    }

    #[test]
    fn transcript_lines_round_trip_and_render() {
        let line = entry_line("ann", "hello\nthere", 0);
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        let entry: Entry = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!((entry.speaker.as_str(), entry.text.as_str()), ("ann", "hello\nthere"));
        let md = render_md(&[Some(entry), None]);
        assert!(md.starts_with("**ann** · "));
        assert!(md.contains("\nhello\nthere\n"));
    }
}
