//! Recognises prompts that need the user. Key tables are fixed; pane text never supplies keys.

use herdbot_core::cards::{Card, CardOption, card_id};
use herdbot_core::home;

const CLAUDE_TRUST: &str = "claude-trust";
const PHRASE_ASK: &str = "Is this a project you created or one you trust";
const PHRASE_YES: &str = "Yes, I trust this folder";

pub fn recognize(tail: &str, bot: &str, agent: &str) -> Option<Card> {
    if !(tail.contains(PHRASE_ASK) && tail.contains(PHRASE_YES)) {
        return None;
    }
    let lines: Vec<&str> = tail.lines().collect();
    let start = lines
        .iter()
        .rposition(|line| line.contains("Accessing workspace"))?;
    let end = lines[start..]
        .iter()
        .position(|line| line.contains("Enter to confirm"))
        .map(|i| start + i)?;
    let body = lines[start..=end]
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    Some(Card {
        id: card_id(agent, &body),
        kind: "approval".into(),
        adapter: CLAUDE_TRUST.into(),
        bot: bot.to_owned(),
        agent: agent.to_owned(),
        title: "Trust this folder?".into(),
        body,
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
        at: home::now(),
        harness: Some("claude".into()),
        run_id: None,
    })
}

/// Recognize a harness prompt for display. Only the Claude trust adapter has answer keys;
/// other dialogs are operator-only cards and must be handled in the pane.
pub fn recognize_prompt(tail: &str, harness: &str, bot: &str, agent: &str, run_id: Option<&str>) -> Option<Card> {
    if harness == "claude" && let Some(mut card) = recognize(tail, bot, agent) {
        card.harness = Some(harness.to_owned());
        card.run_id = run_id.map(str::to_owned);
        return Some(card);
    }
    let lower = tail.to_lowercase();
    let codex_hooks = lower.contains("hooks need review")
        && (lower.contains("trust all") || lower.contains("review hooks"));
    let permission = lower.contains("permission required")
        || lower.contains("approval required")
        || lower.contains("waiting for your approval")
        || lower.contains("permissionrequest");
    if !(codex_hooks || permission) || tail.trim().is_empty() {
        return None;
    }
    let body = tail.lines().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    let choices = visible_choices(&body);
    Some(Card {
        id: card_id(agent, &body), kind: "operator_prompt".into(), adapter: "display-only".into(),
        bot: bot.to_owned(), agent: agent.to_owned(),
        title: if codex_hooks { "Codex hooks need review" } else { "Harness is waiting for input" }.into(),
        body, options: choices, at: home::now(),
        harness: Some(harness.to_owned()), run_id: run_id.map(str::to_owned),
    })
}

fn visible_choices(body: &str) -> Vec<CardOption> {
    let mut choices: Vec<String> = body.lines().filter_map(|line| {
        let text = line.trim().trim_start_matches(['›', '>', '•', ' ']).trim();
        let label = text.strip_prefix(|c: char| c.is_ascii_digit())
            .and_then(|rest| rest.strip_prefix('.').or_else(|| rest.strip_prefix(')')))
            .map(str::trim)
            .filter(|label| !label.is_empty())?;
        Some(label.to_owned())
    }).collect();
    choices.dedup();
    let choices: Vec<CardOption> = choices.into_iter().enumerate().map(|(i, label)| CardOption {
        id: format!("choice-{i}"), label, style: "default".into(),
    }).collect();
    choices
}

pub fn input_timeout_card(tail: &str, harness: &str, bot: &str, agent: &str, run_id: Option<&str>) -> Card {
    let body = if tail.trim().is_empty() {
        "Herdbot sent the task prompt twice, but the harness stayed idle and showed no accepted input.".to_owned()
    } else {
        format!("Herdbot sent the task prompt twice, but the harness stayed idle. Inspect the pane before continuing.\n\n{tail}")
    };
    Card {
        id: card_id(agent, &body), kind: "operator_prompt".into(), adapter: "display-only".into(),
        bot: bot.to_owned(), agent: agent.to_owned(), title: "Task prompt was not accepted".into(),
        body, options: Vec::new(), at: home::now(), harness: Some(harness.to_owned()), run_id: run_id.map(str::to_owned),
    }
}

pub fn blocked_status_card(tail: &str, harness: &str, bot: &str, agent: &str, run_id: Option<&str>) -> Card {
    let body = if tail.trim().is_empty() { "The harness is blocked and needs operator input.".to_owned() } else { tail.to_owned() };
    let options = visible_choices(&body);
    Card {
        id: card_id(agent, &body), kind: "operator_prompt".into(), adapter: "display-only".into(),
        bot: bot.to_owned(), agent: agent.to_owned(), title: "Harness is waiting for input".into(),
        body, options, at: home::now(), harness: Some(harness.to_owned()), run_id: run_id.map(str::to_owned),
    }
}

pub fn keys(adapter: &str, option: &str) -> Option<&'static [&'static str]> {
    match (adapter, option) {
        (CLAUDE_TRUST, "trust") => Some(&["Down", "Enter"]),
        (CLAUDE_TRUST, "exit") => Some(&["Enter"]),
        _ => None,
    }
}

