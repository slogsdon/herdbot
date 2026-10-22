# Claude trust dialog and setup button style Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A bot whose directory is under the herdbot home starts on claude without hanging on claude's "do you trust this folder" dialog, and the setup window's primary button matches the other buttons.

**Architecture:** `herdr_client::Client::agent_start` splits into start plus a public `agent_wait_ready`. `run::start` catches an `agent_blocked` startup error, reads the pane, and, when it is claude's trust dialog for a directory under `HERDBOT_HOME`, sends `Down` `Enter` (the dialog's default is "No, exit") and waits for the agent to become ready. Any other blocked startup behaves as today. The button change is Swift-only.

**Tech Stack:** Rust 2024 (`herdr-client`, `herdbot`); Swift 6 / SwiftUI (`ui/menubar`).

## Evidence (reproduced 2026-10-22)

- Throwaway bot with `--cwd /Users/Shared/trustfresh-NNNN` (a directory with no trusted parent) on `--class plan` (routes to claude): the pane shows

  ```
  Accessing workspace:
  /Users/Shared/trustfresh-92905
  Quick safety check: Is this a project you created or one you trust? …
  ❯ No, exit
    Yes, I trust this folder
  Enter to confirm · Esc to cancel
  ```

  `herdr agent get` says `agent_status: blocked, launch_pending: true`. The daemon log repeats `trustcheck3: herdr agent_blocked: trustcheck3 blocked during startup`, and `herdbot status` shows `queued: 1` indefinitely. That is the loop: `up` fails, closes the workspace, and the next tick tries again.
- `herdr agent send-keys trustcheck3 Down Enter` accepted it: the agent went `idle, interactive_ready: true`, and claude recorded `hasTrustDialogAccepted: true` for the directory in `~/.claude.json`.
- Not reproducible on the author's machine for `~/Bots/<bot>`: `/Users/shane` is already trusted, and claude trusts a directory whose parent is trusted. On a fresh machine nothing is trusted, so the first bot always hits it (reported by Shane for the `helper` bot).
- The dialog's default is "No, exit", so a bare `Enter` would exit claude; the keys must be `Down`, `Enter`.

## Global Constraints

- Auto-accept ONLY when the bot's directory is `HERDBOT_HOME` or inside it (herdbot created it; the user chose to keep bots there). Any other directory keeps today's behaviour (the error is returned), because trusting an arbitrary directory is the user's decision.
- Auto-accept ONLY for the claude harness and only when the pane text contains both `Is this a project you created or one you trust` and `Yes, I trust this folder`.
- Never write `~/.claude.json` ourselves.
- No new dependencies. Existing tests keep passing.
- Never run `herdbot service install|uninstall`, `herdbot setup` without `--only`, or `launchctl`. The live check uses a throwaway bot (below) that the agent removes afterwards. Never kill a process by name.
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.

**Branch/worktree:** `git -C ~/Code/herdbot worktree add .worktrees/claude-trust -b fix/claude-trust main`.

## File map

- Modify `crates/herdr-client/src/lib.rs`: `agent_wait_ready`.
- Modify `crates/herdbot/src/run.rs`: trust handling in `start`; pure helpers + tests.
- Modify `ui/menubar/Sources/HerdbotMenu/SetupView.swift`.
- Modify `README.md`.

---

### Task 1: `agent_wait_ready`

**Files:** Modify `crates/herdr-client/src/lib.rs`.

**Interfaces:**
- Produces: `Client::agent_wait_ready(&self, name: &str, timeout_ms: u64) -> Result<Value>` (the second loop of `agent_start`: polls `agent.get` until `interactive_ready` and not `launch_pending`; errors `agent_blocked` / `timeout` as today). `agent_start` calls it, so behaviour is unchanged.

- [ ] **Step 1:** Move the second `loop { … }` of `agent_start` into `pub async fn agent_wait_ready`, with its own deadline (`now + timeout_ms`). `agent_start` keeps its first loop (the `agent.start` call with the `agent_pane_busy` retry) and then `self.agent_wait_ready(name, remaining_ms).await`, where `remaining_ms` is the time left before the original deadline (never below 1).
- [ ] **Step 2:** There is no fake herdr server in this crate's tests; this is a pure refactor. Verify with `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`.
- [ ] **Step 3: Commit.**

