# Open tasks

## Errored hook turns (2026-11-09)

- [x] Prevent Pi error/aborted or empty final hook turns from settling successfully: forward final assistant metadata, emit `turn_error`, settle as `failed` with `result_source: hook`, and notify the parent that the task can be resumed in a new run. Wire Claude `StopFailure`; Codex 0.167.0 notify has no error field. Pi extension, error/empty-turn parsing/decision, Claude wiring and failure-result/notice regressions pass; the original Pi/empty/Claude/wiring regressions failed before the fix. Workspace build/tests/clippy, Node extension test, isolated hook CLI emission and `git diff --check` passed. No commit, installed-app/daemon activation, or live pane changes.

## Restored main-agent wiring (2026-11-09)

- [x] Persist herdr terminal identity for main-agent launches; recover mismatched/legacy idle or done agents through the normal wired launch, preserve reported Codex/Claude/Pi sessions on matching harnesses, and defer working/blocked/unknown agents (including a second status check before closing). Check at `up`, before queue delivery, and on daemon polling including startup. Keep READY launch-turn exclusion. README updated; changes remain uncommitted.
  - Root cause confirmed read-only: PID 4311 began Nov 3 16:03 with bare `codex resume 01a16695-a5ef-70c3-af34-a2200d0e3cf5`, no HERDBOT environment; saved herdr state has the matching session.
  - Detection and resume-argument regressions failed first (Keep instead of Recover; no resume arguments), then passed. Legacy deserialization and existing READY-marker tests pass. Installed CLI help confirms Codex `resume`, Claude `--resume`, Pi `--session`; unsupported/missing/cross-harness sessions start fresh.
  - Live terminal identity proof: disposable headless session `restore-id-gw8d51` retained pane `w1:p1` across an actual stop/start restore, changing terminal `term_65fe35f5570e51` to `term_65fe35f9248ae1`; repeated pre-restore reads were stable. Session stopped/deleted afterward.
  - Isolated HERDBOT_HOME, bot `restore-gw8d51`, Pi `deepseek-v4.1-flash:low`: replaced its wired terminal with a bare Pi launch, then `up` detected the mismatch and restored wiring. Legacy missing-ID recovery also ran on the first isolated daemon tick with one recovery log line; a subsequent matching-ID `up` kept the pane. Hook events and `turns.jsonl` contained `RESTORE-HOOK-PROOF`, correctly paired queued `FINAL-WIRING-OK`, and final-binary `LATEST-HOOK-OK`. Evidence: `/tmp/gw8d51-evidence.json`; check logs `/tmp/gw8d51-{red,green,build,tests,clippy}.log`. Test daemon, workspace w4N, temporary home, and test Pi session directories were removed; agent/workspace absence verified.
  - `cargo build --workspace`, `cargo test --workspace` (212 passed), `cargo clippy --workspace -- -D warnings`, `git diff --check` passed. Live harness-session resume and Codex/Claude recovery were not exercised: herdr did not report a Pi session during this live check. Existing panes, dr-pc-weekly-1109, installed app and production daemon were untouched.

## Nested hook settlement (2026-11-09)

- [x] Prevent Codex nested-agent notify from settling the root run: merged in `a38a93d`; rollout metadata rejects nested threads before event emission, and late hook-settled reports replace the result with a distinct parent notice. Both regressions failed before and passed after; workspace build, 206 tests, clippy, and isolated CLI checks with the actual incident root/child metadata passed. Live-validated 2026-11-09 on the installed app built from main: run `stsb2q` spawned sub-agent thread `…7f8da079` (parent `…bfca4639`), which finished first without settling the run; the run ended via `report_result` (`NESTED-OK 409`).
- [x] `daemon::tests::liveness_probes_never_fill_the_lock_backlog` flaked under parallel machine load ("probe N refused"). Test-only timing artifact, not a production risk: `status`/`doctor` issue one connect each, and the drain thread accepts and drops every connection immediately, so a probe is refused only after 128 connections queue up unaccepted — a test that fires 400 connects back to back outruns a drain thread the scheduler has descheduled. Fixed in the test: a refused connect is retried on a 1 ms tick with a 5 s per-probe deadline, and the whole 400-probe burst must finish within 20 s, so a starved drain recovers while a stalled or trickling drain still fails. Mutation checks: with no drain thread the test fails at probe 128 (3838 refusals in 5 s); with a 200 ms/accept drain it fails the burst bound. 30/30 passes under 40× `yes` load plus three concurrent `cargo test --workspace` loops; workspace build, 209 tests, and clippy clean. The original flake did not reproduce on this machine (0/50 pre-change runs under load).
- [x] `main.rs` session test wrote under `current_dir()` (`.superpowers/session-guard-test`): switched to `std::env::temp_dir()` with a process-specific directory.

