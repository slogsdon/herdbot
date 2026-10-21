//! Harness hooks normalize a payload into an event without disturbing the harness.

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::home::{self, Result};

const MAX_TEXT: usize = 64 * 1024;

/// Seconds a hooked run may sit settled before the daemon falls back to the pane tail.
pub const HOOK_GRACE: i64 = 5;

/// Harnesses whose launch installs a hook (see route::launch). Others use the pane tail.
pub fn installed(harness: &str) -> bool {
    matches!(harness, "claude" | "codex" | "pi" | "cursor")
}

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
        "cursor" => match str_at(payload, "hook_event_name") {
            "afterAgentResponse" => Some(Parsed::TurnEnd(str_at(payload, "text").to_owned())),
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

#[derive(Debug, PartialEq)]
pub enum Action {
    /// A run's turn ended without report_result: finish it with this reply.
    FinishRun(String),
    BlockRun(String),
    /// Not yet decidable (the run's route is not recorded): keep the event for a later tick.
    Defer,
    /// A main agent's reply, for `<slug>/turns.jsonl`.
    LogTurn(String),
    LogBlocked(String),
    Ignore,
    /// No run and no bot by that name.
    Drop,
}

pub fn decide(event: &Event, run: Option<&home::Run>, is_main: bool) -> Action {
    let text = || event.text.clone().unwrap_or_default();
    let reason = || event.reason.clone().unwrap_or_default();
    match (run, event.kind.as_str()) {
        (Some(r), _) if r.pane_closed || r.status != "running" => Action::Ignore,
        (Some(r), _) if r.route.is_empty() => Action::Defer,
        // A compactor answers many jobs in one run; the daemon's memory tick ends it.
        (Some(r), "turn_end") if r.class == "compact" => Action::Ignore,
        (Some(_), "turn_end") => Action::FinishRun(text()),
        (Some(r), "blocked") if !r.notified_blocked => Action::BlockRun(reason()),
        (Some(_), _) => Action::Ignore,
        (None, "turn_end") if is_main => Action::LogTurn(text()),
        (None, "blocked") if is_main => Action::LogBlocked(reason()),
        (None, _) if is_main => Action::Ignore,
        (None, _) => Action::Drop,
    }
}

/// Whether the tail harvest may run: always for unhooked harnesses, else after HOOK_GRACE.
pub fn grace_over(harness: &str, settled_at: Option<i64>, now: i64) -> bool {
    !installed(harness) || settled_at.is_some_and(|t| now - t >= HOOK_GRACE)
}

/// `<slug>/turns.jsonl`: one line per main-agent turn, the input OptChat memory builds on.
pub fn append_turn(slug: &str, at: i64, prompt: Option<&str>, reply: &str) -> Result<()> {
    let line = serde_json::json!({ "at": at, "prompt": prompt, "reply": reply });
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home::bot_dir(slug).join("turns.jsonl"))?;
    file.write_all(format!("{line}\n").as_bytes())?;
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
    fn parses_cursor() {
        let payload: serde_json::Value = serde_json::from_str(include_str!(
            "../fixtures/hooks/cursor-after-response.json"
        ))
        .unwrap();
        assert_eq!(
            parse_hook("cursor", &payload),
            Some(Parsed::TurnEnd("PONG".into()))
        );
        assert!(installed("cursor"));
        let stop: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/hooks/cursor-stop.json")).unwrap();
        assert_eq!(parse_hook("cursor", &stop), None);
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

    fn event(kind: &str) -> Event {
        Event {
            at: 1,
            agent: "b-r1".into(),
            kind: kind.into(),
            text: Some("reply".into()),
            reason: Some("Bash: ls".into()),
            harness: "claude".into(),
        }
    }

    fn running() -> home::Run {
        home::Run {
            status: "running".into(),
            route: "claude:claude-sonnet-5-5".into(),
            ..Default::default()
        }
    }

    #[test]
    fn decides_for_runs() {
        assert_eq!(
            decide(&event("turn_end"), Some(&running()), false),
            Action::FinishRun("reply".into())
        );
        assert_eq!(
            decide(&event("blocked"), Some(&running()), false),
            Action::BlockRun("Bash: ls".into())
        );
        let told = home::Run {
            notified_blocked: true,
            ..running()
        };
        assert_eq!(decide(&event("blocked"), Some(&told), false), Action::Ignore);
        let done = home::Run {
            status: "done".into(),
            ..running()
        };
        assert_eq!(decide(&event("turn_end"), Some(&done), false), Action::Ignore);
        let starting = home::Run {
            route: String::new(),
            ..running()
        };
        assert_eq!(
            decide(&event("turn_end"), Some(&starting), false),
            Action::Defer
        );
    }

    #[test]
    fn compactor_turn_ends_do_not_finish_the_run() {
        let compact = home::Run { class: "compact".into(), ..running() };
        assert_eq!(decide(&event("turn_end"), Some(&compact), false), Action::Ignore);
        assert_eq!(decide(&event("blocked"), Some(&compact), false), Action::BlockRun("Bash: ls".into()));
    }

    #[test]
    fn decides_for_main_agents_and_strangers() {
        assert_eq!(
            decide(&event("turn_end"), None, true),
            Action::LogTurn("reply".into())
        );
        assert_eq!(
            decide(&event("blocked"), None, true),
            Action::LogBlocked("Bash: ls".into())
        );
        assert_eq!(decide(&event("turn_end"), None, false), Action::Drop);
    }

    #[test]
    fn grace_only_for_hooked_harnesses() {
        assert!(!grace_over("cursor", None, 100));
        assert!(grace_over("cursor", Some(95), 100));
        assert!(!grace_over("claude", None, 100));
        assert!(!grace_over("claude", Some(97), 100));
        assert!(grace_over("claude", Some(95), 100));
    }
}
