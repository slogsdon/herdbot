# Approval Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A config setting, `approvals = "ask" | "auto"`, that herdbot applies to every launch of a bot's main agent and its subagents. A bot whose top-level session runs in auto mode then gets subagents that use their harness's own automatic approval instead of asking Shane for every action.

**Architecture:** `herdbot.toml` gains `[approvals] mode` (default `"ask"`), and `bot.toml` gains an optional per-bot `approvals`. A pure `route::approval_args(harness, mode)` maps the mode to each harness's own flag. `route::launch` appends those flags. `run::start` resolves the bot's mode and passes it in, for main agents and subagents alike.

**Tech Stack:** Rust 2024, toml/serde (already dependencies). claude 2.1.304, codex 0.163.0, pi 1.2.0, cursor-agent 2026.10.19.

## Global Constraints

- `"ask"` is the default and adds no flags. Behaviour is unchanged unless someone sets `"auto"`.
- `"auto"` maps only to each harness's own automatic review, never to an approve-everything bypass:

  | Harness | Flag |
  |---|---|
  | claude | `--permission-mode auto` |
  | codex | `--approve-for-me` (automatic review in the workspace-write sandbox) |
  | pi | nothing (pi doesn't ask for tool approvals) |
  | cursor | nothing (its only option is `--force`/`--yolo`, which is a bypass) |

- Never add `--dangerously-skip-permissions`, `--permission-mode bypassPermissions`, `--dangerously-bypass-approvals-and-sandbox` or `--force`/`--yolo`.
- An unknown mode value is treated as `"ask"`, with one `eprintln!`.
- **Branch:** this plan runs on `feature/roadmap`, after plan 1 (pi built-in MCP) and before plan 2. `route::launch` there has the pi-MCP plan's `pi_args`. Later plans that change `launch` (turn hooks, cua) add their parameters after `approvals`.
- Tests stay in-file `#[cfg(test)]`. Commits: conventional, explicit paths, no AI attribution, never push.

---

### Task 1: Settings and the flag mapping

**Files:**
- Modify: `crates/herdbot/src/home.rs` (`Approvals` enum, `Bot.approvals`, `parse_approvals_default`, `approvals(bot)`, test)
- Modify: `crates/herdbot/src/route.rs` (`approval_args`, test)

**Interfaces:**
- Produces:
  - `pub enum Approvals { Ask, Auto }` (`Debug, Clone, Copy, PartialEq, Default = Ask`)
  - `pub fn parse_approvals(value: &str) -> Approvals`
  - `pub fn parse_approvals_default(herdbot_toml: &str) -> Approvals`
  - `pub fn approvals(bot: &Bot) -> Approvals`, where the bot's own setting wins over `herdbot.toml`
  - `route::approval_args(harness: &str, mode: home::Approvals) -> Vec<String>`

- [ ] **Step 1: Write the failing tests.** In `home.rs` tests:

```rust
    #[test]
    fn approvals_default_to_ask_and_bots_override() {
        assert_eq!(parse_approvals_default(""), Approvals::Ask);
        assert_eq!(parse_approvals_default("[approvals]\nmode = \"auto\"\n"), Approvals::Auto);
        assert_eq!(parse_approvals("auto"), Approvals::Auto);
        assert_eq!(parse_approvals("yolo"), Approvals::Ask); // unknown → ask
        let bot: Bot = toml::from_str("name = \"x\"\napprovals = \"auto\"\n").unwrap();
        assert_eq!(bot.approvals.as_deref(), Some("auto"));
        let plain: Bot = toml::from_str("name = \"x\"\n").unwrap();
        assert_eq!(plain.approvals, None);
    }
```

In `route.rs` tests:

```rust
    #[test]
    fn approval_flags_per_harness() {
        use crate::home::Approvals::{Ask, Auto};
        assert_eq!(approval_args("claude", Auto), ["--permission-mode", "auto"]);
        assert_eq!(approval_args("codex", Auto), ["--approve-for-me"]);
        assert!(approval_args("pi", Auto).is_empty());
        assert!(approval_args("cursor", Auto).is_empty());
        for harness in ["claude", "codex", "pi", "cursor"] {
            assert!(approval_args(harness, Ask).is_empty());
        }
    }
```

- [ ] **Step 2: Run them to verify they fail** (**unit**)

Run: `cargo test -p herdbot approvals_default approval_flags`
Expected: compile errors (`Approvals`, `parse_approvals_default`, `approval_args` not found).

- [ ] **Step 3: Implement.** In `home.rs`, add to `Bot` (after `schedules`, or after any fields earlier plans added):

```rust
    /// "ask" or "auto"; overrides `[approvals] mode` in herdbot.toml for this bot's agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals: Option<String>,
```

After `paths_rewrites`:

```rust
/// How a bot's agents handle tool approvals. Auto uses each harness's own automatic review,
/// never a bypass (route::approval_args).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Approvals {
    #[default]
    Ask,
    Auto,
}

pub fn parse_approvals(value: &str) -> Approvals {
    match value {
        "auto" => Approvals::Auto,
        "ask" => Approvals::Ask,
        other => {
            eprintln!("approvals: unknown mode {other:?}; using ask");
            Approvals::Ask
        }
    }
}

pub fn parse_approvals_default(herdbot_toml: &str) -> Approvals {
    #[derive(Deserialize, Default)]
    struct Section {
        mode: Option<String>,
    }
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        approvals: Section,
    }
    toml::from_str::<Config>(herdbot_toml)
        .unwrap_or_default()
        .approvals
        .mode
        .as_deref()
        .map_or(Approvals::Ask, parse_approvals)
}

/// The bot's own setting, else herdbot.toml's `[approvals] mode`, else ask.
pub fn approvals(bot: &Bot) -> Approvals {
    match bot.approvals.as_deref() {
        Some(mode) => parse_approvals(mode),
        None => parse_approvals_default(&fs::read_to_string(root().join("herdbot.toml")).unwrap_or_default()),
    }
}
```

In `route.rs`, above `pub struct Launch`:

```rust
/// Launch flags for `mode`. Auto is each harness's own automatic review, never a bypass:
/// claude's auto permission mode and codex's --approve-for-me. pi asks for nothing, and
/// cursor only offers --force, which is a bypass.
pub fn approval_args(harness: &str, mode: home::Approvals) -> Vec<String> {
    match (harness, mode) {
        ("claude", home::Approvals::Auto) => vec!["--permission-mode".into(), "auto".into()],
        ("codex", home::Approvals::Auto) => vec!["--approve-for-me".into()],
        _ => Vec::new(),
    }
}
```

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: 2 new tests, all pass. `approval_args` and `approvals` are unused until Task 2. If clippy reports dead code, finish Task 2 before running clippy, and commit Tasks 1 and 2 together.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/2026-10-20-approval-mode.md crates/herdbot/src/home.rs crates/herdbot/src/route.rs
git commit -m "feat(herdbot): approvals setting and per-harness auto-approval flags"
```

---

### Task 2: Apply the mode at every launch

**Files:**
- Modify: `crates/herdbot/src/route.rs` (`launch`: an `approvals: home::Approvals` parameter, appended to the claude and codex args)
- Modify: `crates/herdbot/src/run.rs` (`start`: pass `home::approvals(bot)`)
- Modify: `README.md` (Harness launch section; Open questions)

- [ ] **Step 1: Implement.**
  - Add `approvals: home::Approvals` as the last parameter of `route::launch`.
  - In the `"claude"` arm, add `args.extend(approval_args("claude", approvals));` before `--append-system-prompt-file`.
  - In the `"codex"` arm, add `args.extend(approval_args("codex", approvals));` after the effort block.
  - Leave pi and cursor unchanged.
  - In `run.rs` `start`, pass `home::approvals(bot)` as the new last argument of `route::launch(…)`. `start` serves main agents and subagents, so both get the bot's mode.

- [ ] **Step 2: Build and test** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 3: Live check** (**live**). Install and restart the daemon yourself:

```bash
cargo install --path crates/herdbot && launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon
printf 'approvals = "auto"\n' >> ~/Bots/herdbot-dev/bot.toml
```

From herdbot-dev, start two subagents, each with the task `Run the shell command: ls /Users/shane/Code/herdbot/crates, then call report_result with its output.`:
- one with `model: "codex:gpt-6.1-sol:medium"`
- one with `model: "claude:claude-haiku-5-5"`

Expected:
- both runs finish `done` with the listing, and neither pane showed an approval prompt
- `ps -o args= -p $(pgrep -n codex)` while the codex run is up shows `--approve-for-me`; claude's pane shows its auto-mode indicator

If codex still prompts for `ls`, record exactly what it asked. `--approve-for-me` routes to automatic review, and Shane decides whether that's enough.

- [ ] **Step 4: Docs.** In `README.md`:
  - Harness launch: add a paragraph. `[approvals] mode = "auto"` in `herdbot.toml`, or `approvals = "auto"` in a `bot.toml`, launches that bot's main agent and subagents with claude's `--permission-mode auto` and codex's `--approve-for-me`. pi doesn't ask for approvals. cursor has no automatic-review mode, so it still asks. `"ask"` is the default.
  - Open questions: update the "Should any bot run unattended" bullet. Automatic review is available through `approvals = "auto"`; full bypass (`--yolo`, `--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox`) is still never used.

- [ ] **Step 5: Commit**

```bash
git add crates/herdbot/src/route.rs crates/herdbot/src/run.rs README.md
git commit -m "feat(herdbot): launch agents with the bot's approval mode"
```

The `~/Bots/herdbot-dev/bot.toml` change is Shane's config, not the repo's, so it isn't committed.
