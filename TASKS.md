# Open tasks

## Nested hook settlement (2026-11-09)

- [ ] Prevent Codex nested-agent notify from settling the root run: implemented and locally verified in `fix/nested-hook-settle`; rollout metadata rejects nested threads before event emission, and late hook-settled reports replace the result with a distinct parent notice. Both regressions failed before and passed after; workspace build, 206 tests, clippy, and isolated CLI checks with the actual incident root/child metadata passed. Installed-daemon validation remains open; daemon untouched.
- [ ] `daemon::tests::liveness_probes_never_fill_the_lock_backlog` flakes under parallel machine load ("probe N refused"); pre-existing, not reproduced in this run.
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
