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
    /// Stable queue-message or turn-event ID; absent when legacy records have no ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// "user", "bot", "notice" or "unknown" (legacy provenance is never inferred).
    pub kind: &'static str,
    /// Typed origin, such as lifecycle, bot_message, or user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
    /// A room's speaker (a bot slug) for `bot` items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub text: String,
    /// "inflight" or "queued" for pending items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<&'static str>,
    /// Full lifecycle detail; `text` remains the concise collapsed preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
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

fn item(
    at: i64,
    kind: &'static str,
    speaker: Option<String>,
    text: &str,
    state: Option<&'static str>,
) -> Item {
    Item {
        n: None,
        at,
        id: None,
        kind,
        source: None,
        speaker,
        text: text.to_owned(),
        state,
        detail: None,
    }
}

fn source_name(source: Option<&str>) -> &'static str {
    match source {
        Some("user") => "user",
        Some("bot_message") => "bot_message",
        Some("subagent") => "subagent",
        Some("herdbot") => "herdbot",
        Some("lifecycle") => "lifecycle",
        Some("schedule") => "schedule",
        Some("room_message") => "room_message",
        _ => "unknown",
    }
}

fn source_kind_for(source: Option<&str>) -> &'static str {
    match source_name(source) {
        "user" => "user",
        "unknown" => "unknown",
        _ => "notice",
    }
}

fn sourced_item(at: i64, id: Option<&str>, source: Option<&str>, speaker: Option<String>, text: &str, state: Option<&'static str>) -> Item {
    Item {
        n: None,
        at,
        id: id.map(str::to_owned),
        kind: source_kind_for(source),
        source: Some(source_name(source)),
        speaker,
        text: text.to_owned(),
        state,
        detail: None,
    }
}

fn lifecycle_items(slug: &str) -> Vec<Item> {
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for run in home::list_runs()
        .into_iter()
        .filter(|run| run.parent_agent == slug)
    {
        let fallback = run.result.as_deref().map(preview);
        for event in run.events {
            if !seen.insert(event.id.clone()) {
                continue;
            }
            let text = if event.kind == "completed" {
                run.summary
                    .as_deref()
                    .filter(|summary| !summary.trim().is_empty())
                    .map(str::to_owned)
                    .or_else(|| fallback.clone())
                    .unwrap_or_else(|| event.summary.clone())
            } else {
                event.summary
            };
            items.push(Item {
                n: None,
                at: event.at,
                id: Some(event.id),
                kind: "notice",
                source: Some("lifecycle"),
                speaker: None,
                text,
                state: Some(match event.kind.as_str() {
                    "started" => "started",
                    "progress" => "progress",
                    "blocked" => "blocked",
                    "completed" => "completed",
                    "failed" => "failed",
                    "stopped" => "stopped",
                    "expired" => "expired",
                    _ => "lifecycle",
                }),
                detail: if event.kind == "completed" {
                    run.result.clone().or(event.detail)
                } else {
                    event.detail
                },
            });
        }
    }
    items
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
            let messages = turn["messages"].as_array();
            if let Some(messages) = messages.filter(|messages| !messages.is_empty() && messages.iter().all(|message| message["text"].is_string())) {
                for message in messages {
                    items.push(sourced_item(
                        at,
                        message["id"].as_str(),
                        message["source"].as_str(),
                        None,
                        message["text"].as_str().unwrap_or_default(),
                        None,
                    ));
                }
            } else {
                let source = messages.and_then(|messages| {
                    let first = messages.first()?.get("source")?.as_str()?;
                    messages.iter().all(|message| message["source"].as_str() == Some(first)).then_some(first)
                });
                items.push(sourced_item(at, turn["id"].as_str(), source, None, prompt, None));
            }
        }
        if let Some(reply) = turn["reply"].as_str() {
            let mut reply_item = item(at, "bot", None, reply, None);
            reply_item.id = turn["id"].as_str().map(str::to_owned);
            items.push(reply_item);
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
            "you" => sourced_item(e.at, None, Some("user"), None, &e.text, None),
            "herdbot" => sourced_item(e.at, None, Some("herdbot"), None, &e.text, None),
            bot => {
                let mut item = sourced_item(e.at, None, Some("bot_message"), Some(bot.to_owned()), &e.text, None);
                item.kind = "bot";
                item
            }
        })
        .collect();
    (numbered(items), bad)
}