```bash
git add crates/herdr-client/src/lib.rs
git commit -m "refactor: split agent_wait_ready out of agent_start"
```

---

### Task 2: Accept claude's trust dialog for bot directories

**Files:** Modify `crates/herdbot/src/run.rs`.

**Interfaces:**
- Consumes: `Client::agent_wait_ready`, `Client::agent_send_keys(target, &["Down","Enter"])`, `run::tail(herdr, agent, lines)`.
- Produces: `fn is_claude_trust_dialog(tail: &str) -> bool`, `fn under_home(cwd: &str, root: &Path) -> bool`.

- [ ] **Step 1: Failing tests** (add a `#[cfg(test)] mod tests` to `run.rs` or extend the existing one):

```rust
    #[test]
    fn recognises_claudes_trust_dialog_only() {
        let dialog = "Accessing workspace:\n /Users/x/Bots/helper\n Quick safety check: Is this a project you created or one you trust? (Like your own code)\n ❯ No, exit\n   Yes, I trust this folder\n Enter to confirm · Esc to cancel";
        assert!(is_claude_trust_dialog(dialog));
        assert!(!is_claude_trust_dialog("Do you trust the files in this folder?\n1. Yes 2. No")); // codex's wording, other harness
        assert!(!is_claude_trust_dialog("❯ Try \"write a test\""));
        assert!(!is_claude_trust_dialog(""));
    }

    #[test]
    fn only_directories_inside_the_herdbot_home_are_trusted_automatically() {
        let root = Path::new("/Users/x/Bots");
        assert!(under_home("/Users/x/Bots", root));
        assert!(under_home("/Users/x/Bots/helper", root));
        assert!(under_home("/Users/x/Bots/helper/sub", root));
        assert!(!under_home("/Users/x/Bots-other/helper", root));
        assert!(!under_home("/Users/x/Code/app", root));
        assert!(!under_home("/Users/x/Bots/../Code", root));
        assert!(!under_home("", root));
    }
```

Run `cargo test -p herdbot run::tests` → FAIL.

- [ ] **Step 2: Implement.**

```rust
/// claude's first-run dialog for a directory it has not been told to trust. Its default
/// highlighted choice is "No, exit", so accepting needs Down then Enter.
fn is_claude_trust_dialog(tail: &str) -> bool {
    tail.contains("Is this a project you created or one you trust") && tail.contains("Yes, I trust this folder")
}

/// `cwd` is `root` or inside it, by path components (so `Bots-other` is not inside `Bots`) and
/// without `..` tricks.
fn under_home(cwd: &str, root: &Path) -> bool {
    let path = Path::new(cwd);
    !cwd.is_empty()
        && !path.components().any(|c| matches!(c, std::path::Component::ParentDir))
        && path.starts_with(root)
}
```

Integration in `start`: replace

```rust
    herdr
        .agent_start(agent, launch.kind, pane, &launch.args, 60_000)
        .await?;
```

with

```rust
    let started = herdr.agent_start(agent, launch.kind, pane, &launch.args, 60_000).await;
    if let Err(herdr_client::Error::Api { code, .. }) = &started {
        if code == "agent_blocked" && launch.kind == "claude" {
            let screen = tail(herdr, agent, 60).await;
            let cwd = home::cwd(slug, bot);
            if is_claude_trust_dialog(&screen) && under_home(&cwd, &home::root()) {
                eprintln!("{agent}: trusting {cwd} in claude's folder dialog (a herdbot directory)");
                herdr.agent_send_keys(agent, &["Down", "Enter"]).await?;
                herdr.agent_wait_ready(agent, 60_000).await?;
                started_ok = true;
            }
        }
    }
```

structured so a trusted-and-ready start falls through to the rest of `start`, any other `Err` is returned unchanged (`started?` after the block when `started_ok` is false). Write it however compiles cleanly; the contract is the three rules above and "other blocked startups are unchanged". Check the actual error type path (`herdr_client::Error::Api { code, message }`) and `tail`'s signature in the file before writing.

