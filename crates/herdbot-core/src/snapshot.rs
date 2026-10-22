//! What a UI reads: one serialisable view of herdbot's on-disk state, and a poller that
//! turns changes into lines. Files only: neither needs the daemon or herdr.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::net::UnixStream;

use serde::Serialize;
use serde_json::{Value, json};

use crate::cooldown;
use crate::events::{self, Event};
use crate::history;
use crate::home;
use crate::rooms;

/// Bumped on any breaking change to the JSON the CLI prints.
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BotView {
    pub slug: String,
    pub title: String,
    pub up: bool,
    /// Persisted history items (see `history::history`'s `total`).
    pub items: usize,
    /// Unix seconds of the last item.
    pub last_at: Option<i64>,
    /// First line of the last item, up to 120 characters.
    pub preview: String,
    /// A prompt was delivered and its turn has not been logged yet.
    pub working: bool,
    /// Messages waiting in the bot's queue, not yet delivered.
    pub queued: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomView {
    pub slug: String,
    pub name: String,
    pub mode: rooms::Mode,
    pub paused: bool,
    pub items: usize,
    pub last_at: Option<i64>,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunView {
    pub id: String,
    pub bot: String,
    pub agent: String,
    pub route: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CooldownView {
    pub harness: String,
    /// Unix seconds.
    pub until: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    pub v: u32,
    pub at: i64,
    /// "up" when something accepts connections on the daemon's lock socket, else "down".
    pub daemon: &'static str,
    pub bots: Vec<BotView>,
    pub rooms: Vec<RoomView>,
    /// Running subagent runs only.
    pub runs: Vec<RunView>,
    /// Harnesses still cooling down.
    pub cooldowns: Vec<CooldownView>,
    /// Agents whose latest event is `blocked`.
    pub blocked: Vec<Event>,
    pub warnings: Vec<String>,
}

/// Parse an event log. Unparsable lines are counted, not fatal.
pub fn parse_events(text: &str) -> (Vec<Event>, usize) {
    let mut bad = 0;
    let events = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Event>(line).map_err(|_| bad += 1).ok())
        .collect();
    (events, bad)
}

/// An agent is blocked while its latest event says so; any later event clears it.
pub fn latest_blocked(events: &[Event]) -> Option<&Event> {
    events.last().filter(|event| event.kind == "blocked")
}

fn event_logs(warnings: &mut Vec<String>) -> BTreeMap<String, Vec<Event>> {
    // ponytail: re-reads whole logs each poll; read only the tail if they grow past a few MB.
    let mut logs = BTreeMap::new();
    for entry in fs::read_dir(events::events_dir()).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(agent) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let (events, bad) = parse_events(&text);
        if bad > 0 {
            warnings.push(format!("{agent}.jsonl: skipped {bad} unparsable line(s)"));
        }
        logs.insert(agent.to_owned(), events);
    }
    logs
}

fn cooldowns() -> Vec<CooldownView> {
    let dir = home::root().join(".state").join("cooldown");
    let mut out: Vec<CooldownView> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let harness = entry.path().file_stem()?.to_str()?.to_owned();
            let cooldown = cooldown::active(&harness)?;
            Some(CooldownView { harness, until: cooldown.until, reason: cooldown.reason })
        })
        .collect();
    out.sort_by(|a, b| a.harness.cmp(&b.harness));
    out
}

pub fn snapshot() -> Snapshot {
    let mut warnings = Vec::new();
    let bots = home::list_bots()
        .into_iter()
        .filter_map(|slug| match home::load_bot(&slug) {
            Ok(bot) => {
                let (items, _) = history::bot_items(&slug);
                Some(BotView {
                    up: home::load_state(&slug).is_some(),
                    title: bot.title,
                    items: items.len(),
                    last_at: items.last().map(|i| i.at),
                    preview: items.last().map(|i| history::preview(&i.text)).unwrap_or_default(),
                    working: home::read_inflight(&slug).is_some(),
                    queued: history::queued_items(&slug).len(),
                    slug,
                })
            }
            Err(e) => {
                warnings.push(format!("{slug}: {e}"));
                None
            }
        })
        .collect();
    let rooms = rooms::list_rooms()
        .into_iter()
        .filter_map(|slug| match rooms::load_room(&slug) {
            Ok(room) => {
                let (items, _) = history::room_items(&slug);
                Some(RoomView {
                    paused: rooms::load_state(&slug).paused,
                    name: room.name,
                    mode: room.mode,
                    items: items.len(),
                    last_at: items.last().map(|i| i.at),
                    preview: items.last().map(|i| history::preview(&i.text)).unwrap_or_default(),
                    slug,
                })
            }
            Err(e) => {
                warnings.push(format!("room {slug}: {e}"));
                None
            }
        })
        .collect();
    let mut runs: Vec<RunView> = home::list_runs()
        .into_iter()
        .filter(|run| run.status == "running")
        .map(|run| RunView {
            id: run.id,
            bot: run.bot,
            agent: run.agent,
            route: run.route,
            description: run.description,
        })
        .collect();
    runs.sort_by(|a, b| a.id.cmp(&b.id));
    let mut blocked: Vec<Event> = event_logs(&mut warnings)
        .into_values()
        .filter_map(|events| latest_blocked(&events).cloned())
        .collect();
    blocked.sort_by_key(|event| event.at);
    Snapshot {
        v: VERSION,
        at: home::now(),
        daemon: if UnixStream::connect(home::daemon_lock()).is_ok() { "up" } else { "down" },
        bots,
        rooms,
        runs,
        cooldowns: cooldowns(),
        blocked,
        warnings,
    }
}

/// Polls the state and says what changed. Poll about once a second.
#[derive(Default)]
pub struct Watcher {
    /// Events already reported, per agent.
    seen: BTreeMap<String, usize>,
    last: Option<Snapshot>,
}

impl Watcher {
    /// Lines to print now: a snapshot if anything but its timestamp changed, then each event
    /// logged since the last poll. The first poll prints the snapshot only; history isn't replayed.
    pub fn poll(&mut self) -> Vec<Value> {
        let primed = self.last.is_some();
        let mut out = Vec::new();
        let snap = snapshot();
        let changed = self
            .last
            .as_ref()
            .is_none_or(|last| Snapshot { at: last.at, ..snap.clone() } != *last);
        if changed {
            out.push(json!({ "v": VERSION, "type": "snapshot", "snapshot": snap }));
            self.last = Some(snap);
        }
        for (agent, events) in event_logs(&mut Vec::new()) {
            let seen = self.seen.entry(agent).or_insert(if primed { 0 } else { events.len() });
            for event in &events[(*seen).min(events.len())..] {
                out.push(json!({ "v": VERSION, "type": "event", "event": event }));
            }
            *seen = events.len();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: &str) -> String {
        format!(r#"{{"at":1,"agent":"a","kind":"{kind}","harness":"claude"}}"#)
    }

    #[test]
    fn parse_events_counts_bad_lines_and_keeps_good_ones() {
        let text = format!("{}\nnot json\n\n{}\n", line("blocked"), line("turn_end"));
        let (events, bad) = parse_events(&text);
        assert_eq!(events.len(), 2);
        assert_eq!(bad, 1);
    }

    #[test]
    fn blocked_until_a_later_event_clears_it() {
        let (events, _) = parse_events(&format!("{}\n{}\n", line("turn_end"), line("blocked")));
        assert!(latest_blocked(&events).is_some());
        let (events, _) = parse_events(&format!("{}\n{}\n", line("blocked"), line("turn_end")));
        assert!(latest_blocked(&events).is_none());
        assert!(latest_blocked(&[]).is_none());
    }
}
