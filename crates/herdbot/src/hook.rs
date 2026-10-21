//! Harness hooks normalize a payload into an event without disturbing the harness.

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::home::{self, Result};

const MAX_TEXT: usize = 64 * 1024;

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

#[derive(Debug, PartialEq)]
pub enum Parsed {
    TurnEnd(String),
    Blocked(String),
}

fn str_at<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn summary(input: &Value) -> String {
    let text = match (input.get("command"), input.get("file_path")) {
        (Some(Value::String(command)), _) => command.clone(),
        (Some(Value::Array(parts)), _) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        (_, Some(Value::String(path))) => path.clone(),
        _ => input.to_string(),
    };
    text.chars().take(120).collect()
}

pub fn parse_hook(harness: &str, payload: &Value) -> Option<Parsed> {
    match harness {
        "claude" => match str_at(payload, "hook_event_name") {
            "Stop" if payload["stop_hook_active"] != Value::Bool(true) => Some(Parsed::TurnEnd(
                str_at(payload, "last_assistant_message").to_owned(),
            )),
            "PermissionRequest" => Some(Parsed::Blocked(format!(
                "{}: {}",
                str_at(payload, "tool_name"),
                summary(&payload["tool_input"])
            ))),
            "Notification" => {
                let message = str_at(payload, "message");
                let lower = message.to_lowercase();
                (lower.contains("permission") || lower.contains("waiting for your input"))
                    .then(|| Parsed::Blocked(message.to_owned()))
            }
            _ => None,
        },
        "codex" => match str_at(payload, "type") {
            "agent-turn-complete" => {
                let inputs = payload["input-messages"].as_array();
                if inputs
                    .and_then(|items| items.first())
                    .and_then(Value::as_str)
                    .is_some_and(|input| {
                        input.starts_with("Generate a concise, single-line task title")
                    })
                {
                    None
                } else {
                    Some(Parsed::TurnEnd(
                        str_at(payload, "last-assistant-message").to_owned(),
                    ))
                }
            }
            "approval-requested" => {
                Some(Parsed::Blocked(format!("approval: {}", summary(payload))))
            }
            _ => None,
        },
        "pi" => match str_at(payload, "event") {
            "agent_end" => Some(Parsed::TurnEnd(str_at(payload, "text").to_owned())),
            "ui_prompt_start" => Some(Parsed::Blocked(str_at(payload, "title").to_owned())),
            _ => None,
        },
        _ => None,
    }
}

pub fn cap(text: &str) -> String {
    if text.len() <= MAX_TEXT {
        return text.to_owned();
    }
    let mut head = MAX_TEXT / 2;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - MAX_TEXT / 2;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}[… {} bytes cut …]{}",
        &text[..head],
        tail - head,
        &text[tail..]
    )
}

pub fn agent_from_env(bot: Option<String>, run: Option<String>) -> Option<String> {
    let bot = bot.filter(|value| !value.is_empty())?;
    Some(match run.filter(|value| !value.is_empty()) {
        Some(run) => format!("{bot}-{run}"),
        None => bot,
    })
}

