# Effort Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** herdbot never launches an agent without an explicit effort. A route that names none, whether from the table or an explicit `model`, gets one from its class, the table's default, or `medium`, in that order. Without this, claude falls back to the user's global `effortLevel` (Shane's is `high`), which burns subscription limits.

**Architecture:** `route::resolve` fills `Route.effort` when it's `None`. `routing.toml` gains an optional `effort` per class and in `[default]`, and its claude candidates name efforts explicitly.

**Tech Stack:** Rust 2024, toml.

## Global Constraints

- Runs on `feature/roadmap` after plan 1b (approval mode), before plan 2.
- An effort a candidate names explicitly always wins. The fallback applies only to `None`.
- Fallback order: `[classes.<class>] effort`, then `[default] effort`, then `"medium"`.
- Efforts must be a level the harness accepts (`low`, `medium`, `high`; claude also takes `xhigh` and `max`). Don't validate them here; the harness rejects bad values.
- Tests stay in-file. Commits: conventional, explicit paths, no AI attribution, never push.

---

### Task 1: Fill a missing effort in `resolve`

**Files:**
- Modify: `crates/herdbot/src/route.rs` (`resolve`, `default_effort`, test)
- Modify: `routing.toml` (the repo default: efforts per class and on claude candidates)
- Modify: `README.md` (the Routing section)

- [x] **Step 1: Write the failing test** in `route.rs` tests:

```rust
    #[test]
    fn missing_effort_comes_from_class_then_default() {
        let t: Value = "[default]\neffort = \"low\"\ncandidates = [\"pi:x\"]\n[classes.plan]\neffort = \"high\"\ncandidates = [\"pi:x\"]\n"
            .parse::<toml::Table>()
            .unwrap()
            .into();
        assert_eq!(default_effort(&t, "plan"), "high");
        assert_eq!(default_effort(&t, "explore"), "low");
        let bare: Value = "[default]\ncandidates = [\"pi:x\"]\n".parse::<toml::Table>().unwrap().into();
        assert_eq!(default_effort(&bare, "plan"), "medium");
        // The repo's default table names an effort on every candidate or class.
        let shipped: Value = DEFAULT_ROUTING.parse::<toml::Table>().unwrap().into();
        for (class, body) in shipped["classes"].as_table().unwrap() {
            for c in body["candidates"].as_array().unwrap() {
                let route = parse(c.as_str().unwrap()).unwrap();
                assert!(route.effort.is_some() || body.get("effort").is_some(), "{class}: {c} has no effort");
            }
        }
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot missing_effort`. Expected: compile error, `default_effort` not found.

- [x] **Step 3: Implement.** In `route.rs`:

```rust
/// The effort for a route that names none: its class's `effort`, then `[default] effort`,
/// then medium. Never None: a harness left without one falls back to the user's own global
/// setting, which can be far costlier.
fn default_effort(table: &Value, class: &str) -> String {
    let key = class_key(class);
    table
        .get("classes")
        .and_then(|c| c.get(key))
        .and_then(|c| c.get("effort"))
        .or_else(|| table.get("default").and_then(|d| d.get("effort")))
        .and_then(Value::as_str)
        .unwrap_or("medium")
        .to_owned()
}
```

In `resolve`, change `Ok(()) => return Ok(route),` to:

```rust
            Ok(()) => {
                let effort = route.effort.clone().or_else(|| Some(default_effort(table, class)));
                return Ok(Route { effort, ..route });
            }
```

In the repo's `routing.toml`, copy the `candidates` list of every class and `[default]` from Shane's live `~/Bots/routing.toml`. That table was retuned on 2026-10-20 to the cheapest acceptable model first, with stronger models as fallbacks, and every candidate names an effort. Leave out cursor entries, `[bots.*]` overrides and anything else machine-specific. Add `effort = "medium"` under `[default]`, and under `[classes.compact]` if the OptChat plan has added it. Keep the comment header, and add two lines:
- `# Cost first: the cheapest model that gives acceptable output leads each list; stronger ones are fallbacks.`
- `# Every route names an effort; one that doesn't gets its class's effort, then [default], then medium.`

The existing `candidates_parse` test asserts `parse("claude:claude-opus-5-5").unwrap().effort == None`. That's still true, because `parse` is unchanged and only `resolve` fills the effort.

- [x] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`. Expected: 1 new test, all pass, no warnings.

- [x] **Step 5: Live check** (**live**). `herdbot route plan --bot herdbot-dev` prints a route ending in `:medium`. Then put `[bots.herdbot-dev]\nclasses.explore = ["claude:claude-haiku-5-5"]` in a scratch `HERDBOT_HOME`'s `routing.toml` and run `HERDBOT_HOME=<scratch> herdbot route explore --bot herdbot-dev`. Expected: `claude:claude-haiku-5-5:low` (from the class), or `:medium` if the scratch table has no class effort.

- [x] **Step 6: Docs.** In the README's Routing section, add: every launched agent gets an explicit effort; a candidate without one takes its class's `effort`, then `[default] effort`, then medium, so a harness never falls back to your global setting.

- [ ] **Step 7: Commit**

```bash
git add docs/superpowers/plans/2026-10-20-effort-default.md crates/herdbot/src/route.rs routing.toml README.md
git commit -m "fix(herdbot): never launch an agent without an explicit effort"
```
