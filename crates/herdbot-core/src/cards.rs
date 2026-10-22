//! Pending cards: prompts that need the user. Types and file I/O only.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardOption {
    pub id: String,
    pub label: String,
    pub style: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub kind: String,
    pub adapter: String,
    pub bot: String,
    pub agent: String,
    pub title: String,
    pub body: String,
    pub options: Vec<CardOption>,
    pub at: i64,
}

pub fn card_id(agent: &str, body: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in agent.bytes().chain(std::iter::once(0)).chain(body.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn cards_dir() -> PathBuf {
    home::root().join(".state").join("cards")
}
fn file_in(dir: &Path, agent: &str) -> PathBuf {
    dir.join(format!("{agent}.json"))
}
pub fn write_card_in(dir: &Path, card: &Card) -> Result<()> {
    fs::create_dir_all(dir)?;
    home::write_json(&file_in(dir, &card.agent), card)
}
pub fn read_card_in(dir: &Path, agent: &str) -> Option<Card> {
    home::read_json(&file_in(dir, agent))
}
pub fn read_cards_in(dir: &Path) -> Vec<Card> {
    let mut cards: Vec<Card> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .filter_map(|entry| home::read_json(&entry.path()))
        .collect();
    cards.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.agent.cmp(&b.agent)));
    cards
}
pub fn remove_card_in(dir: &Path, agent: &str) {
    let _ = fs::remove_file(file_in(dir, agent));
}
pub fn write_card(card: &Card) -> Result<()> {
    write_card_in(&cards_dir(), card)
}
pub fn read_card(agent: &str) -> Option<Card> {
    read_card_in(&cards_dir(), agent)
}
pub fn read_cards() -> Vec<Card> {
    read_cards_in(&cards_dir())
}
pub fn remove_card(agent: &str) {
    remove_card_in(&cards_dir(), agent)
}

pub fn log_answer(card: &Card, option: &str, via: &str) -> Result<()> {
    let line = serde_json::json!({ "at": home::now(), "card": card.id, "kind": card.kind, "adapter": card.adapter, "agent": card.agent, "option": option, "via": via });
    let path = home::root().join(".state").join("cards.log");
    fs::create_dir_all(path.parent().unwrap())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> Card {
        Card {
            id: card_id("helper", "dialog text"),
            kind: "approval".into(),
            adapter: "claude-trust".into(),
            bot: "helper".into(),
            agent: "helper".into(),
            title: "Trust this folder?".into(),
            body: "dialog text".into(),
            options: vec![
                CardOption {
                    id: "trust".into(),
                    label: "Yes, I trust this folder".into(),
                    style: "default".into(),
                },
                CardOption {
                    id: "exit".into(),
                    label: "No, exit".into(),
                    style: "destructive".into(),
                },
            ],
            at: 100,
        }
    }

    #[test]
    fn the_id_changes_with_the_agent_or_the_text_and_is_stable() {
        assert_eq!(card_id("a", "text"), card_id("a", "text"));
        assert_ne!(card_id("a", "text"), card_id("b", "text"));
        assert_ne!(card_id("a", "text"), card_id("a", "text2"));
        assert_eq!(card_id("a", "text").len(), 16);
        assert!(card_id("a", "text").chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn json_shape_is_what_the_app_reads() {
        let value = serde_json::to_value(card()).unwrap();
        for key in [
            "id", "kind", "adapter", "bot", "agent", "title", "body", "options", "at",
        ] {
            assert!(value.get(key).is_some(), "missing {key}");
        }
        assert_eq!(value["options"][1]["style"], "destructive");
    }

    #[test]
    fn files_round_trip_list_and_remove() {
        let dir = std::env::temp_dir().join(format!("herdbot-cards-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_cards_in(&dir).is_empty());
        write_card_in(&dir, &card()).unwrap();
        assert_eq!(read_cards_in(&dir), vec![card()]);
        assert_eq!(read_card_in(&dir, "helper"), Some(card()));
        remove_card_in(&dir, "helper");
        assert!(read_cards_in(&dir).is_empty());
        std::fs::write(dir.join("junk.json"), "not json").unwrap();
        assert!(read_cards_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