pub fn codex_notify(config_toml: &str) -> Option<Vec<String>> {
    let table: toml::Table = config_toml.parse().ok()?;
    let argv: Vec<String> = table
        .get("notify")?
        .as_array()?
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    (!argv.is_empty()).then_some(argv)
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

pub fn run(harness: &str, args: &[String]) {
    if let Err(error) = handle(harness, args) {
        let _ = std::fs::create_dir_all(events_dir());
        if let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(events_dir().join("hook-errors.log"))
        {
            let _ = writeln!(log, "{} {harness}: {error}", home::now());
        }
    }
}

fn handle(harness: &str, args: &[String]) -> Result<()> {
    let payload = if harness == "codex" {
        let payload = args.last().cloned().unwrap_or_default();
        chain_codex(&payload);
        payload
    } else {
        let mut payload = String::new();
        std::io::stdin().read_to_string(&mut payload)?;
        payload
    };
    let Some(agent) = agent_from_env(
        std::env::var("HERDBOT_BOT").ok(),
        std::env::var("HERDBOT_RUN").ok(),
    ) else {
        return Ok(());
    };
    let Some(parsed) = parse_hook(harness, &serde_json::from_str(&payload)?) else {
        return Ok(());
    };
    let (kind, text, reason) = match parsed {
        Parsed::TurnEnd(text) => ("turn_end", Some(cap(&text)), None),
        Parsed::Blocked(reason) => ("blocked", None, Some(cap(&reason))),
    };
    append_event(&Event {
        at: home::now(),
        agent,
        kind: kind.into(),
        text,
        reason,
        harness: harness.into(),
    })
}

fn chain_codex(payload: &str) {
    let config = std::fs::read_to_string(home::home_dir().join(".codex").join("config.toml"))
        .unwrap_or_default();
    if let Some(argv) = codex_notify(&config) {
        let _ = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .arg(payload)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> Value {
        let text = match name {
            "claude-stop" => include_str!("../fixtures/hooks/claude-stop.json"),
            "claude-permission" => include_str!("../fixtures/hooks/claude-permission.json"),
            "codex-turn" => include_str!("../fixtures/hooks/codex-turn.json"),
            _ => unreachable!(),
        };
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn parses_claude() {
        assert_eq!(
            parse_hook("claude", &fixture("claude-stop")),
            Some(Parsed::TurnEnd("PONG".into()))
        );
        let Some(Parsed::Blocked(reason)) = parse_hook("claude", &fixture("claude-permission"))
        else {
            panic!("permission request is a block");
        };
        assert!(reason.starts_with("Bash: touch x && test -e x"), "{reason}");
        let mut cont = fixture("claude-stop");
        cont["stop_hook_active"] = json!(true);
        assert_eq!(parse_hook("claude", &cont), None);
        let note = json!({"hook_event_name":"Notification","message":"Claude needs your permission to use Bash"});
        assert!(matches!(
            parse_hook("claude", &note),
            Some(Parsed::Blocked(_))
        ));
        let idle =
            json!({"hook_event_name":"Notification","message":"Claude is waiting for your input"});
        assert!(matches!(
            parse_hook("claude", &idle),
            Some(Parsed::Blocked(_))
        ));
        let other = json!({"hook_event_name":"Notification","message":"Task finished"});
        assert_eq!(parse_hook("claude", &other), None);
    }

    #[test]
    fn parses_codex_and_pi() {
        assert_eq!(
            parse_hook("codex", &fixture("codex-turn")),
            Some(Parsed::TurnEnd("PONG".into()))
        );
        let title = json!({"type":"agent-turn-complete","input-messages":["Generate a concise, single-line task title"],"last-assistant-message":{"title":"x"}});
        assert_eq!(parse_hook("codex", &title), None);
        assert!(matches!(
            parse_hook(
                "codex",
                &json!({"type":"approval-requested","command":["touch","y"]})
            ),
            Some(Parsed::Blocked(_))
        ));
        assert_eq!(parse_hook("codex", &json!({"type":"something-new"})), None);
        assert_eq!(
            parse_hook("pi", &json!({"event":"agent_end","text":"done"})),
            Some(Parsed::TurnEnd("done".into()))
        );
        assert_eq!(
            parse_hook(
                "pi",
                &json!({"event":"ui_prompt_start","title":"Allow bash?"})
            ),
            Some(Parsed::Blocked("Allow bash?".into()))
        );
        assert_eq!(parse_hook("nope", &json!({})), None);
    }

    #[test]
    fn caps_long_text_on_char_boundaries() {
        assert_eq!(cap("short"), "short");
        let long = "é".repeat(40_000);
        let capped = cap(&long);
        assert!(capped.len() < 66 * 1024);
        assert!(capped.contains(" bytes cut …]"));
        assert!(capped.starts_with('é') && capped.ends_with('é'));
    }

    #[test]
    fn agent_and_codex_notify() {
        assert_eq!(agent_from_env(Some("bot".into()), None), Some("bot".into()));
        assert_eq!(
            agent_from_env(Some("bot".into()), Some("k3x9a1".into())),
            Some("bot-k3x9a1".into())
        );
        assert_eq!(agent_from_env(None, Some("k3x9a1".into())), None);
        let config = "model = \"x\"\nnotify = [\n  \"/Apps/Sky Client\",\n  \"turn-ended\",\n]\n";
        assert_eq!(
            codex_notify(config),
            Some(vec!["/Apps/Sky Client".into(), "turn-ended".into()])
        );
        assert_eq!(codex_notify("model = \"x\"\n"), None);
    }
}
