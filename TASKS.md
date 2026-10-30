# Open tasks

## EasyUI-inspired UI review

EasyUI's current [introduction](https://easyui.site/docs/introduction) names source ownership, tactile spring physics, and accessible/type-safe controls, including keyboard accessibility and reduced-motion support. For Herdbot's native SwiftUI/AppKit surfaces, keep implementation locally source-owned and use native controls; add motion only when it clarifies an interaction, and honor Reduce Motion whenever custom motion is used.

For every UI plan, record the adapted principle or why none applies, review default/loading/success/error/disabled states, and track keyboard operation, VoiceOver labels, visible focus, reduced motion, and visual hierarchy. Automated checks and manual checks must be reported separately.

| UI work | Automated state | Manual design review |
|---|---|---|
| Chat provenance and collaboration | Task 2 tests/build passing in `docs/superpowers/plans/2026-10-22-chat-provenance.md` | Deferred by Shane: visual hierarchy, keyboard/focus, VoiceOver, and reduced-motion review |
| Delegated lifecycle chat entries | Task 3 tests/build passing in `docs/superpowers/plans/2026-10-22-delegated-task-lifecycle.md` | Deferred by Shane: visual, notification, routing, and VoiceOver review |
| Menu discovery and setup visibility | Implementation tests recorded in `docs/superpowers/plans/2026-10-22-menu-discovery.md` | Deferred: live visual, keyboard, and VoiceOver review |
| Herdr session picker | Implementation tests recorded in `docs/superpowers/plans/2026-10-22-settings-session.md` | Deferred: live visual, keyboard, VoiceOver, and focus review |

## Open tasks

- [ ] Profile the 26.6-second Swift test run and investigate safe parallelization to reduce repeat-run time without making results flaky.

- [ ] Add `@` mentions to the message composer so the user can flag a bot or room for collaboration; mention targets determine collaborators independently of the currently selected bot or room.

- [ ] Render bot-to-bot messages and delegated subagent request/result notices as collapsed, clickable system/status entries (for example, “Messaged [bot]” or “Subagent finished”), with message details or completion reports shown on click instead of presenting them as user messages.

- [ ] Diagnose why the Herdbot MCP `task` tool started Codex Luna sessions when Pi / DeepSeek was requested, then fix and verify harness dispatch.

- [ ] Rename `ui/menubar/` to `ui/macos/` and update its package, build, release, and documentation references; keep the app's user-facing name unchanged.
- [ ] For delegated requests, return a concise immediate acknowledgment, occasional summarized progress updates, and a summarized completion response in the chat window.
