//! The per-agent event log that harness hooks append to: `.state/events/<agent>.jsonl`.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub id: String,
    #[serde(default = "event_source")]
    pub source: String,
    pub at: i64,
    pub agent: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub harness: String,
}

fn event_source() -> String { "hook".into() }

pub fn events_dir() -> PathBuf {
    home::root().join(".state").join("events")
}

pub fn append_event(event: &Event) -> Result<()> {
    append_event_in(&events_dir(), event)
}

fn append_event_in(dir: &std::path::Path, event: &Event) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.jsonl", event.agent));
    if !event.id.is_empty() {
        for candidate in [&path, &path.with_extension("taking")] {
            if std::fs::read_to_string(candidate).unwrap_or_default().lines()
                .filter_map(|line| serde_json::from_str::<Event>(line).ok())
                .any(|existing| existing.id == event.id) { return Ok(()); }
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(format!("{}\n", serde_json::to_string(event)?).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_hook_event_id_is_written_once() {
        let dir = std::env::temp_dir().join(format!("herdbot-events-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let event = Event {
            id: "hook:agent:turn-1".into(), source: "hook".into(), at: 1,
            agent: "agent".into(), kind: "turn_end".into(), text: Some("done".into()),
            reason: None, harness: "pi".into(),
        };
        append_event_in(&dir, &event).unwrap();
        append_event_in(&dir, &event).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("agent.jsonl")).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
