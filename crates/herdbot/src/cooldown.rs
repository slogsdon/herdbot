//! Harnesses that just hit a subscription quota or an auth failure. Routing skips them
//! until `until` passes; one file per harness at `.state/cooldown/<harness>.json`.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::home::{self, Result};

/// Used when the output names no reset time (auth failures, unparsed quota text).
const DEFAULT_SECS: i64 = 3600;
/// Added to a parsed reset time so the first retry lands after the reset, not on it.
const MARGIN_SECS: i64 = 60;
/// A parsed reset further out than this is a misparse; fall back to the default.
const MAX_SECS: i64 = 8 * 24 * 3600;
/// Only the bottom of the pane counts. Older lines are conversation, which can quote these
/// strings (a bot working on herdbot, or a forwarded "[subagent … failed]" message).
const SCAN_LINES: usize = 15;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cooldown {
    /// Unix seconds.
    pub until: i64,
    pub reason: String,
}

impl Cooldown {
    pub fn active(&self, now: i64) -> bool {
        self.until > now
    }

    /// `until` as local "HH:MM", for messages.
    pub fn until_local(&self) -> String {
        DateTime::from_timestamp(self.until, 0)
            .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
            .unwrap_or_default()
    }
}

fn path(harness: &str) -> std::path::PathBuf {
    home::root()
        .join(".state")
        .join("cooldown")
        .join(format!("{harness}.json"))
}

/// The harness's cooldown, if one is still running.
pub fn active(harness: &str) -> Option<Cooldown> {
    let c: Cooldown = serde_json::from_str(&std::fs::read_to_string(path(harness)).ok()?).ok()?;
    c.active(home::now()).then_some(c)
}

pub fn record(harness: &str, cooldown: &Cooldown) -> Result<()> {
    let path = path(harness);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_vec_pretty(cooldown)?)?;
    Ok(())
}

/// Drop TUI gutter glyphs (⎿, ■, spaces) and normalise the curly apostrophe. Markers must
/// start the trimmed line: quoted or forwarded text has something in front of them.
fn trimmed(line: &str) -> String {
    line.trim_start_matches(|c: char| !c.is_alphanumeric())
        .replace('’', "'")
}

/// Quota lines start with "You've hit your … limit" (claude: "You've hit your session limit ·
/// resets 12:30am"; codex: "You've hit your usage limit. … Try again at 3:45 PM."). cursor's
/// quota text comes from its server, not the CLI, so cursor has auth detection only.
fn quota_line(harness: &str, line: &str) -> bool {
    let text = trimmed(line);
    matches!(harness, "claude" | "codex")
        && text.starts_with("You've hit your")
        && text.contains("limit")
}

/// Auth-failure lines, verified against the installed CLIs (2026-10-20): claude 2.1.304,
/// codex 0.163.0, cursor-agent 2026.10.16. Assumption: the opencode-go / codex
/// `api_key_expired` body shows on a line that also carries the 401 status; no capture yet.
fn auth_line(harness: &str, line: &str) -> bool {
    let text = trimmed(line);
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| text.starts_with(p));
    let expired_key = text.contains("401") && text.contains("api_key_expired");
    match harness {
        // "API Error: 401 Invalid API key · Please run /login"
        "claude" => {
            text.starts_with("API Error")
                && (text.contains("/login") || text.contains("authentication_error"))
        }
        "codex" => {
            starts(&[
                "Your access token could not be refreshed",
                "OAuth refresh token was rejected",
            ]) || expired_key
        }
        "cursor" => starts(&[
            "Your stored authentication is invalid",
            "Error: Authentication required",
            "If you set CURSOR_API_KEY, check that it is correct",
        ]),
        "pi" => expired_key,
        _ => false,
    }
}

/// A cooldown for `harness` if the bottom of `tail` shows a quota or auth failure.
pub fn detect(harness: &str, tail: &str, now: DateTime<Local>) -> Option<Cooldown> {
    let lines: Vec<&str> = tail.lines().filter(|l| !l.trim().is_empty()).collect();
    let recent = &lines[lines.len().saturating_sub(SCAN_LINES)..];
    let fallback = now.timestamp() + DEFAULT_SECS;
    if let Some(line) = recent.iter().rev().find(|l| quota_line(harness, l)) {
        let until = reset_time(line, now)
            .map(|t| t.timestamp() + MARGIN_SECS)
            .filter(|&t| t > now.timestamp() && t - now.timestamp() <= MAX_SECS)
            .unwrap_or(fallback);
        return Some(Cooldown {
            until,
            reason: format!("quota: {}", line.trim()),
        });
    }
    recent
        .iter()
        .rev()
        .find(|l| auth_line(harness, l))
        .map(|line| Cooldown {
            until: fallback,
            reason: format!("auth: {}", line.trim()),
        })
}

/// claude "resets 12:30am"; codex "[t|T]ry again at 3:45 PM." or "… at Oct 21st, 2026
/// 3:45 PM.". Assumption: claude may also print a date or zone after the time; those fall
/// back to the next occurrence of the clock time, or to the default when unparsed.
fn reset_time(line: &str, now: DateTime<Local>) -> Option<DateTime<Local>> {
    // ASCII lowering keeps byte offsets, so indices found in `lower` slice `line`.
    let lower = line.to_ascii_lowercase();
    let after = |key: &str| lower.find(key).map(|i| line[i + key.len()..].trim());
    let rest = after("try again at ").or_else(|| after("resets "))?;
    let rest = rest.trim_end_matches('.');
    if let Some(dt) = codex_date(rest) {
        return Local.from_local_datetime(&dt).earliest();
    }
    let time = clock(rest)?;
    let today = Local
        .from_local_datetime(&now.date_naive().and_time(time))
        .earliest()?;
    Some(if today > now {
        today
    } else {
        today + chrono::Duration::days(1)
    })
}

