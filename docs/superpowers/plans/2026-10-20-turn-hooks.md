# Turn Hooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every harness reports "turn ended, with the reply" and "waiting on you" through its own native hook into herdbot, as specified in [the turn-hooks design](../specs/2026-10-20-turn-hooks-design.md).

**Architecture:** A new `herdbot hook <harness>` entry point, in `src/hook.rs`, turns a hook payload into one line in `.state/events/<agent>.jsonl`, and always exits 0 silently. `route::launch` installs the hook through each harness's per-agent config. The daemon drains the event files each tick. A pure `hook::decide` maps each event to an action: finish a run with the reply, notify a block, log a main-agent turn to `<slug>/turns.jsonl`, or defer.

**Tech Stack:** Rust 2024 (rustc 1.98), serde_json/toml (already dependencies). The hooks target claude 2.1.304, codex 0.163.0, pi 1.2.0 and cursor-agent 2026.10.19.

## Global Constraints

- **Precondition:** the [pi built-in MCP plan](2026-10-20-pi-builtin-mcp.md) is merged. Task 5 edits the `herdbot-pi.ts` it produced.
- `herdbot hook` always exits 0 and never writes to stdout. Its errors go to `$HERDBOT_HOME/.state/events/hook-errors.log`.
- Never edit `~/.claude/settings.json`, `~/.codex/config.toml` or any other user config. Per-agent hook config goes in launch args or `.state/launch/`.
- codex: the hook re-runs your own `notify` argv from `~/.codex/config.toml` with the payload appended, detached.
- Event text is capped at 64 KB (head and tail, `[… N bytes cut …]` between).
- Quota and auth detection, herdr idle checks and failover don't change.
- `HOOK_GRACE = 5` seconds. Only harnesses in `hook::installed()` get the grace.
- Tests stay in-file `#[cfg(test)]`. Payload fixtures live in `crates/herdbot/fixtures/hooks/` and are loaded with `include_str!`. Live checks are labeled **live**.
- Commits: conventional, explicit paths only. Clippy runs with `-D warnings`. Test counts are given as the number of new tests, because other plans may have landed first.

---

### Task 1: Capture real hook payloads (live, no herdbot code)

The spec's payload table comes from strings in the installed binaries. This task records real payloads as the fixtures later tasks test against. Where a capture contradicts the spec, use the capture and update the spec table in the same commit.

**Files:**
- Create: `crates/herdbot/fixtures/hooks/claude-stop.json`, `claude-permission.json`, `codex-turn.json`, `cursor-after-response.json` (only if cursor fires)
- Modify (only if a capture contradicts it): `docs/superpowers/specs/2026-10-20-turn-hooks-design.md` (payload table)

- [ ] **Step 1: claude Stop** (**live**)

```bash
mkdir -p /tmp/hookcap && cd /tmp/hookcap
claude --settings '{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"cat > /tmp/hookcap/claude-stop.json"}]}]}}' -p "Reply with exactly: PONG"
jq '{hook_event_name, stop_hook_active, last_assistant_message}' /tmp/hookcap/claude-stop.json
```

Expected: `"Stop"`, `false`, `"PONG"`. Also check whether your own `Stop` hook from `~/.claude/settings.json` ran too, from whatever that hook writes or logs. That tells you whether `--settings` hooks stack with yours. Record the answer in the spec under "Verified facts".

- [ ] **Step 2: claude PermissionRequest** (**live**, interactive). Run `claude --settings '{"hooks":{"PermissionRequest":[{"hooks":[{"type":"command","command":"cat > /tmp/hookcap/claude-permission.json"}]}]}}'` in `/tmp/hookcap`, ask it to `run the shell command: touch x`, and deny the prompt. Then:

```bash
jq '{hook_event_name, tool_name, tool_input}' /tmp/hookcap/claude-permission.json
```

Expected: `"PermissionRequest"`, `"Bash"`, and an object with `command`.

- [ ] **Step 3: codex notify** (**live**, interactive). In `/tmp/hookcap`, run

```bash
codex -c 'notify=["/bin/sh","-c","cat > /dev/null; printf %s \"$1\" > /tmp/hookcap/codex-$(date +%s%N).json","x"]'
```

Ask it `Reply with exactly: PONG`, then ask it to `run: touch y` and deny the approval. Then:

```bash
for f in /tmp/hookcap/codex-*.json; do jq -c '{type, "last-assistant-message"}' "$f"; done
```