/// What a bot has been sent but not logged: the in-flight prompt, then each queued message.
/// The main agent's name is the bot's slug (`run::main_agent`), which names its queue file.
pub fn pending_items(slug: &str) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(sent) = home::read_inflight(slug) {
        if !sent.messages.is_empty() && sent.messages.iter().all(|message| message.text.is_some()) {
            out.extend(sent.messages.iter().map(|message| sourced_item(
                sent.at, message.id.as_deref(), Some(&message.source), None, message.text.as_deref().unwrap_or_default(), Some("inflight"),
            )));
        } else {
            let source = sent.messages.first().and_then(|first| sent.messages.iter().all(|message| message.source == first.source).then_some(first.source.as_str()));
            out.push(sourced_item(sent.at, sent.messages.first().and_then(|message| message.id.as_deref()), source, None, &sent.prompt, Some("inflight")));
        }
    }
    out.extend(queued_items(slug));
    out
}

/// The messages waiting in a bot's queue, not yet delivered.
pub fn queued_items(slug: &str) -> Vec<Item> {
    let queue =
        fs::read_to_string(home::queue_dir().join(format!("{slug}.jsonl"))).unwrap_or_default();
    queue
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|queued| {
            let text = queued["text"].as_str()?;
            Some(sourced_item(
                queued["at"].as_i64().unwrap_or_default(),
                queued["id"].as_str(),
                queued["source"].as_str(),
                None,
                text,
                Some("queued"),
            ))
        })
        .collect()
}

/// The first line of `text`, cut to 120 characters, for a list row.
pub fn preview(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
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
            let lifecycle = lifecycle_items(target);
            let lifecycle_ids: std::collections::HashSet<String> = lifecycle
                .iter()
                .filter_map(|item| item.id.clone())
                .collect();
            let mut items = items;
            items.retain(|item| item.id.as_ref().is_none_or(|id| !lifecycle_ids.contains(id)));
            items.extend(lifecycle);
            items.sort_by_key(|item| item.at);
            let mut pending = pending_items(target);
            pending.retain(|item| item.id.as_ref().is_none_or(|id| !lifecycle_ids.contains(id)));
            (numbered(items), bad, pending)
        }
    };
    let total = items.len();
    let end = before.map_or(total, |n| n.min(total));
    let mut page = items[end.saturating_sub(limit)..end].to_vec();
    if before.is_none() {
        page.extend(pending);
    }
    let warnings = if bad > 0 {
        vec![format!("skipped {bad} unparsable line(s)")]
    } else {
        Vec::new()
    };
    Ok(History {
        v: VERSION,
        target: target.to_owned(),
        total,
        items: page,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_provenance_does_not_infer_human_authorship() {
        assert_eq!(source_kind_for(Some("user")), "user");
        assert_eq!(source_kind_for(Some("bot_message")), "notice");
        assert_eq!(source_kind_for(Some("subagent")), "notice");
        assert_eq!(source_kind_for(Some("herdbot")), "notice");
        assert_eq!(source_kind_for(Some("unknown")), "unknown");
        assert_eq!(source_kind_for(None), "unknown");
    }

    #[test]
    fn preview_is_the_first_nonblank_line_cut_short() {
        assert_eq!(preview("\n  hello \nworld"), "hello");
        assert_eq!(preview(""), "");
        assert_eq!(preview(&"x".repeat(300)).len(), 120);
    }
}
