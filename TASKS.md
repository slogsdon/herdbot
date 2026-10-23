# Open tasks

- [ ] Profile the 26.6-second Swift test run and investigate safe parallelization to reduce repeat-run time without making results flaky.

- [ ] Add `@` mentions to the message composer so the user can flag a bot or room for collaboration; mention targets determine collaborators independently of the currently selected bot or room.

- [ ] Render bot-to-bot messages and delegated subagent request/result notices as collapsed, clickable system/status entries (for example, “Messaged [bot]” or “Subagent finished”), with message details or completion reports shown on click instead of presenting them as user messages.

- [ ] Diagnose why the Herdbot MCP `task` tool started Codex Luna sessions when Pi / DeepSeek was requested, then fix and verify harness dispatch.

- [ ] Rename `ui/menubar/` to `ui/macos/` and update its package, build, release, and documentation references; keep the app's user-facing name unchanged.
- [ ] For delegated requests, return a concise immediate acknowledgment, occasional summarized progress updates, and a summarized completion response in the chat window.