Expected: one line `{"type":"agent-turn-complete","last-assistant-message":"PONG"}`. If no file has `"type":"approval-requested"`, codex doesn't send approvals to `notify`. Record that in the spec, and codex keeps herdr status for blocked. Copy the turn-complete file to the fixture path.

- [ ] **Step 4: cursor** (**live**, interactive). Write `/tmp/hookcap/.cursor/hooks.json`:

```json
{ "version": 1, "hooks": { "afterAgentResponse": [{ "command": "cat > /tmp/hookcap/cursor-after-response.json" }], "stop": [{ "command": "cat > /tmp/hookcap/cursor-stop.json" }] } }
```

Run `cursor-agent --trust` in `/tmp/hookcap` and ask `Reply with exactly: PONG`. Expected, if supported: a payload with the reply text. Record the field that holds it. If neither file appears, record "cursor: no hooks.json support seen" in the spec, and Task 6 becomes docs only.

- [ ] **Step 5: Save the fixtures.** Copy the four captured files into `crates/herdbot/fixtures/hooks/`, the cursor one only if it exists. Before committing, replace any absolute paths (`transcript_path`, `cwd`) with `/tmp/x` and any session ids with `00000000-0000-0000-0000-000000000000`.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/fixtures/hooks/claude-stop.json crates/herdbot/fixtures/hooks/claude-permission.json crates/herdbot/fixtures/hooks/codex-turn.json docs/superpowers/specs/2026-10-20-turn-hooks-design.md
git commit -m "test(herdbot): capture real turn-end hook payloads"
```

Add `crates/herdbot/fixtures/hooks/cursor-after-response.json` to the `git add` if it exists. Drop the spec path if it didn't change.

---

### Task 2: `herdbot hook`: parse, cap, write events, chain codex

**Files:**
- Create: `crates/herdbot/src/hook.rs`
- Modify: `crates/herdbot/src/main.rs` (`mod hook;`, USAGE, a `"hook"` arm)

**Interfaces:**
- Produces:
  - `pub struct Event { pub at: i64, pub agent: String, pub kind: String /* "turn_end" | "blocked" */, pub text: Option<String>, pub reason: Option<String>, pub harness: String }` (`Debug, Clone, PartialEq, Serialize, Deserialize`; `text`/`reason` `skip_serializing_if = "Option::is_none"` and `default`)
  - `pub enum Parsed { TurnEnd(String), Blocked(String) }` (`Debug, PartialEq`)
  - `pub fn parse_hook(harness: &str, payload: &serde_json::Value) -> Option<Parsed>`
  - `pub fn cap(text: &str) -> String`
  - `pub fn agent_from_env(bot: Option<String>, run: Option<String>) -> Option<String>`
  - `pub fn codex_notify(config_toml: &str) -> Option<Vec<String>>`
  - `pub fn events_dir() -> PathBuf`, `pub fn append_event(event: &Event) -> Result<()>`
  - `pub fn run(harness: &str, args: &[String])`, which never fails

- [ ] **Step 1: Write the failing tests.** Create `crates/herdbot/src/hook.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture(name: &str) -> serde_json::Value {
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
        assert_eq!(parse_hook("claude", &fixture("claude-stop")), Some(Parsed::TurnEnd("PONG".into())));
        let Some(Parsed::Blocked(reason)) = parse_hook("claude", &fixture("claude-permission")) else {
            panic!("permission request is a block");
        };
        assert!(reason.starts_with("Bash: touch x"), "{reason}");
        // Another hook is continuing the turn: not an end.
        let mut cont = fixture("claude-stop");
        cont["stop_hook_active"] = json!(true);
        assert_eq!(parse_hook("claude", &cont), None);
        let note = json!({"hook_event_name": "Notification", "message": "Claude needs your permission to use Bash"});
        assert!(matches!(parse_hook("claude", &note), Some(Parsed::Blocked(_))));
        let idle = json!({"hook_event_name": "Notification", "message": "Claude is waiting for your input"});
        assert!(matches!(parse_hook("claude", &idle), Some(Parsed::Blocked(_))));
        let other = json!({"hook_event_name": "Notification", "message": "Task finished"});
        assert_eq!(parse_hook("claude", &other), None);
    }

    #[test]
    fn parses_codex_and_pi() {
        assert_eq!(parse_hook("codex", &fixture("codex-turn")), Some(Parsed::TurnEnd("PONG".into())));
        assert!(matches!(
            parse_hook("codex", &json!({"type": "approval-requested", "command": ["touch", "y"]})),
            Some(Parsed::Blocked(_))
        ));
        assert_eq!(parse_hook("codex", &json!({"type": "something-new"})), None);
        assert_eq!(
            parse_hook("pi", &json!({"event": "agent_end", "text": "done"})),
            Some(Parsed::TurnEnd("done".into()))
        );
        assert_eq!(
            parse_hook("pi", &json!({"event": "ui_prompt_start", "title": "Allow bash?"})),
            Some(Parsed::Blocked("Allow bash?".into()))
        );
        assert_eq!(parse_hook("nope", &json!({})), None);
    }

    #[test]
    fn caps_long_text_on_char_boundaries() {
        assert_eq!(cap("short"), "short");
        let long = "é".repeat(40_000); // 80,000 bytes
        let capped = cap(&long);
        assert!(capped.len() < 66 * 1024);
        assert!(capped.contains(" bytes cut …]"));
        assert!(capped.starts_with('é') && capped.ends_with('é'));
    }

    #[test]
    fn agent_and_codex_notify() {
        assert_eq!(agent_from_env(Some("bot".into()), None), Some("bot".into()));
        assert_eq!(agent_from_env(Some("bot".into()), Some("k3x9a1".into())), Some("bot-k3x9a1".into()));
        assert_eq!(agent_from_env(None, Some("k3x9a1".into())), None);
        let config = "model = \"x\"\nnotify = [\n  \"/Apps/Sky Client\",\n  \"turn-ended\",\n]\n";
        assert_eq!(codex_notify(config), Some(vec!["/Apps/Sky Client".into(), "turn-ended".into()]));
        assert_eq!(codex_notify("model = \"x\"\n"), None);
    }
}
```

If Task 1's captures had different text than `PONG` (a trailing period, say), match the test strings to the fixture.

- [ ] **Step 2: Run them to verify they fail** (**unit**). Add `mod hook;` to `main.rs` (after `mod home;`).

Run: `cargo test -p herdbot hook::`
Expected: compile errors (`parse_hook`, `Parsed`, `cap`, `agent_from_env`, `codex_notify` not found).

- [ ] **Step 3: Implement.** Put this above the tests in `hook.rs`. If Task 1 found different field names, change only the field names in `parse_hook`.

```rust
//! `herdbot hook <harness>`: a harness's own turn-end and permission hooks report into
//! herdbot. One payload → at most one line in `.state/events/<agent>.jsonl`, drained by the
//! daemon. A hook must never disturb its harness: always exit 0, never print to stdout.