## Pi premature harvest

- [x] Reproduce herdr 0.9.1 idle classification on both captured Pi tails (`default_known_agent_idle_fallback`); require hooked runs to finish via turn-end events or `report_result`, preserving unhooked tail fallback. Regression failed before the fix; workspace build, 192 tests, clippy, and release build passed. The worktree release binary wrote the expected agent-scoped Pi `turn_end` event in an isolated fixture home. Live daemon harvest verification skipped: it requires the new daemon; the installed app daemon must remain untouched.

## EasyUI-inspired UI review

EasyUI's current [introduction](https://easyui.site/docs/introduction) names source ownership, tactile spring physics, and accessible/type-safe controls, including keyboard accessibility and reduced-motion support. For Herdbot's native SwiftUI/AppKit surfaces, keep implementation locally source-owned and use native controls; add motion only when it clarifies an interaction, and honor Reduce Motion whenever custom motion is used.

For every UI plan, record the adapted principle or why none applies, review default/loading/success/error/disabled states, and track keyboard operation, VoiceOver labels, visible focus, reduced motion, and visual hierarchy. Automated checks and manual checks must be reported separately.

| UI work | Automated state | Manual design review |
|---|---|---|
| Chat provenance and collaboration | Task 2 tests/build passing in `docs/superpowers/plans/2026-10-22-chat-provenance.md` | Deferred by Shane: visual hierarchy, keyboard/focus, VoiceOver, and reduced-motion review |
| Delegated lifecycle chat entries | Task 3 tests/build passing in `docs/superpowers/plans/2026-10-22-delegated-task-lifecycle.md` | Deferred by Shane: visual, notification, routing, and VoiceOver review |
| Menu discovery and setup visibility | Implementation tests recorded in `docs/superpowers/plans/2026-10-22-menu-discovery.md` | Deferred: live visual, keyboard, and VoiceOver review |
| Herdr session picker | Implementation tests recorded in `docs/superpowers/plans/2026-10-22-settings-session.md` | Deferred: live visual, keyboard, VoiceOver, and focus review |

## Completed

- [x] Profile the Swift test run and enable safe parallel execution by default: warm serial 9.11s; warm parallel 1.53–1.66s; 155 tests passed in three parallel runs. `--no-parallel` remains available.
- [x] Add `@` mentions to the message composer so the user can flag a bot or room for collaboration; mention targets determine collaborators independently of the currently selected bot or room. Implemented and automated-verified in `feature/chat-provenance-task1`.
- [x] Render bot-to-bot messages and delegated subagent request/result notices as collapsed system/status entries, with details disclosed on demand. Automated-verified in `feature/chat-status-rows`; live UI checks remain deferred.
- [x] Show delegated task start, coalesced progress, and summarized completion updates in chat history. Automated-verified by the delegated lifecycle plan; live UI checks remain deferred.

## Open tasks

