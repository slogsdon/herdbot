# On-Device `fm` Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Use Apple's on-device model (`/usr/bin/fm`, macOS 27) to read pane tails when herdbot's string matching has no answer. That means catching quota and auth failures it misses, and holding a delivery while a modal is up. Optionally it also picks a task class when a bot names none.

**Architecture:** A new `src/fm.rs` runs `fm respond --no-stream -g --schema <file>` under a 10 s timeout and parses a `{kind, line}` verdict. fm is advisory only:
- A failure verdict counts only if its `line` is one of the last 5 non-empty pane lines and contains a failure keyword.
- An fm-sourced cooldown lasts 10 min.
- It applies to **subagent runs only**. A misread there re-runs one task. A misread for a main agent would restart it and lose its conversation, so `watch_bots` keeps the string matchers alone. herdbot-dev's turns routinely end on `cooldown.rs` fixtures like `You've hit your session limit`.

With fm missing or unlicensed, every function returns `None` and herdbot behaves exactly as today.

**Tech Stack:** Rust 2024, tokio (`process` feature added), `fm` CLI.

## Global Constraints

- fm is optional. No code path may fail or wait on it when `fm available` is not exit 0.
- Call fm only on state transitions the daemon already handles (a run settling or blocking, a main agent finishing a turn, a delivery to an idle agent). Never call it every tick.
- Send fm at most the last 15 non-empty lines (`cooldown::SCAN_LINES`). The on-device context window is small.
- An fm verdict never restarts a main agent. Only `requeue_if_cooled` (subagent runs) may act on an fm quota or auth verdict.
- Never send fm anything but pane text from this machine. It runs on-device.
- Tests stay in-file `#[cfg(test)]`. Unit tests never run the real `fm`. Live checks are labeled **live**.
- Commits: conventional, explicit paths only.

## Verified facts (2026-10-20, macOS 27, license accepted)

- `fm available` prints `System model available` and exits 0.
- `fm respond --no-stream -g -i <instructions> --schema <file> <prompt>` prints one JSON object, taking 0.45 s warm and 1.8 s cold.
- Schemas must be in fm's own format (`title`, `x-order`, `additionalProperties`), as made by `fm schema object`. A plain JSON Schema string is rejected (`Invalid schema … missing`).
- Sample verdicts with the Task 1 schema:
  - `API Error: 429 rate_limit_error: This request would exceed your weekly limit` gives `{"kind":"quota","line":"API Error: 429 …"}`. The current matcher misses this.
  - `I fixed the login bug in auth.rs; tests pass.` gives `{"kind":"done","line":""}`.

---

### Task 1: `fm.rs`: availability, one call, verdict parsing and acceptance

**Files:**
- Create: `crates/herdbot/src/fm.rs`
- Modify: `crates/herdbot/src/main.rs:3-11` (add `mod fm;`)
- Modify: `crates/herdbot/Cargo.toml:13` (tokio features `["io-std", "process"]`)
- Modify: `crates/herdbot/src/cooldown.rs:17` (`const SCAN_LINES` → `pub const SCAN_LINES`)

**Interfaces:**
- Produces:
  - `pub enum TailKind { Quota, Auth, Approval, Done, Working }` (`Debug, Clone, Copy, PartialEq`)
  - `pub struct Verdict { pub kind: TailKind, pub line: String }`
  - `pub fn parse_verdict(json: &str) -> Option<Verdict>`
  - `pub fn recent(tail: &str) -> Vec<&str>`: the last `SCAN_LINES` non-empty lines
  - `pub fn accepted(verdict: &Verdict, tail: &str) -> bool`
  - `pub async fn classify_tail(tail: &str) -> Option<Verdict>`: `None` when fm is unavailable, times out, or returns junk
  - `pub fn available() -> bool` (cached per process)
  - `async fn respond(instructions: &str, schema_name: &str, schema: &str, prompt: &str) -> Option<String>` (module-private, reused by Task 4)

