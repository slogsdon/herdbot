//! A conversation as a UI shows it: finished turns (a bot) or posts (a room), then whatever is
//! still on its way. Files only: it needs neither the daemon nor herdr.

use std::fs;

use serde::Serialize;
use serde_json::Value;

use crate::home::{self, Result};
use crate::rooms;

/// Bumped with `snapshot::VERSION`: both are the JSON the CLI prints.
pub const VERSION: u32 = crate::snapshot::VERSION;

/// Items per page unless the caller asks for another size.
pub const DEFAULT_LIMIT: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    /// Index among the persisted items, stable as new ones append. `None` while pending.
    pub n: Option<usize>,
    pub at: i64,
    /// "user", "bot" or "notice".
    pub kind: &'static str,
    /// A room's speaker (a bot slug) for `bot` items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub text: String,
    /// "inflight" or "queued" for pending items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct History {
    pub v: u32,
    pub target: String,
    /// Persisted items in the whole conversation; pending ones are not counted.
    pub total: usize,
    pub items: Vec<Item>,
    pub warnings: Vec<String>,
}

fn item(at: i64, kind: &'static str, speaker: Option<String>, text: &str, state: Option<&'static str>) -> Item {
    Item { n: None, at, kind, speaker, text: text.to_owned(), state }
}

/// Number the persisted items in order.
fn numbered(mut items: Vec<Item>) -> Vec<Item> {
    for (i, it) in items.iter_mut().enumerate() {
        it.n = Some(i);
    }
    items
}

/// A bot's turns: each `turns.jsonl` line gives your prompt (absent when typed in the pane), then
/// the reply. Also the count of unparsable lines.
// ponytail: re-reads the whole log each call; read only the tail if logs grow past a few MB.
pub fn bot_items(slug: &str) -> (Vec<Item>, usize) {
    let text = fs::read_to_string(home::bot_dir(slug).join("turns.jsonl")).unwrap_or_default();
    let mut items = Vec::new();
    let mut bad = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Ok(turn) = serde_json::from_str::<Value>(line) else {
            bad += 1;
            continue;
        };
        let at = turn["at"].as_i64().unwrap_or_default();
        if let Some(prompt) = turn["prompt"].as_str() {
            items.push(item(at, "user", None, prompt, None));
        }
        if let Some(reply) = turn["reply"].as_str() {
            items.push(item(at, "bot", None, reply, None));
        }
    }
    (numbered(items), bad)
}

/// A room's transcript: you, each bot (with its slug as `speaker`), and herdbot's notices.
pub fn room_items(slug: &str) -> (Vec<Item>, usize) {
    let entries = rooms::read_entries(slug);
    let bad = entries.iter().filter(|e| e.is_none()).count();
    let items = entries
        .into_iter()
        .flatten()
        .map(|e| match e.speaker.as_str() {
            "you" => item(e.at, "user", None, &e.text, None),
            "herdbot" => item(e.at, "notice", None, &e.text, None),
            bot => item(e.at, "bot", Some(bot.to_owned()), &e.text, None),
        })
        .collect();
    (numbered(items), bad)
}

/// What a bot has been sent but not logged: the in-flight prompt, then each queued message.
/// The main agent's name is the bot's slug (`run::main_agent`), which names its queue file.
pub fn pending_items(slug: &str) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(sent) = home::read_inflight(slug) {
        out.push(item(sent.at, "user", None, &sent.prompt, Some("inflight")));
    }
    let queue = fs::read_to_string(home::queue_dir().join(format!("{slug}.jsonl"))).unwrap_or_default();
    for queued in queue.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
        if let Some(text) = queued["text"].as_str() {
            out.push(item(queued["at"].as_i64().unwrap_or_default(), "user", None, text, Some("queued")));
        }
    }
    out
}

/// The first line of `text`, cut to 120 characters, for a list row.
pub fn preview(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().chars().take(120).collect()
}

/// `target` is a bot slug or `room:<slug>`. Pages from the end: `before` is an item `n` and the
/// page is the `limit` items ending just before it. Pending items ride along on the newest page.
pub fn history(target: &str, before: Option<usize>, limit: usize) -> Result<History> {
    let (items, bad, pending) = match target.strip_prefix("room:") {
        Some(slug) => {
            rooms::load_room(slug)?;
            let (items, bad) = room_items(slug);
            (items, bad, Vec::new())
        }
        None => {
            home::load_bot(target)?;
            let (items, bad) = bot_items(target);
            (items, bad, pending_items(target))
        }
    };
    let total = items.len();
    let end = before.map_or(total, |n| n.min(total));
    let mut page = items[end.saturating_sub(limit)..end].to_vec();
    if before.is_none() {
        page.extend(pending);
    }
    let warnings = if bad > 0 { vec![format!("skipped {bad} unparsable line(s)")] } else { Vec::new() };
    Ok(History { v: VERSION, target: target.to_owned(), total, items: page, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_the_first_nonblank_line_cut_short() {
        assert_eq!(preview("\n  hello \nworld"), "hello");
        assert_eq!(preview(""), "");
        assert_eq!(preview(&"x".repeat(300)).len(), 120);
    }
}