- [x] Diagnose why the Herdbot MCP `task` tool started Codex Luna sessions when Pi / DeepSeek was requested, then fix and verify harness dispatch.
  - Historical router (`63d0198^`, `route.rs::resolve`) treated an explicit route as the first ordinary candidate, silently advancing to Codex after parse/availability rejection. Current `74d8811` already prioritizes explicit requests, rejects invalid/unknown models, and records unavailable-route fallbacks. The exact availability rejection in the original deployed incident cannot be recovered from its missing metadata; version `0.1.2` alone does not identify the installed source revision.
  - Remaining source defect: `run.rs::start` returned the route only after successful startup, so failed/blocked launches lost requested-route/fallback metadata; `tools.rs` also omitted route diagnostics from failed/blocked acknowledgements. Persist the resolution before launch, reload it on either outcome, and append fallback diagnostics for every launch status. Changes are uncommitted and revertible.
  - Trace: MCP `model` bypasses class picking, flows through `SpawnRequest.explicit_model` to `route::resolve`; explicit Pi wins over bot/class candidates. Missing effort inherits class/default effort; a final `low|medium|high` suffix is effort. Missing PATH binary, cooldown, or failed model listing can choose configured fallback with a surfaced reason. Malformed/unknown-harness requests and successful listings proving model absence return errors. Provider-prefixed `pi:opencode-go/deepseek...` is rejected against the configured provider listing, rather than silently selecting Codex; use bare model IDs.
  - Regression: `task_response_surfaces_fallback_for_every_launch_status` failed on the old blocked response, then passed. Added `explicit_pi_ignores_bot_and_class_candidates_with_or_without_effort` and `provider_prefixed_pi_model_is_an_error_instead_of_a_fallback`; all 22 router tests passed.
  - E2E: worktree `cargo build --release`, then release `herdbot mcp --bot herdbot-dev` over stdio (`initialize`, `notifications/initialized`, `tools/call task`, model `pi:deepseek-v4.1-flash`, prompt `Reply OK. Do not edit files or perform other work.`). Response: `started run spi4cu on pi:deepseek-v4.1-flash:medium in pane w3H:p24. Its result will arrive as a message.` Durable `~/Bots/.runs/spi4cu.json`: same route, requested_route `pi:deepseek-v4.1-flash`, fallback_reason null, status done, result OK, result_source hook, pane_closed true. Daemon had already closed the pane; `herdr pane list` confirmed it absent. Transcript and run snapshot: `target/mcp-route-e2e.json`. Installed binary and `dr-pc-weekly-1109` were untouched.
  - Checks: release build, `cargo build --workspace`, `cargo test --workspace` (195 tests), `cargo clippy --workspace -- -D warnings`, and `git diff --check` passed. This proves the worktree release MCP route; installed-service routing was not reverified.
- [x] Move the desktop Swift package to `ui/macos/` and update tracked package, build, release, and documentation references. The executable target remains `HerdbotMenu`; the user-facing app name remains `Herdbot`. Verified with 156 Swift tests, app build/signature checks, `scripts/test-release.sh` (universal dry run), `cargo build`, 144 Herdbot tests, clippy, and `git diff --check`.

## Settings session salvage (2026-11-09)

- [x] Salvage and recheck named-session spike read-only.
- [x] Salvage CLI/UI in order and reconcile lifecycle/collaboration changes.
- [x] Add workspace ownership and command-routing regression checks; fresh review fixes.
- [x] Complete final Rust build/tests/clippy, 197 Swift tests, arm64 app/signature checks and report uncommitted diff. One unchanged daemon backlog test failed once, then full rerun passed.
- [ ] Shane: manual UI, disposable-session lifecycle and installed daemon socket verification.


## U2(a): create bots and rooms (2026-11-09)

- [x] Review inherited work; complete CLI creation options, JSON, validation and Swift forms through ProcessCLI.
- [x] Cover parser, filesystem validation, form/state transitions and ProcessCLI argument/error handling.
- [x] `cargo build --workspace`; `cargo test --workspace` (213 tests); `cargo clippy --workspace -- -D warnings`; `ui/macos/scripts/test.sh` (216 tests reported, opt-in real-binary test skipped); `OUT_DIR=build/u2a ./scripts/build-app.sh` (arm64, codesign verified); `git diff --check`. Rebuilt the same bundle with the current worktree helper embedded for review. Isolated CLI smoke inspected JSON, BOT.md/bot.toml/room.toml/transcript.md, unchanged human output, actionable failures and watch-stream roster updates; temporary home removed.
- [ ] Shane: open the locally built app and review both forms at default, invalid/disabled, loading, success and CLI-error states. Check folder picker cancellation/selection, room membership order, moderated-only picker, draft preservation/retry and watch-driven sidebar appearance/selection.
- [ ] Shane: verify Tab/Shift-Tab traversal and visible focus, Return Create (newline in instructions), Escape Cancel, keyboard operation of + menu/member toggles/pickers, VoiceOver labels and error discovery, layout with long names/errors, and Reduce Motion. Native SwiftUI/AppKit controls are used, with no custom animation; live accessibility and visual behavior remain unverified.

Design principle: use native source-owned controls, clear labels and inline validation; keep all writes behind the existing CLI seam. Create disables while invalid/unavailable/running, shows progress while running, preserves drafts on error and closes/selects the returned slug on success. Cancel during creation dismisses without undoing the CLI write. No template/URL import.
