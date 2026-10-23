# Implementation plans

## Research-derived plans

The supplied *Build Your Own Duet* PDF is an independent engineering synthesis, not a description of Duet's internals. The Herdbot translation is in [the research mapping](../../research/build-your-own-duet-herdbot-translation.md). These plans adapt delegation, provenance, retry, and replay ideas; external event watching, severity scoring, autonomous policy learning, and a new credential broker are out of scope.

| Plan | State | Depends on |
|---|---|---|
| [Orchestration reliability](2026-10-22-orchestration-reliability.md) ([spec](../specs/2026-10-22-orchestration-reliability-design.md)) | Draft; unavailable explicit routes may fall back with visible route and reason | — |
| [Delegated task lifecycle](2026-10-22-delegated-task-lifecycle.md) ([spec](../specs/2026-10-22-delegated-task-lifecycle-design.md)) | Draft; progress is chat-only | reliability plan |
| [Chat provenance and collaboration](2026-10-22-chat-provenance.md) ([spec](../specs/2026-10-22-chat-provenance-design.md)) | Draft; agent messages are retained in memory; @ target is sole recipient | reliability and lifecycle plans |

These plans capture the resolved product decisions and remain sequenced after the active usability/settings roadmap.

Written 2026-10-20 for the roadmap items in the top-level README.

