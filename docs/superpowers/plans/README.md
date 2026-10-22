# Implementation plans

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
| Grok Bot schedule/skill import | blocked: no data on disk | — |

Run them one after another, in the order **pi → fm → rooms → turn hooks → OptChat memory → cua**, each starting from the previous one's merge. cua goes last because its live checks wait on Shane installing cua-driver and granting it permissions. All six edit `README.md`, and turn hooks also edits `route::launch` and the daemon loop. pi and cua both edit `route::launch`. pi, fm and rooms touch `main.rs` and `tools.rs`. fm and rooms both add to the daemon loop. Running them in parallel only trades the waiting for merge conflicts.

Current baseline with all plans applied: 63 tests.

Still unverified live: fm modal hold (not exercised; no modal appeared), CUA `ask` approval flow, remote CUA host.
