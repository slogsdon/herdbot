# HO-20261102-0001 — Operator prompt cards

Worktree: `/Volumes/Dock/Code/herdbot/.worktrees/operator-prompt-cards`
Branch: `feature/operator-prompt-cards` (based on `feature/ui-macos-rename-tier2`)

## Implemented

- Pane startup checks for known trust/review/permission prompts before task delivery and while waiting for prompt acceptance. Codex Hooks review and Claude folder trust have concrete detection; any Herdr-blocked harness gets a display-only fallback card. A task that remains idle across the two 15-second delivery windows stays blocked with its pane open.
- Cards carry optional `harness` and `run_id` metadata. The macOS chat card displays harness, run, bot, dialog text, and observed choices; generic prompt choices are informational and cannot answer the harness.
- Blocked runs remain in v1 snapshots. Hook completion and `report_result` can settle a blocked run after operator action; the card clears when the prompt/run clears. Operator blocks are not queued to the parent model.
- Pane-tail fallback retains the full output and sets `status=failed`, `result_source=tail`; only `report_result` or a turn-end hook settles as done.
- README documents the Codex hooks review preflight and repeating it after plugin updates.

## Checks

- `cargo build` — passed.
- `cargo test -p herdbot` — 148 passed.
- `cargo test -p herdbot-core` — 34 unit tests, 2 integration tests, and doc tests passed.
- `cargo clippy -p herdbot -- -D warnings` — passed.
- `cd ui/macos && ./scripts/test.sh` — 159 passed in 17 suites; live-binary streaming test skipped because `HERDBOT_BIN` was unset.
- `cd ui/macos && ./scripts/build-app.sh` — passed; executable check and `codesign --verify --deep --strict` passed.
- `git diff --check` — passed.

Live daemon and UI interaction checks remain pending; no install or daemon restart was performed.
