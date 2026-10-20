# Implementation plans

Written 2026-10-20 for the roadmap items in the top-level README.

| Plan | State | Touches |
|---|---|---|
| [pi through built-in MCP](2026-10-20-pi-builtin-mcp.md) | ready; approach verified live | `route.rs` (pi arm), `herdbot-pi.ts`, `run.rs`, README |
| [fm on-device fallback](2026-10-20-fm-on-device-fallback.md) | ready; fm verified live | new `fm.rs`, `daemon.rs`, `cooldown.rs`, `tools.rs` (optional task), README |
| [computer use through cua](2026-10-20-cua-computer-use.md) | ready; live checks need cua-driver installed | `home.rs`, `route.rs` (`launch` signature, claude arm), `run.rs`, README |
| [Rooms](2026-10-20-rooms.md) ([spec](../specs/2026-10-20-rooms-design.md)) | ready | new `rooms.rs`, `daemon.rs`, `tools.rs`, `home.rs`, `main.rs`, README |
| [Turn hooks](2026-10-20-turn-hooks.md) ([spec](../specs/2026-10-20-turn-hooks-design.md)) | ready; needs the pi plan merged first | new `hook.rs`, `daemon.rs`, `route.rs`, `home.rs`, `run.rs`, `tools.rs`, `herdbot-pi.ts`, README |
| OptChat memory per bot | needs a spec; builds on turn hooks' `turns.jsonl` | — |
| Pi Durable backend | needs a spec; experimental, after OptChat | — |
| Desktop UI | deferred | — |
| Grok Bot schedule/skill import | blocked: no data on disk | — |

Run them one after another, in the order **pi → cua → fm → rooms → turn hooks**, each starting from the previous one's merge. All five edit `README.md`, and turn hooks also edits `route::launch` and the daemon loop. pi and cua both edit `route::launch`. pi, fm and rooms touch `main.rs` and `tools.rs`. fm and rooms both add to the daemon loop. Running them in parallel only trades the waiting for merge conflicts.

Each plan's "Expected: N passed" counts assume a baseline of 13 tests on `main` with only that plan applied. When plans stack, add each plan's new tests instead (pi +1, cua +4, fm +5, or +6 with its optional Task 4, rooms +12).
