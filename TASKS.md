# Open tasks

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

- [ ] Diagnose why the Herdbot MCP `task` tool started Codex Luna sessions when Pi / DeepSeek was requested, then fix and verify harness dispatch. The committed source includes routing/fallback tracking, but the installed helper is still 0.1.2; a Pi request from this session started Codex without a surfaced fallback reason.
- [x] Move the desktop Swift package to `ui/macos/` and update tracked package, build, release, and documentation references. The executable target remains `HerdbotMenu`; the user-facing app name remains `Herdbot`. Verified with 156 Swift tests, app build/signature checks, `scripts/test-release.sh` (universal dry run), `cargo build`, 144 Herdbot tests, clippy, and `git diff --check`.

## Settings session salvage (2026-11-09)

- [x] Salvage and recheck named-session spike read-only.
- [x] Salvage CLI/UI in order and reconcile lifecycle/collaboration changes.
- [x] Add workspace ownership and command-routing regression checks; fresh review fixes.
- [x] Complete final Rust build/tests/clippy, 197 Swift tests, arm64 app/signature checks and report uncommitted diff. One unchanged daemon backlog test failed once, then full rerun passed.
- [ ] Shane: manual UI, disposable-session lifecycle and installed daemon socket verification.
