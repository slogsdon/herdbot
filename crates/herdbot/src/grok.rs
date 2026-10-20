//! One-way sync from Grok Bot's local roster cache into herdbot bots.
//!
//! Grok Bot keeps a per-account roster snapshot in
//! `~/Library/Application Support/Grok Bot/sand-client-persistence/<base32 key>.blob`, where
//! the key decodes to `sand.client.slice.account.<account>.roster.last-roster`. Each row's
//! `description` holds the bot's full instructions. Schedules and skills are not cached
//! locally (they live in Grok Bot's server-side store), so they are not imported here.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use serde_json::Value;

use crate::home::{self, Bot, Result};

fn persistence_dir() -> PathBuf {
    home::home_dir().join("Library/Application Support/Grok Bot/sand-client-persistence")
}

/// RFC 4648 base32, lowercase, unpadded.
fn base32_decode(input: &str) -> Option<String> {
    let mut bits: u64 = 0;
    let mut count = 0;
    let mut out = Vec::new();
    for c in input.bytes() {
        let v = match c {
            b'a'..=b'z' => c - b'a',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | v as u64;
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn roster_path() -> Option<PathBuf> {
    fs::read_dir(persistence_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(base32_decode)
                .is_some_and(|key| key.ends_with(".roster.last-roster"))
        })
}

pub fn roster_mtime() -> Option<SystemTime> {
    fs::metadata(roster_path()?).ok()?.modified().ok()
}

/// Create or refresh one bot per roster row. BOT.md is overwritten only for bots whose
/// `source` is "grok"; local edits belong in BOT.local.md. Returns a summary of changes.
pub fn import() -> Result<String> {
    let path = roster_path().ok_or("Grok Bot roster cache not found")?;
    let roster: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let rows = roster["value"]["rows"]
        .as_array()
        .ok_or("roster: no rows")?;
    let rewrites = home::paths_rewrites();
    let mut report = Vec::new();
    for row in rows {
        let (Some(id), Some(name)) = (row["id"].as_str(), row["name"].as_str()) else {
            continue;
        };
        if row["isGroup"] == true {
            report.push(format!("skipped group {name:?} (rooms are not built yet)"));
            continue;
        }
        let existing = home::list_bots()
            .into_iter()
            .find(|s| home::load_bot(s).ok().and_then(|b| b.grok_id).as_deref() == Some(id));
        let slug = existing.clone().unwrap_or_else(|| home::slug(name));
        let mut bot = match existing {
            Some(_) => home::load_bot(&slug)?,
            None if home::bot_dir(&slug).join("bot.toml").exists() => {
                report.push(format!("skipped {name:?}: local bot {slug} already exists"));
                continue;
            }
            None => Bot {
                source: "grok".into(),
                grok_id: Some(id.into()),
                ..Bot::default()
            },
        };
        bot.name = name.into();
        bot.title = row["title"].as_str().unwrap_or_default().into();
        home::save_bot(&slug, &bot)?;
        if bot.source == "grok" {
            let mut text = row["description"].as_str().unwrap_or_default().to_owned();
            for (from, to) in &rewrites {
                text = text.replace(
                    from.as_str(),
                    &to.replace('~', &home::home_dir().to_string_lossy()),
                );
            }
            let target = home::bot_dir(&slug).join("BOT.md");
            if fs::read_to_string(&target).ok().as_deref() != Some(text.as_str()) {
                fs::write(target, &text)?;
                report.push(format!("updated {slug}"));
            }
        }
    }
    Ok(report.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_persistence_keys() {
        assert_eq!(
            base32_decode(
                "onqw4zbomnwgszlooqxhg3djmnss4ztjojzxillsovxc4zdfozuwgzjnn5xge33bojsgkza"
            )
            .as_deref(),
            Some("sand.client.slice.first-run.device-onboarded")
        );
    }
}