use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::home::{self, Result};

/// Harnesses whose launch installs a hook (see route::launch). Others use the pane tail.
pub fn installed(harness: &str) -> bool {
    matches!(harness, "claude" | "codex" | "pi")
}

/// Seconds a hooked run may sit settled before the daemon falls back to the pane tail.
pub const HOOK_GRACE: i64 = 5;

const MAX_TEXT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub at: i64,
    pub agent: String,
    /// "turn_end" or "blocked".
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

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// A short description of a tool input: its command or path, else compact JSON, ≤ 120 chars.
fn summary(input: &Value) -> String {
    let text = match (input.get("command"), input.get("file_path")) {
        (Some(Value::String(c)), _) => c.clone(),
        (Some(Value::Array(parts)), _) => parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
        (_, Some(Value::String(p))) => p.clone(),
        _ => input.to_string(),
    };
    text.chars().take(120).collect()
}

pub fn parse_hook(harness: &str, payload: &Value) -> Option<Parsed> {
    match harness {
        "claude" => match str_at(payload, "hook_event_name") {
            "Stop" if payload["stop_hook_active"] != Value::Bool(true) => {
                Some(Parsed::TurnEnd(str_at(payload, "last_assistant_message").to_owned()))
            }
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
                Some(Parsed::TurnEnd(str_at(payload, "last-assistant-message").to_owned()))
            }
            "approval-requested" => Some(Parsed::Blocked(format!("approval: {}", summary(payload)))),
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

/// At most MAX_TEXT bytes: the head and tail, cut on character boundaries.
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
    format!("{}[… {} bytes cut …]{}", &text[..head], tail - head, &text[tail..])
}

/// The herdr agent the hook's pane belongs to: `<bot>-<run>` for a subagent, `<bot>` for a
/// main agent, None outside herdbot panes.
pub fn agent_from_env(bot: Option<String>, run: Option<String>) -> Option<String> {
    let bot = bot.filter(|b| !b.is_empty())?;
    Some(match run.filter(|r| !r.is_empty()) {
        Some(run) => format!("{bot}-{run}"),
        None => bot,
    })
}

/// Shane's own codex `notify` argv, to run after herdbot's.
pub fn codex_notify(config_toml: &str) -> Option<Vec<String>> {
    let table: toml::Table = config_toml.parse().ok()?;
    let argv: Vec<String> = table
        .get("notify")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    (!argv.is_empty()).then_some(argv)
}

pub fn events_dir() -> PathBuf {
    home::root().join(".state").join("events")
}

/// One line, one write, append mode: concurrent hooks never interleave.
pub fn append_event(event: &Event) -> Result<()> {
    std::fs::create_dir_all(events_dir())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(events_dir().join(format!("{}.jsonl", event.agent)))?;
    file.write_all(format!("{}\n", serde_json::to_string(event)?).as_bytes())?;
    Ok(())
}

/// The CLI entry point. Never fails: errors go to hook-errors.log, never to the harness.
pub fn run(harness: &str, args: &[String]) {
    if let Err(e) = handle(harness, args) {
        let _ = std::fs::create_dir_all(events_dir());
        if let Ok(mut log) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(events_dir().join("hook-errors.log"))
        {
            let _ = writeln!(log, "{} {harness}: {e}", home::now());
        }
    }
}

fn handle(harness: &str, args: &[String]) -> Result<()> {
    // codex passes the payload as its last argument; the others on stdin.
    let payload = if harness == "codex" {
        let payload = args.last().cloned().unwrap_or_default();
        chain_codex(&payload);
        payload
    } else {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        text
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

/// Run Shane's own codex notify with the same payload, detached, as codex would have.
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
```

In `main.rs`, add to USAGE: `  herdbot hook <claude|codex|pi|cursor> [payload]   a harness hook (installed by launch)`. Add the arm `"hook" => { hook::run(args.first().map(String::as_str).unwrap_or_default(), args.get(1..).unwrap_or_default()); Ok(()) }` before `"mcp" =>`.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: all pass, 4 new tests, no warnings. `installed`, `HOOK_GRACE`, `Event`'s use in the daemon and `events_dir` are only used from Task 3 onwards. If clippy reports dead code, put `#[allow(dead_code)] // used from Task 3` on those two items, and remove it in Task 3.

- [ ] **Step 5: Live check** (**live**, no daemon needed):

```bash
cargo build -p herdbot
printf '{"hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"hi"}' \
  | HERDBOT_BOT=hooktest HERDBOT_HOME=/tmp/hb target/debug/herdbot hook claude; echo exit=$?
cat /tmp/hb/.state/events/hooktest.jsonl
printf 'not json' | HERDBOT_BOT=hooktest HERDBOT_HOME=/tmp/hb target/debug/herdbot hook claude; echo exit=$?
cat /tmp/hb/.state/events/hook-errors.log; rm -r /tmp/hb
```

Expected: `exit=0` with no other output, one `turn_end` line with `"text":"hi"`, then `exit=0` again and one error line in `hook-errors.log`.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/hook.rs crates/herdbot/src/main.rs
git commit -m "feat(herdbot): herdbot hook turns harness hook payloads into events"
```

---

### Task 3: The daemon consumes events

**Files:**
- Modify: `crates/herdbot/src/home.rs:197-218` (`Run`: derive `Default`, add `result_source` and `settled_at`)
- Modify: `crates/herdbot/src/hook.rs` (`Action`, `decide`, `grace_over`, `append_turn` plus tests; remove Task 2's `dead_code` allows)
- Modify: `crates/herdbot/src/run.rs:262-276` (the `Run { … }` literal in `spawn`: add the two fields)
- Modify: `crates/herdbot/src/tools.rs` (`report_result`: `run.result_source = "report".into();`)
- Modify: `crates/herdbot/src/daemon.rs` (`MainAgents.delivered`, `consume_events`, grace in `watch_runs`, the call in `run()`)

**Interfaces:**
- Consumes: Task 2's `Event`, `installed`, `HOOK_GRACE`, `events_dir`.
- Produces:
  - `pub enum Action { FinishRun(String), BlockRun(String), Defer, LogTurn(String), LogBlocked(String), Ignore, Drop }` (`Debug, PartialEq`)
  - `pub fn decide(event: &Event, run: Option<&home::Run>, is_main: bool) -> Action`
  - `pub fn grace_over(harness: &str, settled_at: Option<i64>, now: i64) -> bool`
  - `pub fn append_turn(slug: &str, at: i64, prompt: Option<&str>, reply: &str) -> Result<()>`

- [ ] **Step 1: Write the failing tests** in `hook.rs` tests:

```rust
    fn event(kind: &str) -> Event {
        Event { at: 1, agent: "b-r1".into(), kind: kind.into(), text: Some("reply".into()), reason: Some("Bash: ls".into()), harness: "claude".into() }
    }

    fn running() -> home::Run {
        home::Run { status: "running".into(), route: "claude:claude-sonnet-5-5".into(), ..Default::default() }
    }

    #[test]
    fn decides_for_runs() {
        assert_eq!(decide(&event("turn_end"), Some(&running()), false), Action::FinishRun("reply".into()));
        assert_eq!(decide(&event("blocked"), Some(&running()), false), Action::BlockRun("Bash: ls".into()));
        let told = home::Run { notified_blocked: true, ..running() };
        assert_eq!(decide(&event("blocked"), Some(&told), false), Action::Ignore);
        let done = home::Run { status: "done".into(), ..running() };
        assert_eq!(decide(&event("turn_end"), Some(&done), false), Action::Ignore);
        let starting = home::Run { route: String::new(), ..running() };
        assert_eq!(decide(&event("turn_end"), Some(&starting), false), Action::Defer);
    }

    #[test]
    fn decides_for_main_agents_and_strangers() {
        assert_eq!(decide(&event("turn_end"), None, true), Action::LogTurn("reply".into()));
        assert_eq!(decide(&event("blocked"), None, true), Action::LogBlocked("Bash: ls".into()));
        assert_eq!(decide(&event("turn_end"), None, false), Action::Drop);
    }

    #[test]
    fn grace_only_for_hooked_harnesses() {
        assert!(grace_over("cursor", None, 100));
        assert!(!grace_over("claude", None, 100));
        assert!(!grace_over("claude", Some(97), 100));
        assert!(grace_over("claude", Some(95), 100));
    }
```

- [ ] **Step 2: Run them to verify they fail** (**unit**)

Run: `cargo test -p herdbot hook::`
Expected: compile errors (`decide`, `Action`, `grace_over`, `Run: Default` missing).

- [ ] **Step 3: Implement.**

In `home.rs`, change `#[derive(Debug, Clone, Serialize, Deserialize)]` on `Run` to `#[derive(Debug, Clone, Default, Serialize, Deserialize)]`, and add after `prompt`:

```rust
    /// Where `result` came from: "report" (report_result), "hook" (the harness's turn-end
    /// hook) or "tail" (pane text). Empty for runs from before hooks.
    #[serde(default)]
    pub result_source: String,
    /// When the daemon first saw the agent settled; cleared while it works.
    #[serde(default)]
    pub settled_at: Option<i64>,
```

In `run.rs` `spawn`, add `result_source: String::new(), settled_at: None,` to the `Run { … }` literal. In `tools.rs` `report_result`, add `run.result_source = "report".into();` before `home::save_run(&run)?;`.

In `hook.rs`, add the following, and delete Task 2's `#[allow(dead_code)]` lines:

```rust
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
```

In `daemon.rs`:
- Change `use crate::{cooldown, grok, run};` to `use crate::{cooldown, grok, hook, run};`.
- Add this field to `MainAgents`:

  ```rust
      /// Last text delivered, kept until that turn's reply is logged (`prompt` above is
      /// consumed by quota failover, so turn pairing needs its own copy).
      delivered: HashMap<String, String>,
  ```

- In `flush_queues`, next to `mains.prompt.insert(agent.clone(), text);`, add `mains.delivered.insert(agent.clone(), text.clone());`. Move `.clone()` as needed so `text` is still available.
- In `run()`, right after the `flush_queues` block:

  ```rust
          if let Err(e) = consume_events(&herdr, &mut mains).await {
              eprintln!("events: {e}");
          }
  ```

- Add:

```rust
/// Drain `.state/events/*.jsonl` (written by `herdbot hook`) into runs, parents and turn logs.
async fn consume_events(herdr: &Client, mains: &mut MainAgents) -> Result<()> {
    let dir = hook::events_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        // Rename first so events written while we work land in a fresh file.
        let taken = path.with_extension("taking");
        fs::rename(&path, &taken)?;
        let mut deferred = Vec::new();
        for line in fs::read_to_string(&taken)?.lines() {
            let Ok(event) = serde_json::from_str::<hook::Event>(line) else {
                eprintln!("events: skipped malformed line in {}", taken.display());
                continue;
            };
            let run = home::list_runs().into_iter().find(|r| r.agent == event.agent);
            let is_main = home::load_state(&event.agent).is_some();
            match hook::decide(&event, run.as_ref(), is_main) {
                hook::Action::FinishRun(text) => {
                    let mut r = run.expect("decide only finishes known runs");
                    r.status = "done".into();
                    r.result = Some(text.clone());
                    r.result_source = "hook".into();
                    home::enqueue(
                        &r.parent_agent,
                        &format!("[subagent {} \"{}\" finished]\n{text}", r.id, r.description),
                    )?;
                    home::save_run(&r)?;
                    println!("run {} done (hook)", r.id);
                }
                hook::Action::BlockRun(reason) => {
                    let mut r = run.expect("decide only blocks known runs");
                    let tail = run::tail(herdr, &r.agent, 60).await;
                    if !requeue_if_cooled(herdr, &mut r, &tail).await? {
                        home::enqueue(
                            &r.parent_agent,
                            &format!(
                                "[subagent {} \"{}\" is blocked]\nIt is waiting for an approval in herdr pane {} ({reason}). Shane must answer it there.",
                                r.id, r.description, r.pane
                            ),
                        )?;
                        r.notified_blocked = true;
                    }
                    home::save_run(&r)?;
                }
                hook::Action::Defer => deferred.push(line.to_owned()),
                hook::Action::LogTurn(reply) => {
                    let prompt = mains.delivered.remove(&event.agent);
                    hook::append_turn(&event.agent, event.at, prompt.as_deref(), &reply)?;
                }
                hook::Action::LogBlocked(reason) => println!("blocked {}: {reason}", event.agent),
                hook::Action::Ignore => {}
                hook::Action::Drop => eprintln!("events: dropping event for unknown agent {}", event.agent),
            }
        }
        if !deferred.is_empty() {
            let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)?;
            std::io::Write::write_all(&mut file, format!("{}\n", deferred.join("\n")).as_bytes())?;
        }
        fs::remove_file(taken)?;
    }
    Ok(())
}
```

In `watch_runs`, right after `let settled = …;`:

```rust
        let now = home::now();
        match (settled, r.settled_at) {
            (true, None) => {
                r.settled_at = Some(now);
                home::save_run(&r)?;
            }
            (false, Some(_)) => {
                r.settled_at = None;
                home::save_run(&r)?;
            }
            _ => {}
        }
        let harness = r.route.split(':').next().unwrap_or_default().to_owned();
        let grace_over = hook::grace_over(&harness, r.settled_at, now);
```

Change the harvest arm's guard from `if settled && home::now() - r.started_at > HARVEST_AFTER` to `if settled && grace_over && now - r.started_at > HARVEST_AFTER`. Inside it, next to `r.result = Some(text.clone());`, add `r.result_source = "tail".into();`.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: all pass, 3 new tests (7 since Task 1), no warnings.

- [ ] **Step 5: Live check, by hand** (**live**, daemon on the new binary, no launch wiring yet). Fake a hook event for a real run:

```bash
cargo install --path crates/herdbot && launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon
herdbot task herdbot-dev explore "Wait for further instructions; do nothing yet."
R=$(herdbot runs | tail -1 | cut -d' ' -f1)
printf '{"at":%s,"agent":"herdbot-dev-%s","kind":"turn_end","text":"hooked reply","harness":"pi"}\n' $(date +%s) $R >> ~/Bots/.state/events/herdbot-dev-$R.jsonl
sleep 5; jq '{status, result, result_source}' ~/Bots/.runs/$R.json
```

Expected: `"done"`, `"hooked reply"`, `"hook"`, and herdbot-dev receives `[subagent <id> … finished]\nhooked reply`.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/hook.rs crates/herdbot/src/home.rs crates/herdbot/src/run.rs crates/herdbot/src/tools.rs crates/herdbot/src/daemon.rs
git commit -m "feat(herdbot): finish runs and log main-agent turns from hook events"
```

---

### Task 4: Install the hooks for claude and codex

**Files:**
- Modify: `crates/herdbot/src/route.rs` (`claude_hook_settings`, `codex_notify_arg` plus tests; the claude and codex arms of `launch`)

**Interfaces:**
- Produces: `fn claude_hook_settings(exe: &str) -> serde_json::Value` and `fn codex_notify_arg(exe: &str) -> String`.

- [ ] **Step 1: Write the failing tests** in `route.rs` tests:

```rust
    #[test]
    fn hook_wiring() {
        let s = claude_hook_settings("/Users/x/My Tools/herdbot");
        let cmd = s["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, "'/Users/x/My Tools/herdbot' hook claude");
        assert_eq!(s["hooks"]["PermissionRequest"][0]["hooks"][0]["type"], "command");
        assert!(s["hooks"].get("Notification").is_some());
        assert_eq!(
            codex_notify_arg("/Users/x/herdbot"),
            r#"notify=["/Users/x/herdbot","hook","codex"]"#
        );
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot hook_wiring`. Expected: compile error.

- [ ] **Step 3: Implement.** In `route.rs`:

```rust
/// claude `--settings` content adding herdbot's hooks. claude runs `command` through a shell,
/// so the exe path is single-quoted.
fn claude_hook_settings(exe: &str) -> serde_json::Value {
    let command = format!("'{}' hook claude", exe.replace('\'', r"'\''"));
    let entry = serde_json::json!([{ "hooks": [{ "type": "command", "command": command }] }]);
    serde_json::json!({ "hooks": { "Stop": entry, "PermissionRequest": entry, "Notification": entry } })
}

/// codex `-c` override: herdbot's hook as notify (it chains Shane's own notify itself).
fn codex_notify_arg(exe: &str) -> String {
    format!("notify={}", serde_json::json!([exe, "hook", "codex"]))
}
```

In the claude arm of `launch`, after writing `mcp_file`:

```rust
            let settings_file = dir.join(format!("{agent}.settings.json"));
            std::fs::write(&settings_file, serde_json::to_vec_pretty(&claude_hook_settings(&exe))?)?;
```

Then add `s("--settings"), settings_file.to_string_lossy().into_owned(),` to the `args` vec before `--allowedTools`. In the codex arm, add `s("-c"), codex_notify_arg(&exe),` to `args` after the `default_tools_approval_mode` pair.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`. Expected: 1 new test, no warnings.

- [ ] **Step 5: Live check** (**live**, daemon on the new binary). Use `herdbot route` to check which harness a class resolves to. Pass explicit models from a bot pane if needed: ask herdbot-dev to call `task` with `model: "claude:claude-haiku-5-5"`, then again with `model: "codex:gpt-6-luna:low"`. Each task is `Say PONG and stop, without calling report_result.`

Expected for each run:
- `jq '{status, result, result_source}' ~/Bots/.runs/<id>.json` shows `"done"`, a result containing `PONG`, and `"hook"` (not `"tail"`).
- For codex, Codex Computer Use's client still ran. Check with its own log if it has one, or with `ps` during the turn. Note what you checked.
- A claude run asked to `touch x` produces one `[subagent … is blocked]` message with `Bash: touch x` in it.
- Your own `~/.claude/settings.json` `Stop` hook still fired (Task 1's stacking result).

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/route.rs
git commit -m "feat(herdbot): install turn hooks for claude and codex agents"
```

---

### Task 5: pi hook in `herdbot-pi.ts`

**Files:**
- Modify: `crates/herdbot/assets/herdbot-pi.ts` (as left by the pi-MCP plan)

- [ ] **Step 1: Implement.** Add to `herdbot-pi.ts`. Keep the existing `registerMcpServer` call, and put the hook code above it inside the default export:

```ts
import { execFile } from "node:child_process";

type Message = { role?: string; content?: unknown };

/** The text of the last assistant message: its text items, without thinking or tool calls. */
function lastAssistantText(messages: Message[]): string {
	for (let i = messages.length - 1; i >= 0; i--) {
		const m = messages[i];
		if (m.role !== "assistant") continue;
		if (typeof m.content === "string") return m.content;
		if (Array.isArray(m.content)) {
			return m.content
				.filter((c): c is { type: "text"; text: string } => c?.type === "text" && typeof c.text === "string")
				.map((c) => c.text)
				.join("");
		}
	}
	return "";
}

/** Hand one event to `herdbot hook pi` on stdin. Fire and forget: a hook never slows pi down. */
function hook(payload: object) {
	const child = execFile(process.env.HERDBOT_EXE ?? "herdbot", ["hook", "pi"], () => {});
	child.stdin?.end(JSON.stringify(payload));
}
```

Inside `export default function (pi: ExtensionAPI) {`:

```ts
	pi.on("agent_end", (event) => hook({ event: "agent_end", text: lastAssistantText(event.messages as Message[]) }));
	pi.on("ui_prompt_start", (event) => hook({ event: "ui_prompt_start", title: event.title ?? event.kind }));
```

- [ ] **Step 2: Build and test** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`. Expected: no new tests, all pass. The asset is `include_str!`'d, so a build is enough to embed it.

- [ ] **Step 3: Live check** (**live**). From herdbot-dev, `task` with `model: "pi:deepseek-v4-flash:low"` and the prompt `Say PONG and stop, without calling report_result.`

Expected: the run is `done` with `result_source: "hook"` and `PONG` in its result. Also send herdbot-dev itself a message and check that `~/Bots/herdbot-dev/turns.jsonl` gains a line pairing it with the reply. That works if herdbot-dev runs on pi; on claude, Task 4 already covered it.

- [ ] **Step 4: Commit**

```bash
git add crates/herdbot/assets/herdbot-pi.ts
git commit -m "feat(herdbot): report pi turn ends and prompts through herdbot hook"
```

---

### Task 6: cursor (if Task 1 found hook support) and docs

**Files:**
- Modify (cursor only): `crates/herdbot/src/hook.rs` (cursor arm in `parse_hook`, `installed`, a test), `crates/herdbot/src/route.rs` (the cursor arm merges `.cursor/hooks.json`)
- Modify: `README.md`

- [ ] **Step 1 (cursor supported): write the failing test** in `hook.rs`. Use the field Task 1 recorded. The example below assumes `text`:

```rust
    #[test]
    fn parses_cursor() {
        let payload: serde_json::Value = serde_json::from_str(include_str!("../fixtures/hooks/cursor-after-response.json")).unwrap();
        assert_eq!(parse_hook("cursor", &payload), Some(Parsed::TurnEnd("PONG".into())));
        assert!(installed("cursor"));
    }
```

Run: `cargo test -p herdbot parses_cursor`. Expected: FAIL.

- [ ] **Step 2 (cursor supported): implement.**
  - Add `| "cursor"` to `installed`.
  - In `parse_hook`, add this arm, with the field name from Task 1:

    ```rust
            "cursor" => match str_at(payload, "hook_event_name") {
                "afterAgentResponse" => Some(Parsed::TurnEnd(str_at(payload, "text").to_owned())),
                _ => None,
            },
    ```

  - In `route.rs`'s cursor arm, right after the `mcp.json` merge, merge `<cwd>/.cursor/hooks.json` the same way. Read it or start from `{"version": 1, "hooks": {}}`. If no existing `afterAgentResponse` entry's `command` contains `hook cursor`, push `{"command": "'<exe>' hook cursor"}` to `hooks.afterAgentResponse` and write the file.

  Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`. Expected: 1 new test, all pass. Live check: repeat Task 4 Step 5 with `model: "cursor:composer-2.5"`. Expected: `result_source: "hook"`.

- [ ] **Step 3: Docs.** In `README.md`:
  - Subagent lifecycle, step 6: "**No report.**" now says the daemon uses the harness's own last reply from its turn-end hook (claude `Stop`, codex `notify`, pi `agent_end`, plus cursor if wired). It falls back to the last 60 pane lines only for harnesses without a hook, or 5 s after a hooked agent settles with no hook event.
  - Add a `### Turn hooks` subsection after "Harness launch":
    - a table of harness → hook → how it's installed (`--settings` file, `-c notify=`, `herdbot-pi.ts`, `.cursor/hooks.json` or none)
    - that `herdbot hook` always exits 0 silently and logs to `.state/events/hook-errors.log`
    - that your own codex `notify` is re-run with the same payload
    - that `--settings` hooks stack with your own (Task 1's finding)
    - that main-agent turns are logged to `<slug>/turns.jsonl`
  - Bot layout block: add `<slug>/turns.jsonl  # main-agent prompt/reply pairs, from turn hooks` and `.state/events/<agent>.jsonl  # hook events waiting for the daemon`.
  - The known limitation about reading a claude pane's history returning stale text: note that claude subagent results no longer depend on it, while quota detection still does.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document turn hooks"
```

If cursor was wired, add `crates/herdbot/src/hook.rs crates/herdbot/src/route.rs` to the `git add`, and use `git commit -m "feat(herdbot): cursor turn hook, and document turn hooks"`.