If `home::cwd` already returns an expanded absolute path, use it directly; if it can return a relative or `~` path, expand it with `home::expand_tilde` first.

- [ ] **Step 3: Verify.** `cargo build && cargo test --workspace && cargo clippy --workspace -- -D warnings`.

- [ ] **Step 4: Live check** with a throwaway bot, then remove it. The daemon on this machine runs the INSTALLED binary, not your build, so run the check against a separate daemon-free path: build the branch binary, then, in a temp `HERDBOT_HOME` (`T=$(mktemp -d)`), do NOT use the real daemon. Instead run `HERDBOT_HOME=$T target/debug/herdbot up trustlive` after creating the bot there with `HERDBOT_HOME=$T target/debug/herdbot new trustlive --class plan` (default cwd = `$T/trustlive`, i.e. under the home). `$T` is under `/var/folders/...`, which has no trusted parent in `~/.claude.json`; check first with `python3 -c "import json,os;p=json.load(open(os.path.expanduser('~/.claude.json')))['projects'];print([k for k in p if '/var/folders' in k][:3])"` and say what you found. `up` talks to herdr directly (no daemon), so it exercises the new code. Expect: the log line `trusting … (a herdbot directory)`, `up` returning Ok, and `herdr agent get trustlive` showing `idle`. Then `HERDBOT_HOME=$T target/debug/herdbot down trustlive` and `rm -rf $T`. If the temp directory turns out to be already trusted (no dialog), say so and stop; do not weaken the check. Also confirm the negative case once by hand with the unit tests only (a directory outside the home is not auto-accepted); do not create an untrusted directory outside the home on the machine.

- [ ] **Step 5: Commit.**

```bash
git add crates/herdbot/src/run.rs
git commit -m "fix: accept claude's trust dialog for bot directories under the herdbot home"
```

---

### Task 3: Button style

**Files:** Modify `ui/menubar/Sources/HerdbotMenu/SetupView.swift`.

- [ ] **Step 1:** Remove `.buttonStyle(.borderedProminent)` from "Set up everything" so it uses the same default bordered style as "Fix" and "Done" (Shane reported it still looks different from the others; the prominent style uses the accent fill and its own label colour). Keep the spinner, `.disabled(...)` and `.help(...)`.
- [ ] **Step 2: Verify.** `cd ui/menubar && timeout 200 ./scripts/test.sh` and `./scripts/build-app.sh` (no new warnings). Do not launch the app.
- [ ] **Step 3: Commit.**

```bash
git add ui/menubar/Sources/HerdbotMenu/SetupView.swift
git commit -m "fix(menubar): use the standard button style for Set up everything"
```

---

### Task 4: Docs

**Files:** Modify `README.md`.

- [ ] **Step 1:** In the Harness launch section (near the paragraph about trust modals and the cursor `--trust` row), add one paragraph: claude shows a "Is this a project you created or one you trust?" dialog the first time it runs in a directory that is not trusted (a trusted parent counts), and its default choice is "No, exit"; herdbot answers it (Down, Enter) only when the bot's directory is under `HERDBOT_HOME`; for a bot with its own `cwd` elsewhere, trust the directory once yourself (run `claude` there, or `herdbot focus <agent>` and answer). State that codex and pi are not covered by this.
- [ ] **Step 2: Commit.**

```bash
git add README.md
git commit -m "docs: claude's folder trust dialog"
```

---

## Self-review (done)

- Covers: the reported loop for the default `helper` bot (T2), the safety boundary (T2 `under_home`), the key sequence found by experiment (T2), the button (T3).
- Names: `agent_wait_ready`, `is_claude_trust_dialog`, `under_home`.
- Known gap, deliberate: a bot with an explicit `cwd` outside the herdbot home still loops (start fails, workspace closed, retry next tick). A follow-up should stop that retry spin and surface the blocked agent to the user instead; it needs a design decision about where the blocked agent lives while it waits.
- Unverified: codex's equivalent dialog (README says it eats the first prompt) and pi (none seen).