| Plan | State | Touches |
|---|---|---|
| [pi through built-in MCP](2026-10-20-pi-builtin-mcp.md) | done (committed on feature/roadmap), 2026-10-20 | `route.rs` (pi arm), `herdbot-pi.ts`, `run.rs`, README |
| [fm on-device fallback](2026-10-20-fm-on-device-fallback.md) | done (committed on feature/roadmap), 2026-10-20 | new `fm.rs`, `daemon.rs`, `cooldown.rs`, `tools.rs` (optional task), README |
| [computer use through cua](2026-10-20-cua-computer-use.md) | done (committed on feature/roadmap), 2026-10-20 | `home.rs`, `route.rs` (`launch` signature, claude arm), `run.rs`, README |
| [Rooms](2026-10-20-rooms.md) ([spec](../specs/2026-10-20-rooms-design.md)) | done (committed on feature/roadmap), 2026-10-20 | new `rooms.rs`, `daemon.rs`, `tools.rs`, `home.rs`, `main.rs`, README |
| [Turn hooks](2026-10-20-turn-hooks.md) ([spec](../specs/2026-10-20-turn-hooks-design.md)) | done (committed on feature/roadmap), 2026-10-20 | new `hook.rs`, `daemon.rs`, `route.rs`, `home.rs`, `run.rs`, `tools.rs`, `herdbot-pi.ts`, README |
| [OptChat memory per bot](2026-10-20-optchat-memory.md) ([spec](../specs/2026-10-20-optchat-memory-design.md)) | done (committed on feature/roadmap), 2026-10-20 | new `memory.rs`, `daemon.rs`, `tools.rs`, `mcp.rs`, `run.rs`, `hook.rs`, `home.rs`, `route.rs`, `routing.toml`, two assets, README |
| Pi Durable backend | needs a spec; experimental, after OptChat | — |
| [Desktop UI, phase 0: core + CLI](2026-10-21-ui-core-and-cli.md) ([spec](../specs/2026-10-21-ui-layer-design.md)) | done (committed on feature/ui-core), 2026-10-21 | new `herdbot-core` crate (`home.rs`, `cooldown.rs` move), `hook.rs`, `main.rs`, `herdr-client`, README |
| [Desktop UI, phase 1: menu bar app](2026-10-21-menubar-app.md) | done (committed on feature/ui-menubar), 2026-10-21 | new `ui/menubar` Swift package, README |
| [Desktop UI, phase 2: quick entry](2026-10-22-quick-entry.md) | done (committed on feature/ui-quickentry), 2026-10-22 | `herdbot-core` (`rooms.rs` moves in), `herdbot` `rooms.rs`, `ui/menubar` quick-entry files, README |
| [Desktop UI, phase 3.1: chat data seam](2026-10-22-chat-data-seam.md) ([spec](../specs/2026-10-22-chat-app-design.md)) | done (committed on feature/ui-chat), 2026-10-22 | `herdbot-core` (`history.rs`, `rooms.rs`, `home.rs`, `snapshot.rs`), `main.rs`, `daemon.rs`, README |
| [Desktop UI, phase 3.2: chat window](2026-10-22-chat-window.md) | done (committed on feature/ui-chat), 2026-10-22 | `ui/menubar` sidebar, chat view, history models; macOS floor 15 |
| [Desktop UI, phase 3.3: composer, unread, notifications](2026-10-22-chat-composer.md) | done (committed on feature/ui-chat), 2026-10-22 | `ui/menubar` composer, unread tracker, notifier |
| [Approval mode](2026-10-20-approval-mode.md) | done (6642445), 2026-10-20 | `route.rs` (`approval_args`), `home.rs`, README |
| [Effort default](2026-10-20-effort-default.md) | done (becfa43), 2026-10-20 | `route.rs` (`resolve`), `routing.toml` |
| [Desktop UI, phase 2b: quick-entry redesign](2026-10-22-quick-entry-redesign.md) | done (on main), 2026-10-22 | `ui/menubar` quick-entry panel |
| [Packaging, 1: CLI](2026-10-22-packaging-cli.md) ([spec](../specs/2026-10-22-packaging-design.md)) | done (on main), 2026-10-22 | `service.rs`, `daemon.rs`, new `doctor.rs` and `setup.rs`, `home.rs` (`DaemonInfo`), README |
| [Packaging, 2: app](2026-10-22-packaging-app.md) | done (on main), 2026-10-22 | `ui/menubar` setup window, login item, bundled-helper lookup |
| [Packaging, 3: release](2026-10-22-packaging-release.md) | done (on main), 2026-10-22; released 0.1.0 to 0.1.2 | `scripts/release.sh`, `packaging/herdbot.rb.tmpl`, `build-app.sh` |
| [GUI PATH and daemon start](2026-10-22-gui-path.md) | done (on main), 2026-10-22 | new `shellenv.rs`, `doctor.rs`, `service.rs`, `setup.rs` |
| [Setup window feedback](2026-10-22-setup-feedback.md) | done (on main), 2026-10-22 | `ui/menubar` `SetupState`, `SetupView` |
| [Approval cards, 1: CLI and daemon](2026-10-22-approval-cards-cli.md) ([spec](../specs/2026-10-22-approval-cards-design.md)) | done (on main), 2026-10-22 | new `herdbot-core` `cards.rs`, new `adapters.rs`, `run.rs`, `daemon.rs`, `main.rs`, `snapshot.rs` |
| [Approval cards, 2: app](2026-10-22-approval-cards-app.md) | done (on main), 2026-10-22 | `ui/menubar` card types, `CardView`, notifications, menu |
| Menu discovery and setup visibility ([spec](../specs/2026-10-22-menu-discovery-design.md)) | implementation complete; manual UI checks pending | U1 + U5, clickable top five bots and conditional Setup item |
| Quick-entry focus lifecycle ([spec](../specs/2026-10-22-quick-entry-focus-design.md)) | implementation complete; macOS focus and dictation checks pending | U3 + U4, panel focus and dismissal |
| Chat block markdown ([spec](../specs/2026-10-22-chat-markdown-design.md)) | implementation complete; manual chat inspection pending | headings and ordered/nested lists |
| Settings CLI ([spec](../specs/2026-10-22-settings-design.md)) | implementation and Rust validation complete | typed registry, read/list, and format-preserving set/unset |
| Settings app ([spec](../specs/2026-10-22-settings-design.md)) | Tasks 1–2 implemented; manual UI check pending | typed models/CLI seam, global and per-bot forms, sidebar and status-menu entry points; session control follows its spike |
| Settings lifecycle ([spec](../specs/2026-10-22-settings-design.md)) | planned; depends on Settings app | setup rerun, daemon disable/enable, uninstall |
| Herdr named-session spike ([spec](../specs/2026-10-22-settings-design.md)) | planned before session setting | session creation and socket-resolution research |
| Herdr session setting ([spec](../specs/2026-10-22-settings-design.md)) | planned; blocked on spike | `2026-10-22-settings-session.md`; defaults to `default`, no automatic workspace migration |
| Terminal focus spike ([spec](../specs/2026-10-22-terminal-focus-design.md)) | planned; implementation plan follows spike | Terminal.app, iTerm2, Ghostty attach behavior and permissions |
| Grok Bot schedule/skill import | blocked: no data on disk | — |

Approval-card adapters for Claude per-tool prompts and the Codex trust modal remain unplanned until Shane supplies real screen captures, as required by the approval-card design. U2 (create bots/rooms and import templates) remains open and outside this usability batch.

Run them one after another, in the order **pi → fm → rooms → turn hooks → OptChat memory → cua**, each starting from the previous one's merge. cua goes last because its live checks wait on Shane installing cua-driver and granting it permissions. All six edit `README.md`, and turn hooks also edits `route::launch` and the daemon loop. pi and cua both edit `route::launch`. pi, fm and rooms touch `main.rs` and `tools.rs`. fm and rooms both add to the daemon loop. Running them in parallel only trades the waiting for merge conflicts.

Current baseline with all plans applied (0.1.2, 2026-10-22): 116 Rust tests (`cargo test --workspace`) and 92 Swift tests (`ui/menubar/scripts/test.sh`).

Still unverified live: fm modal hold (not exercised; no modal appeared), CUA `ask` approval flow, remote CUA host.
