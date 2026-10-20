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
const FAILURE_WORDS: &[&str] = &["limit", "quota", "429", "401", "403", "/login", "authentication", "credential", "token", "expired"];
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
const CITE_LINES: usize = 12;

pub fn accepted(verdict: &Verdict, tail: &str) -> bool {
    let (words, window) = match verdict.kind {
        TailKind::Quota | TailKind::Auth => (FAILURE_WORDS, CITE_LINES),
        TailKind::Approval => (APPROVAL_WORDS, SCAN_LINES),
        TailKind::Done | TailKind::Working => return true,
    };
    let line = verdict.line.to_ascii_lowercase();
    let lines = recent(tail);
    let window = &lines[lines.len().saturating_sub(window)..];
    !line.is_empty() && words.iter().any(|w| line.contains(w)) && cites_rows(&line, window)
}

/// Shortest cited text (whitespace removed) that counts: "limit" alone proves nothing.
const MIN_CITE_CHARS: usize = 12;

/// Whether `line` appears in the bottom `rows` of the pane, ignoring case and whitespace. A
/// narrow pane wraps one message over several rows, and fm may cite only a piece of it.
fn cites_rows(line: &str, rows: &[&str]) -> bool {
    let squash = |text: &str| -> String {
        text.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect()
    };
    let wanted = squash(line);
    wanted.chars().count() >= MIN_CITE_CHARS && squash(&rows.concat()).contains(&wanted)
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

/// Deliveries held for one agent before delivering anyway (about 2 min at the 15 s retry), so
/// a misread prompt cannot stall a queue for good.
pub const MAX_HOLDS: u32 = 8;

/// Whether to hold a delivery to an "idle" agent because its pane shows a prompt.
pub fn hold(verdict: Option<&Verdict>, tail: &str, times_held: u32) -> bool {
    times_held < MAX_HOLDS
        && verdict.is_some_and(|v| v.kind == TailKind::Approval && accepted(v, tail))
}

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
        // fm calls "fixed the login bug in auth.rs" an auth failure; bare "login"/"auth" must not pass.
        let fixed = "I fixed the login bug in auth.rs; tests pass.\n";
        assert!(!accepted(&v(TailKind::Auth, "I fixed the login bug in auth.rs; tests pass."), fixed));
        // A narrow pane wraps one message over several rows; fm may cite it with the breaks.
        let wrapped = "noise\nAPI Error: 429 rate_li\nmit_error: weekly\n limit\n";
        assert!(accepted(&v(TailKind::Quota, "API Error: 429 rate_limit_error: weekly limit"), wrapped));
        assert!(accepted(&v(TailKind::Quota, "429\nrate_li\nmit_err\nor:"), wrapped));
        assert!(!accepted(&v(TailKind::Quota, "weekly limit"), wrapped));
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
}
