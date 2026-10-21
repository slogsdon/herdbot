//! The per-agent event log that harness hooks append to: `.state/events/<agent>.jsonl`.

use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub at: i64,
    pub agent: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub harness: String,
}

pub fn events_dir() -> PathBuf {
    home::root().join(".state").join("events")
}

pub fn append_event(event: &Event) -> Result<()> {
    std::fs::create_dir_all(events_dir())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_dir().join(format!("{}.jsonl", event.agent)))?;
    file.write_all(format!("{}\n", serde_json::to_string(event)?).as_bytes())?;
    Ok(())
}