- [ ] **Step 1: Write the failing tests.** Create `crates/herdbot/src/fm.rs` containing only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_kinds_only() {
        assert_eq!(
            parse_verdict(r#"{"kind": "quota", "line": "API Error: 429 limit"}"#),
            Some(Verdict { kind: TailKind::Quota, line: "API Error: 429 limit".into() })
        );
        assert_eq!(parse_verdict(r#"{"kind": "Done", "line": ""}"#).unwrap().kind, TailKind::Done);
        assert_eq!(parse_verdict(r#"{"kind": "maybe", "line": ""}"#), None);
        assert_eq!(parse_verdict("not json"), None);
    }

    #[test]
    fn failure_verdicts_need_a_real_line_with_a_keyword() {
        let tail = "Compiling herdbot\nAPI Error: 429 rate_limit_error: weekly limit\n> \n";
        let v = |kind, line: &str| Verdict { kind, line: line.into() };
        // Verbatim (after trimming) and has a keyword.
        assert!(accepted(&v(TailKind::Quota, "API Error: 429 rate_limit_error: weekly limit"), tail));
        // Paraphrased: not in the tail.
        assert!(!accepted(&v(TailKind::Quota, "rate limit hit"), tail));
        // Empty line.
        assert!(!accepted(&v(TailKind::Auth, ""), tail));
        // In the tail but no failure keyword.
        assert!(!accepted(&v(TailKind::Auth, "Compiling herdbot"), tail));
        // More than CITE_LINES from the bottom: conversation, not the failure that ended the turn.
        let mut old = String::from("API Error: 401 please log in\n");
        old.push_str(&"output\n".repeat(CITE_LINES));
        assert!(!accepted(&v(TailKind::Auth, "API Error: 401 please log in"), &old));
        // Non-failure kinds need no line.
        assert!(accepted(&v(TailKind::Done, ""), tail));
    }

    #[test]
    fn approval_needs_a_prompt_keyword() {
        let tail = "Do you trust the files in this folder?\n  1. Yes  2. No\n";
        let v = |line: &str| Verdict { kind: TailKind::Approval, line: line.into() };
        assert!(accepted(&v("Do you trust the files in this folder?"), tail));
        // A bare menu has no approval word, so it does not hold a delivery.
        assert!(!accepted(&v("1. Yes  2. No"), "1. Yes  2. No\n"));
        assert!(!accepted(&v(""), tail));
    }
}
```

- [ ] **Step 2: Run them to verify they fail** (**unit**)

Run: `cargo test -p herdbot fm::`
Expected: compile errors (`parse_verdict`, `Verdict`, `TailKind`, `accepted` not found). Add `mod fm;` to `main.rs` first so the module compiles at all.

- [ ] **Step 3: Implement.** Put this above the test module in `fm.rs`:

```rust
//! Apple's on-device model (`fm`, macOS 27) as a second reader of pane tails. Advisory:
//! callers treat `None` as "no opinion", and a verdict counts only when the line it cites is
//! really at the bottom of the pane and looks like a failure or a prompt.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::cooldown::SCAN_LINES;
use crate::home;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Made with `fm schema object --name Tail --string kind --description … --string line
/// --description …`; fm rejects plain JSON Schema.
const TAIL_SCHEMA: &str = r#"{
  "title": "Tail", "type": "object", "additionalProperties": false,
  "required": ["kind", "line"], "x-order": ["kind", "line"],
  "properties": {
    "kind": { "type": "string", "description": "quota, auth, approval, done or working" },
    "line": { "type": "string", "description": "the exact line from the text that shows it, or empty" }
  }
}"#;

const TAIL_INSTRUCTIONS: &str = "Classify the bottom of a coding-agent terminal. kind is exactly one of: quota (a usage or rate limit was hit), auth (login or credentials failed), approval (waiting for the user to approve or confirm something), done (finished and waiting for input), working (anything else). line is the single line, copied exactly, that shows it.";

/// A failure line must contain one of these (lowercased); fm alone never cools a harness.
const FAILURE_WORDS: &[&str] = &["limit", "quota", "429", "401", "403", "login", "auth", "credential", "token", "expired"];
const APPROVAL_WORDS: &[&str] = &["allow", "approve", "trust", "permission", "proceed", "continue?", "(y/n)", "[y/n]"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TailKind {
    Quota,
    Auth,
    Approval,
    Done,
    Working,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub kind: TailKind,
    pub line: String,
}

pub fn parse_verdict(json: &str) -> Option<Verdict> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    let kind = match v["kind"].as_str()?.trim().to_ascii_lowercase().as_str() {
        "quota" => TailKind::Quota,
        "auth" => TailKind::Auth,
        "approval" => TailKind::Approval,
        "done" => TailKind::Done,
        "working" => TailKind::Working,
        _ => return None,
    };
    Some(Verdict { kind, line: v["line"].as_str().unwrap_or_default().trim().to_owned() })
}

/// The last `SCAN_LINES` non-empty lines, trimmed: all fm ever sees, and all a cited line may
/// come from.
pub fn recent(tail: &str) -> Vec<&str> {
    let lines: Vec<&str> = tail.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(SCAN_LINES)..].to_vec()
}

/// A cited failure line must be this close to the bottom: a turn that died on a quota ends
/// on it, while quoted fixtures and diffs sit higher up.
const CITE_LINES: usize = 5;

pub fn accepted(verdict: &Verdict, tail: &str) -> bool {
    let (words, window) = match verdict.kind {
        TailKind::Quota | TailKind::Auth => (FAILURE_WORDS, CITE_LINES),
        TailKind::Approval => (APPROVAL_WORDS, SCAN_LINES),
        TailKind::Done | TailKind::Working => return true,
    };
    let line = verdict.line.to_ascii_lowercase();
    let lines = recent(tail);
    !line.is_empty()
        && lines[lines.len().saturating_sub(window)..]
            .iter()
            .any(|l| l.to_ascii_lowercase() == line)
        && words.iter().any(|w| line.contains(w))
}

/// `fm available` exits 0 once the model is present and its terms are accepted.
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::process::Command::new("fm")
            .arg("available")
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

/// One `fm respond` with a schema written under `.state/fm/`. None on any failure or timeout.
async fn respond(instructions: &str, schema_name: &str, schema: &str, prompt: &str) -> Option<String> {
    if !available() {
        return None;
    }
    let dir = home::root().join(".state").join("fm");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{schema_name}.json"));
    if std::fs::read_to_string(&path).ok().as_deref() != Some(schema) {
        std::fs::write(&path, schema).ok()?;
    }
    let child = tokio::process::Command::new("fm")
        .args(["respond", "--no-stream", "-g", "-i", instructions, "--schema"])
        .arg(&path)
        .arg(prompt)
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(TIMEOUT, child).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// fm's reading of the bottom of a pane.
pub async fn classify_tail(tail: &str) -> Option<Verdict> {
    let text = recent(tail).join("\n");
    if text.is_empty() {
        return None;
    }
    let verdict = parse_verdict(&respond(TAIL_INSTRUCTIONS, "tail", TAIL_SCHEMA, &text).await?)?;
    println!("fm: {:?} {:?}", verdict.kind, verdict.line);
    Some(verdict)
}
```

Make `SCAN_LINES` in `cooldown.rs` `pub const`, and set tokio features to `["io-std", "process"]` in `crates/herdbot/Cargo.toml`.

- [ ] **Step 4: Run the tests to verify they pass** (**unit**)

Run: `cargo test -p herdbot`
Expected: 16 passed. `fm::tests` adds 3 to the existing 13.

- [ ] **Step 5: Live check against the real model** (**live**, no herdr needed). Add this temporary test, run it, then delete it before committing:

```rust
    #[tokio::test]
    #[ignore]
    async fn live_fm() {
        let v = classify_tail("Compiling herdbot\nAPI Error: 429 rate_limit_error: This request would exceed your weekly limit\n> \n").await.unwrap();
        assert_eq!(v.kind, TailKind::Quota);
        assert!(accepted(&v, "API Error: 429 rate_limit_error: This request would exceed your weekly limit"));
        let v = classify_tail("I fixed the login bug in auth.rs; tests pass.\n> \n").await.unwrap();
        assert_ne!(v.kind, TailKind::Auth);
    }
```

Run: `cargo test -p herdbot live_fm -- --ignored --nocapture`
Expected: PASS. Two `fm:` lines print.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/fm.rs crates/herdbot/src/main.rs crates/herdbot/src/cooldown.rs crates/herdbot/Cargo.toml Cargo.lock
git commit -m "feat(herdbot): read pane tails with Apple's on-device model when available"
```

---

### Task 2: fm as a backstop for quota and auth detection on subagent runs

Subagents only. In `watch_bots` a misread would restart a main agent and throw away its conversation, so main agents keep `cooldown::detect` alone. Don't touch `watch_bots`.

**Files:**
- Modify: `crates/herdbot/src/fm.rs` (add `cooldown_backstop` plus a test)
- Modify: `crates/herdbot/src/daemon.rs:197-203` (`requeue_if_cooled` only)

**Interfaces:**
- Consumes: Task 1's `classify_tail`, `accepted`, `Verdict`, `TailKind`, and `cooldown::Cooldown`.
- Produces: `pub async fn cooldown_backstop(tail: &str, now: i64) -> Option<Cooldown>` in `fm.rs`, and `fn verdict_cooldown(v: &Verdict, tail: &str, now: i64) -> Option<Cooldown>` (pure, tested). Plus `async fn detect_failure(harness: &str, tail: &str) -> Option<cooldown::Cooldown>` in `daemon.rs`.

- [ ] **Step 1: Write the failing test** in `fm.rs` tests:

```rust
    #[test]
    fn fm_cooldowns_are_short_and_marked() {
        let tail = "API Error: 429 rate_limit_error: weekly limit\n";
        let quota = Verdict { kind: TailKind::Quota, line: "API Error: 429 rate_limit_error: weekly limit".into() };
        let c = verdict_cooldown(&quota, tail, 1000).unwrap();
        assert_eq!(c.until, 1000 + FM_COOLDOWN_SECS);
        assert!(c.reason.starts_with("quota (fm): API Error: 429"));
        let made_up = Verdict { kind: TailKind::Quota, line: "limit".into() };
        assert_eq!(verdict_cooldown(&made_up, tail, 1000), None);
        let done = Verdict { kind: TailKind::Done, line: String::new() };
        assert_eq!(verdict_cooldown(&done, tail, 1000), None);
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot fm_cooldowns`
Expected: compile error, `verdict_cooldown` not found.

- [ ] **Step 3: Implement** in `fm.rs`:

```rust
/// An fm-only cooldown is short: a misread costs ten minutes of one harness, not an hour.
const FM_COOLDOWN_SECS: i64 = 600;

fn verdict_cooldown(v: &Verdict, tail: &str, now: i64) -> Option<crate::cooldown::Cooldown> {
    let kind = match v.kind {
        TailKind::Quota => "quota",
        TailKind::Auth => "auth",
        _ => return None,
    };
    accepted(v, tail).then(|| crate::cooldown::Cooldown {
        until: now + FM_COOLDOWN_SECS,
        // "quota (fm)" before the colon: daemon messages show only that part.
        reason: format!("{kind} (fm): {}", v.line),
    })
}

/// A cooldown when the string matchers found nothing but fm reads a quota or auth failure.
pub async fn cooldown_backstop(tail: &str, now: i64) -> Option<crate::cooldown::Cooldown> {
    verdict_cooldown(&classify_tail(tail).await?, tail, now)
}
```

In `daemon.rs`, add `fm` to `use crate::{cooldown, fm, grok, run};` and add:

```rust
/// The harness's own failure text first; fm only when that finds nothing. Subagent runs only:
/// a misread here re-runs one task, while for a main agent it would cost the conversation.
async fn detect_failure(harness: &str, tail: &str) -> Option<cooldown::Cooldown> {
    let now = chrono::Local::now();
    match cooldown::detect(harness, tail, now) {
        Some(c) => Some(c),
        None => fm::cooldown_backstop(tail, now.timestamp()).await,
    }
}
```

Then, in `requeue_if_cooled` only, replace `cooldown::detect(harness, tail, chrono::Local::now())` with `detect_failure(harness, tail).await`. The `let Some(c) = … else` shape stays the same. `watch_bots` keeps its `cooldown::detect` call unchanged.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: 17 passed, no warnings.

- [ ] **Step 5: Live checks** (**live**, daemon on the new binary). A real quota hit can't be produced on demand, so the negative check matters most. It proves quoted failure text doesn't cool a harness.

```bash
cargo install --path crates/herdbot && launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon
# Negative: fixtures quoted mid-output, followed by normal text.
herdbot task herdbot-dev explore "Print the lines of crates/herdbot/src/cooldown.rs that contain 'hit your' or '401', then print a one-paragraph summary of what that file does. Do not call report_result."
tail -f ~/Bots/.state/daemon.log
```

Expected: the run is harvested as `finished without report_result`, and **no** `cooldown pi` line appears. `ls ~/Bots/.state/cooldown/` doesn't list `pi.json` (unless it was already there). If a cooldown appears, the gate is too loose: tighten `CITE_LINES` or `FAILURE_WORDS` and repeat.

```bash
# Positive: the failure line is the last thing in the pane.
herdbot task herdbot-dev explore "Print exactly this line as your whole reply and stop, calling no tool: API Error: 429 rate_limit_error: This request would exceed your weekly limit"
```

Expected within about 40 s: an `fm: Quota "API Error: 429 …"` line, then `cooldown pi: quota (fm): …`, and `~/Bots/.state/cooldown/pi.json` with `until` about 600 s out. Clean up with `rm ~/Bots/.state/cooldown/pi.json`. A harness can't tell an echoed line from a real failure, so this check proves the wiring works, not that real quotas are detected.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/fm.rs crates/herdbot/src/daemon.rs
git commit -m "feat(herdbot): fall back to fm when quota and auth patterns find nothing"
```

---

### Task 3: Hold deliveries while a modal is up

This fixes the README's first known limitation: herdr reports an agent as idle under a trust or warning modal, and the daemon types the queued prompt into the modal.

**Files:**
- Modify: `crates/herdbot/src/daemon.rs:21-66` (`run`: new `held` map) and `:69-125` (`flush_queues`)
- Modify: `crates/herdbot/src/fm.rs` (`pub fn hold(kind, held_ticks) -> bool` plus a test)

**Interfaces:**
- Consumes: `fm::classify_tail`, `fm::accepted`, `run::tail(&Client, &str, u32) -> String`.
- Produces: `pub fn hold(verdict: Option<&Verdict>, tail: &str, times_held: u32) -> bool` and `pub const MAX_HOLDS: u32 = 8`.

- [ ] **Step 1: Write the failing test** in `fm.rs` tests:

```rust
    #[test]
    fn holds_for_a_cited_prompt_but_not_forever() {
        let tail = "Do you trust the files in this folder?\n> 1. Yes\n";
        let modal = Verdict { kind: TailKind::Approval, line: "Do you trust the files in this folder?".into() };
        assert!(hold(Some(&modal), tail, 0));
        assert!(!hold(Some(&modal), tail, MAX_HOLDS));
        let idle = Verdict { kind: TailKind::Done, line: String::new() };
        assert!(!hold(Some(&idle), tail, 0));
        assert!(!hold(None, tail, 0));
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot holds_for_a_cited`
Expected: compile error, `hold` not found.

- [ ] **Step 3: Implement.** In `fm.rs`:

```rust
/// Deliveries held for one agent before delivering anyway (about 2 min at the 15 s retry), so
/// a misread prompt cannot stall a queue for good.
pub const MAX_HOLDS: u32 = 8;

/// Whether to hold a delivery to an "idle" agent because its pane shows a prompt.
pub fn hold(verdict: Option<&Verdict>, tail: &str, times_held: u32) -> bool {
    times_held < MAX_HOLDS
        && verdict.is_some_and(|v| v.kind == TailKind::Approval && accepted(v, tail))
}
```

In `daemon.rs`: add `let mut held: HashMap<String, u32> = HashMap::new();` in `run()`, pass `&mut held` as a new last parameter of `flush_queues`, and in `flush_queues`, right after the `if status != "idle" && status != "done" { continue; }` check:

```rust
        if fm::available() {
            let tail = run::tail(herdr, &agent, 60).await;
            let times = held.get(&agent).copied().unwrap_or(0);
            let verdict = fm::classify_tail(&tail).await;
            if fm::hold(verdict.as_ref(), &tail, times) {
                held.insert(agent.clone(), times + 1);
                // Retry after DELIVERY_COOLDOWN, not every tick.
                delivered_at.insert(agent.clone(), home::now());
                eprintln!("{agent}: holding delivery, its pane shows a prompt ({}/{})", times + 1, fm::MAX_HOLDS);
                continue;
            }
            // Reset only once fm stops seeing a prompt. After MAX_HOLDS the count stays
            // saturated, so a pane fm keeps misreading is held once, not on every message.
            if !verdict.is_some_and(|v| v.kind == fm::TailKind::Approval) {
                held.remove(&agent);
            }
        }
```

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: 18 passed, no warnings.

- [ ] **Step 5: Live check** (**live**). Make a codex bot in a fresh directory, where codex's first-run trust prompt appears:

```bash
mkdir -p /tmp/modal-test && herdbot new modal-test --cwd /tmp/modal-test
printf '\n[bots.modal-test]\nclasses.default = ["codex:gpt-6-luna:low"]\n' >> ~/Bots/routing.toml
herdbot send modal-test "Reply with the word PONG."
tail -f ~/Bots/.state/daemon.log
```

Expected: `modal-test: holding delivery, its pane shows a prompt (1/8)`. Accept the modal in the herdr pane, and on the next retry the daemon delivers, so PONG appears. Afterwards remove the `[bots.modal-test]` lines from `~/Bots/routing.toml`, then run `herdbot down modal-test && rm -r ~/Bots/modal-test /tmp/modal-test`. If codex shows no modal this time, record that the live check was not exercised. Don't call it passed.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/fm.rs crates/herdbot/src/daemon.rs
git commit -m "feat(herdbot): hold deliveries while fm sees a prompt in an idle pane"
```

---

### Task 4 (optional): pick a task class when the bot names none

Skip this unless bots are seen calling `task` without `subagent_type` often enough to matter. Check first with `grep -c '"class": "default"' ~/Bots/.runs/*.json | awk -F: '{s+=$2} END {print s}'`.

**Files:**
- Modify: `crates/herdbot/src/fm.rs` (`pick_class` plus a test)
- Modify: `crates/herdbot/src/tools.rs:110-116` (the `"task"` arm)

**Interfaces:**
- Produces: `pub fn parse_class(json: &str) -> Option<&'static str>` and `pub async fn pick_class(description: &str, prompt: &str) -> Option<&'static str>`. The result is always one of `CLASSES`.

- [ ] **Step 1: Write the failing test** in `fm.rs` tests:

```rust
    #[test]
    fn class_must_be_known() {
        assert_eq!(parse_class(r#"{"class": "explore"}"#), Some("explore"));
        assert_eq!(parse_class(r#"{"class": " Review "}"#), Some("review"));
        assert_eq!(parse_class(r#"{"class": "watch_video"}"#), None);
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot class_must_be_known`. Expected: compile error.

- [ ] **Step 3: Implement** in `fm.rs`:

```rust
const CLASSES: &[&str] = &["explore", "shell", "plan", "code", "review", "debug", "research", "media_review", "browser_use", "computer_use", "custom"];

const CLASS_SCHEMA: &str = r#"{
  "title": "Class", "type": "object", "additionalProperties": false,
  "required": ["class"], "x-order": ["class"],
  "properties": { "class": { "type": "string", "description": "explore, shell, plan, code, review, debug, research, media_review, browser_use, computer_use or custom" } }
}"#;

const CLASS_INSTRUCTIONS: &str = "Pick the task class for a coding-agent subagent. explore: read-only search of files or code. shell: run commands. plan: design an approach. code: write or change code. review: review code or text. debug: find a bug's cause. research: web search and synthesis. media_review: look at images or video. browser_use: drive a web browser. computer_use: drive desktop apps. custom: none of these.";

pub fn parse_class(json: &str) -> Option<&'static str> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    let wanted = v["class"].as_str()?.trim().to_ascii_lowercase();
    CLASSES.iter().copied().find(|c| *c == wanted)
}

pub async fn pick_class(description: &str, prompt: &str) -> Option<&'static str> {
    // The prompt can be long; its start says what the task is.
    let head: String = prompt.chars().take(2000).collect();
    let text = format!("{description}\n\n{head}");
    parse_class(&respond(CLASS_INSTRUCTIONS, "class", CLASS_SCHEMA, &text).await?)
}
```

In `tools.rs`, the `"task"` arm: replace the `let class = … .unwrap_or("default");` with

```rust
            let named = args.get("subagent_type").and_then(Value::as_str);
            let picked = match named {
                Some(_) => None,
                None => crate::fm::pick_class(arg(args, "description")?, arg(args, "prompt")?).await,
            };
            let class = named.or(picked).unwrap_or("default");
```

Also pass `class` into `run::spawn` as before. The run's stored `class` then shows the picked one.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`. Expected: 19 passed.

- [ ] **Step 5: Live check** (**live**). From a claude bot pane, ask it to call `task` with only `description: "find TODOs"` and `prompt: "List every TODO comment under crates/."`, with no subagent_type. Expected: the new run's `class` in `~/Bots/.runs/<id>.json` is `explore`, and its route is the explore route.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/fm.rs crates/herdbot/src/tools.rs
git commit -m "feat(herdbot): let fm pick a task class when none is given"
```

---

### Task 5: Docs

**Files:**
- Modify: `README.md` (the cooldown section after the harness table; "Two known limitations"; Roadmap)

- [ ] **Step 1: Edit.**
  - Cooldown section: add a paragraph. For subagent runs, when none of the patterns match and `fm available` succeeds, the daemon asks Apple's on-device model to read the same 15 lines. It counts a quota or auth verdict only when the line fm cites is one of the last 5 and contains a failure word, and that cooldown lasts 10 minutes. Main agents never act on fm, because a misread would restart them and lose the conversation. Daemon log lines start `fm:`.
  - Known limitation 1 (modal): replace "Accept the modal once in that pane and resend" with: when `fm` is available, the daemon holds the delivery (up to 8 retries, about 2 min) while fm reads a prompt in the pane, so accepting the modal lets it through. Without fm it is lost as before.
  - `.state/` layout block: add `.state/fm/            schemas handed to fm`.
  - Roadmap: remove the "Apple's on-device model" bullet. Note that summarizing subagent output was left out on purpose: the parent gets the raw tail, which loses nothing, while on-device summaries of long output would.
- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document the fm fallback"
```