pub fn plan_answer(
    stored: &Card,
    option: &str,
    card_id: &str,
    live: Option<&Card>,
) -> Result<&'static [&'static str], String> {
    if stored.id != card_id {
        return Err(format!("no pending card {card_id} for {}", stored.agent));
    }
    match live {
        Some(live) if live.id == stored.id => {}
        _ => return Err("the prompt changed or is gone; nothing was sent".into()),
    }
    if !stored.options.iter().any(|item| item.id == option) {
        return Err(format!("unknown option {option}"));
    }
    keys(&stored.adapter, option).ok_or_else(|| format!("unknown option {option}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TRUST: &str = include_str!("../fixtures/panes/claude-trust.txt");
    const READY: &str = include_str!("../fixtures/panes/claude-ready.txt");

    #[test]
    fn the_real_dialog_becomes_an_approval_card() {
        let card = recognize(TRUST, "helper", "helper").expect("matches");
        assert_eq!(
            (card.kind.as_str(), card.adapter.as_str()),
            ("approval", "claude-trust")
        );
        assert_eq!(
            (card.bot.as_str(), card.agent.as_str()),
            ("helper", "helper")
        );
        assert_eq!(card.title, "Trust this folder?");
        assert!(card.body.contains("/Users/Shared/trustfresh-92905"));
        assert!(card.body.contains("Yes, I trust this folder"));
        assert!(!card.body.contains("claude --model"));
        let ids: Vec<_> = card
            .options
            .iter()
            .map(|o| (o.id.as_str(), o.style.as_str()))
            .collect();
        assert_eq!(ids, [("trust", "default"), ("exit", "destructive")]);
        assert_eq!(card.id, herdbot_core::cards::card_id("helper", &card.body));
    }

    #[test]
    fn the_id_follows_the_directory_in_the_dialog() {
        let other = TRUST.replace("trustfresh-92905", "trustfresh-11111");
        assert_ne!(
            recognize(TRUST, "b", "a").unwrap().id,
            recognize(&other, "b", "a").unwrap().id
        );
    }

    #[test]
    fn other_screens_do_not_match() {
        assert!(recognize(READY, "b", "a").is_none());
        assert!(recognize("", "b", "a").is_none());
        assert!(
            recognize(
                "Do you trust the files in this folder?\n1. Yes 2. No",
                "b",
                "a"
            )
            .is_none()
        );
        assert!(recognize("Is this a project you created or one you trust?", "b", "a").is_none());
        assert!(recognize("   Yes, I trust this folder", "b", "a").is_none());
    }

    #[test]
    fn codex_hooks_review_is_a_display_only_card_with_run_context() {
        let screen = "Hooks\n4 hooks need review before they can run.\n1. Review hooks\n2. Trust all and continue\n3. Continue without trusting\n";
        let card = recognize_prompt(screen, "codex", "alpha", "alpha-r1", Some("r1")).unwrap();
        assert_eq!(card.kind, "operator_prompt");
        assert_eq!(card.harness.as_deref(), Some("codex"));
        assert_eq!(card.run_id.as_deref(), Some("r1"));
        assert_eq!(card.options.len(), 3);
        assert!(keys(&card.adapter, &card.options[1].id).is_none());
    }

    #[test]
    fn unknown_permission_prompts_are_display_only() {
        let card = recognize_prompt("Permission required\nAllow access to the workspace?\n1) Allow\n2) Deny", "pi", "alpha", "alpha-r2", Some("r2")).unwrap();
        assert_eq!(card.kind, "operator_prompt");
        assert!(keys(&card.adapter, "choice").is_none());
    }

    #[test]
    fn the_key_table_is_fixed_and_checked() {
        assert_eq!(keys("claude-trust", "trust"), Some(&["Down", "Enter"][..]));
        assert_eq!(keys("claude-trust", "exit"), Some(&["Enter"][..]));
        assert_eq!(keys("claude-trust", "anything else"), None);
        assert_eq!(keys("unknown-adapter", "trust"), None);
    }

    #[test]
    fn answer_is_refused_unless_the_same_prompt_is_still_showing() {
        let stored = recognize(TRUST, "helper", "helper").unwrap();
        let id = stored.id.clone();
        assert_eq!(
            plan_answer(&stored, "trust", &id, Some(&stored)),
            Ok(&["Down", "Enter"][..])
        );
        assert_eq!(
            plan_answer(&stored, "exit", &id, Some(&stored)),
            Ok(&["Enter"][..])
        );
        assert!(
            plan_answer(&stored, "trust", "0000000000000000", Some(&stored))
                .unwrap_err()
                .contains("no pending card")
        );
        let moved = recognize(
            &TRUST.replace("trustfresh-92905", "trustfresh-1"),
            "helper",
            "helper",
        )
        .unwrap();
        assert!(
            plan_answer(&stored, "trust", &id, Some(&moved))
                .unwrap_err()
                .contains("prompt changed")
        );
        assert!(
            plan_answer(&stored, "trust", &id, None)
                .unwrap_err()
                .contains("prompt changed")
        );
        assert!(
            plan_answer(&stored, "always", &id, Some(&stored))
                .unwrap_err()
                .contains("unknown option")
        );
    }
}