/// "Oct 21st, 2026 3:45 PM" (codex's format for a reset on another day).
fn codex_date(text: &str) -> Option<NaiveDateTime> {
    let (month_day, year_time) = text.split_once(", ")?;
    let (month, day) = month_day.split_once(' ')?;
    let day = day.trim_end_matches(|c: char| c.is_alphabetic());
    let (year, time) = year_time.split_once(' ')?;
    let date = NaiveDate::parse_from_str(&format!("{month} {day} {year}"), "%b %d %Y").ok()?;
    Some(date.and_time(clock(time)?))
}

/// The clock time at the start of `text`: "12:30am", "3pm", "3:45 PM".
fn clock(text: &str) -> Option<NaiveTime> {
    let compact: String = text
        .split_whitespace()
        .take(2)
        .collect::<String>()
        .to_lowercase();
    let end = [compact.find("am"), compact.find("pm")]
        .into_iter()
        .flatten()
        .min()?
        + 2;
    let token = &compact[..end];
    NaiveTime::parse_from_str(token, "%I:%M%p")
        .or_else(|_| {
            NaiveTime::parse_from_str(
                &token.replace("am", ":00am").replace("pm", ":00pm"),
                "%I:%M%p",
            )
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, 20, h, m, 0).unwrap()
    }

    #[test]
    fn claude_session_limit_uses_reset_time() {
        let tail = "some work\n  ⎿  You’ve hit your session limit · resets 12:30am\n\n> \n";
        let c = detect("claude", tail, at(22, 0)).unwrap();
        // 12:30am has passed today, so tomorrow.
        assert_eq!(
            c.until,
            Local
                .with_ymd_and_hms(2026, 10, 21, 0, 31, 0)
                .unwrap()
                .timestamp()
        );
        assert!(c.reason.starts_with("quota:"));
    }

    #[test]
    fn codex_usage_limit_same_day_and_dated() {
        let line = "■ You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/settings/usage to purchase more credits or try again at 3:45 PM.";
        let c = detect("codex", line, at(9, 0)).unwrap();
        assert_eq!(c.until, at(15, 46).timestamp());
        let line = "■ You've hit your usage limit. Try again at Oct 22nd, 2026 3:45 PM.";
        let c = detect("codex", line, at(9, 0)).unwrap();
        assert_eq!(
            c.until,
            Local
                .with_ymd_and_hms(2026, 10, 22, 15, 46, 0)
                .unwrap()
                .timestamp()
        );
        let line = "■ You've hit your usage limit. Try again at 3:45 PM.";
        let c = detect("codex", line, at(9, 0)).unwrap();
        assert_eq!(c.until, at(15, 46).timestamp());
    }

    #[test]
    fn auth_failures_default_to_an_hour() {
        let now = at(9, 0);
        for (harness, line) in [
            (
                "claude",
                "API Error: 401 Invalid API key · Please run /login",
            ),
            (
                "codex",
                "Your access token could not be refreshed. Please log out and sign in again.",
            ),
            (
                "cursor",
                "Your stored authentication is invalid. Please log in again.",
            ),
            ("pi", r#"401 {"error":{"code":"api_key_expired"}}"#),
        ] {
            let c = detect(harness, line, now).unwrap_or_else(|| panic!("{harness}"));
            assert_eq!(c.until, now.timestamp() + DEFAULT_SECS, "{harness}");
            assert!(c.reason.starts_with("auth:"));
        }
    }

    #[test]
    fn ignores_quoted_or_old_mentions() {
        let now = at(9, 0);
        // Mid-sentence quote: not a line start.
        assert!(
            detect(
                "claude",
                "the agent replied \"You've hit your session limit\"",
                now
            )
            .is_none()
        );
        // Real line, but scrolled above the scan window.
        let mut tail = String::from("You've hit your session limit · resets 12:30am\n");
        tail.push_str(&"later output\n".repeat(SCAN_LINES));
        assert!(detect("claude", &tail, now).is_none());
        // Another harness's wording.
        assert!(detect("pi", "You've hit your usage limit.", now).is_none());
        // Auth text inside a reply, and the all-cooled resolve error a parent receives.
        assert!(
            detect(
                "claude",
                "If you see \"Please run /login\", log in again.",
                now
            )
            .is_none()
        );
        let resolve_error = "Re-queueing failed: no available route for class \"default\": claude:claude-sonnet-5-5: cooling down until 01:42 (auth); pi:kimi-k2.7-code:medium: cooling down until 01:42 (auth)";
        for harness in ["claude", "codex", "cursor", "pi"] {
            assert!(detect(harness, resolve_error, now).is_none(), "{harness}");
        }
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock("12:30am"), NaiveTime::from_hms_opt(0, 30, 0));
        assert_eq!(
            clock("3pm (Europe/London)"),
            NaiveTime::from_hms_opt(15, 0, 0)
        );
        assert_eq!(clock("3:45 PM"), NaiveTime::from_hms_opt(15, 45, 0));
        assert_eq!(clock("soon"), None);
    }

    #[test]
    fn expiry() {
        let c = Cooldown {
            until: 100,
            reason: String::new(),
        };
        assert!(c.active(99));
        assert!(!c.active(100));
    }
}
