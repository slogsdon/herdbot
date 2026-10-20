# pi Through Built-in MCP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** pi agents reach herdbot's tools through pi's built-in MCP client, talking to the same `herdbot mcp` server claude, codex and cursor use, instead of the `herdbot-pi.ts` tool bridge that shells out to `herdbot tool`.

**Architecture:** pi keeps `--no-extensions` (the model/provider guardrail). The built-in MCP extension is turned back on explicitly with `-e builtin:mcp`, and a 15-line `herdbot-pi.ts` registers the herdbot stdio server with `pi.registerMcpServer()` from the pane environment. pi's `--mcp-config` flag is **not** usable: it belongs to the third-party `pi-mcp-adapter` extension, which `--no-extensions` drops. Code mode is a separate, gated task because it hung in testing.

**Tech Stack:** Rust 2024 (herdbot crate), TypeScript pi extension (`@earendil-works/pi-coding-agent` 1.2.0 API), herdr 0.9.1.

## Global Constraints

- pi always launches with `--no-extensions --provider opencode-go --model <id>`. Never drop `--no-extensions`.
- Only `builtin:mcp` (and, if Task 3 passes, `builtin:codemode`) may be re-enabled. No other builtins.
- Tests stay in-file `#[cfg(test)] mod tests`, matching the crate.
- Commits: conventional (`feat:`, `fix:`, `docs:`, `refactor:`), explicit paths only, no `git add -A`/`.`.
- Each verification step is labeled **unit** (runs in `cargo test`) or **live** (needs herdr, the daemon and a signed-in pi).

## Verified facts this plan depends on (2026-10-20, pi 1.2.0)

- `pi --no-extensions -e builtin:mcp -e <ext.ts>` with `pi.registerMcpServer("herdbot", { command, args, env, exposure: "direct" })` declares `mcp__herdbot__task`, `mcp__herdbot__check_subagent`, `mcp__herdbot__stop_subagent`, `mcp__herdbot__send_to_agent`, `mcp__herdbot__list_bots`, and exits 0.
- `pi --no-extensions … --mcp-config x` fails: `Error: Unknown option: --mcp-config`.
- The built-in MCP also reads `~/.pi/agent/mcp.json`. Its servers default to `exposure: "codemode"`, so without `builtin:codemode` they are not declared to the model. A global server explicitly set to `"direct"` would leak into bot sessions.
- Adding `-e builtin:codemode` with `exposure: "codemode"` made `pi -p` hang past 150 s with no output.

---

### Task 1: Register herdbot as a pi MCP server

**Files:**
- Modify: `crates/herdbot/assets/herdbot-pi.ts` (whole file, 42 lines → ~20)
- Modify: `crates/herdbot/src/route.rs:294-322` (the `"pi"` arm of `launch`), plus a new test in `mod tests` (`:338`)

**Interfaces:**
- Consumes: pane env `HERDBOT_EXE`, `HERDBOT_BOT`, `HERDBOT_RUN` (optional), `HERDBOT_HOME`, set by `run::pane_env` (`src/run.rs:134`).
- Produces: `route::pi_args(provider: &str, model: &str, effort: Option<String>, extension: String, prompt_file: String) -> Vec<String>`, a pure helper the `"pi"` arm calls.

- [ ] **Step 1: Write the failing test** in `crates/herdbot/src/route.rs` `mod tests`:

```rust
    #[test]
    fn pi_keeps_guardrail_and_loads_builtin_mcp() {
        let args = pi_args(
            "opencode-go",
            "kimi-k2.7-code",
            Some("medium".into()),
            "/h/.state/herdbot-pi.ts".into(),
            "/h/.state/launch/a.md".into(),
        );
        assert_eq!(
            args,
            [
                "--no-extensions", "-e", "builtin:mcp", "-e", "/h/.state/herdbot-pi.ts",
                "--provider", "opencode-go", "--model", "kimi-k2.7-code",
                "--thinking", "medium",
                "--append-system-prompt", "/h/.state/launch/a.md",
            ]
        );
        assert!(!args.iter().any(|a| a == "--mcp-config"));
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot pi_keeps_guardrail`
Expected: compile error `cannot find function pi_args`.

- [ ] **Step 3: Add `pi_args` and use it in the `"pi"` arm.** Add above `const PI_EXTENSION`:

```rust
/// pi's argv. --no-extensions stops user extensions from swapping the model or failing over
/// to other providers; builtin:mcp is the one built-in turned back on, so herdbot-pi.ts can
/// register herdbot's MCP server (pi's --mcp-config flag comes from a third-party extension
/// that --no-extensions drops).
fn pi_args(
    provider: &str,
    model: &str,
    effort: Option<String>,
    extension: String,
    prompt_file: String,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "--no-extensions", "-e", "builtin:mcp", "-e",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(extension);
    args.extend(["--provider".into(), provider.to_owned(), "--model".into(), model.to_owned()]);
    if let Some(e) = effort {
        args.extend(["--thinking".into(), e]);
    }
    args.extend(["--append-system-prompt".into(), prompt_file]);
    args
}
```

Replace the body of the `"pi" => { … }` arm with:

```rust
        "pi" => {
            let provider = table
                .get("harness")
                .and_then(|h| h.get("pi"))
                .and_then(|p| p.get("provider"))
                .and_then(Value::as_str)
                .unwrap_or("opencode-go");
            // herdbot-pi.ts registers `herdbot mcp` with bot/run from the pane env.
            Launch {
                kind: "pi",
                args: pi_args(provider, &route.model, effort, pi_extension()?, prompt_file),
                preamble_prompt: None,
            }
        }
```

- [ ] **Step 4: Run the tests to verify they pass** (**unit**)

Run: `cargo test -p herdbot`
Expected: 14 passed.

- [ ] **Step 5: Replace `crates/herdbot/assets/herdbot-pi.ts`** with:

```ts
/**
 * herdbot tools for pi. pi runs with --no-extensions -e builtin:mcp (see route.rs), so this
 * extension only registers herdbot's stdio MCP server; pi's built-in MCP client does the rest.
 * Bot and run identity come from HERDBOT_BOT / HERDBOT_RUN in the pane environment.
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

export default function (pi: ExtensionAPI) {
	const args = ["mcp", "--bot", process.env.HERDBOT_BOT ?? ""];
	if (process.env.HERDBOT_RUN) args.push("--run", process.env.HERDBOT_RUN);
	pi.registerMcpServer("herdbot", {
		command: process.env.HERDBOT_EXE ?? "herdbot",
		args,
		env: { HERDBOT_HOME: process.env.HERDBOT_HOME ?? "" },
		// "direct": declared to the model at once. codemode is Task 3.
		exposure: "direct",
		description: "herdbot: start subagents, message other bots, report results",
	});
}
```

- [ ] **Step 6: Live check, pi alone** (**live**, no herdr needed). Run from a scratch dir:

```bash
cargo install --path crates/herdbot
cp crates/herdbot/assets/herdbot-pi.ts /tmp/herdbot-pi.ts
HERDBOT_EXE=$HOME/.cargo/bin/herdbot HERDBOT_BOT=herdbot-dev HERDBOT_HOME=$HOME/Bots \
  timeout 120 pi --no-extensions -e builtin:mcp -e /tmp/herdbot-pi.ts \
  --provider opencode-go --model deepseek-v4-flash \
  -p "Call mcp__herdbot__list_bots and print its raw output."
```

Expected: exit 0 and one line per bot (`herdbot-dev: …`), matching `herdbot list`.

- [ ] **Step 7: Live check through herdbot** (**live**, daemon running). Restart the daemon on the new binary, then start a pi-routed subagent. `explore` resolves to `pi:deepseek-v4-flash:low` first (`herdbot route explore` should print that):

```bash
launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon
herdbot route explore
herdbot task herdbot-dev explore "Call list_bots, then report_result with its output."
herdbot runs | tail -1
```

Expected: within about a minute the run shows `done`, and `cat ~/Bots/.runs/<id>.json` has a `result` listing the bots. That proves `report_result` came through MCP, not the harvest fallback, because a harvested result starts with the pane text.

- [ ] **Step 8: Commit**

```bash
git add crates/herdbot/src/route.rs crates/herdbot/assets/herdbot-pi.ts
git commit -m "feat(herdbot): serve pi's tools through its built-in MCP client"
```

---

### Task 2: Remove the bridge leftovers and update docs

**Files:**
- Modify: `crates/herdbot/src/run.rs:134-146` (`pane_env`: drop `HERDBOT_TOOLS`)
- Modify: `crates/herdbot/src/main.rs:28` (USAGE line for `tool`)
- Modify: `crates/herdbot/src/tools.rs:1-3` (module doc)
- Modify: `README.md:57-68` (diagram), `README.md:138` (pi row), `README.md:186` (roadmap bullet)

**Interfaces:**
- Consumes: Task 1's launch. After it, nothing reads `HERDBOT_TOOLS`.
- Produces: nothing new. `herdbot tool` stays as a shell debugging entry point (8 lines in `main.rs`).

- [ ] **Step 1: Confirm `HERDBOT_TOOLS` is dead** (**unit**)

Run: `grep -rn HERDBOT_TOOLS crates/`
Expected: only `crates/herdbot/src/run.rs:137`. Stop and re-plan if anything else reads it.

- [ ] **Step 2: Delete the `"HERDBOT_TOOLS": …` line** from `pane_env` in `run.rs`, and drop `use crate::tools;` from `run.rs` only if the compiler reports it unused. `start` still uses `tools::main_preamble`, so it most likely stays.

- [ ] **Step 3: Update the text.**
- `main.rs` USAGE: `herdbot tool <name> --bot BOT [--run ID] <json>   one tool call (pi extension)` → `… one tool call, for debugging from a shell`.
- `tools.rs` module doc, first line: `served over MCP (claude, codex, cursor) and through \`herdbot tool\` (pi's extension)` → `served over MCP to every harness (pi registers the server from herdbot-pi.ts); \`herdbot tool\` calls one from a shell`.
- README diagram: replace the two lines ending `(claude, codex, cursor: stdio MCP;` / `pi: herdbot-pi.ts extension)` with one `(stdio MCP, all harnesses)`. Remove the `herdbot tool <name> …` arrow line.
- README pi row, Tools column: `` `-e builtin:mcp -e herdbot-pi.ts`, which registers the herdbot stdio MCP server with `pi.registerMcpServer` (`--mcp-config` comes from a third-party extension that `--no-extensions` drops) ``. Launch column: `` `--no-extensions -e builtin:mcp -e <herdbot-pi.ts> --provider opencode-go --model`, `--thinking` ``.
- README pi guardrail paragraph (after the table): add "Built-in MCP also reads `~/.pi/agent/mcp.json`. Servers there default to code-mode exposure, which a bot's pi doesn't load, so they stay out of reach. A global server set to `\"exposure\": \"direct\"` would show up in bot sessions."
- README roadmap: delete the "pi through MCP and code mode" bullet if Task 3 is skipped, or reduce it to the code-mode result.

- [ ] **Step 4: Build and test** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: 14 passed, no warnings.

- [ ] **Step 5: Live regression** (**live**). Repeat Task 1 Step 7 with a `cargo install` binary and a restarted daemon. Expected: same `done` result.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/run.rs crates/herdbot/src/main.rs crates/herdbot/src/tools.rs README.md
git commit -m "refactor(herdbot): drop the pi tool-list env now that pi uses MCP"
```

---

### Task 3 (gated spike): pi code mode

The pass/fail rule is decided up front. **Pass:** a `-p` run with `builtin:codemode` and herdbot at `exposure: "codemode"` calls `list_bots` from a script and exits 0 in under 60 s, three runs out of three. **Fail:** any hang or error that Steps 1-3 can't explain. On fail, record the finding in the README roadmap and stop. Task 1's `direct` exposure stays as shipped.

**Files:**
- Modify (pass only): `crates/herdbot/src/route.rs` (`pi_args`), `crates/herdbot/assets/herdbot-pi.ts` (`exposure`), `README.md`

- [ ] **Step 1: Reproduce the hang with a timer** (**live**):

```bash
sed 's/"direct"/"codemode"/' crates/herdbot/assets/herdbot-pi.ts > /tmp/herdbot-pi-cm.ts
time HERDBOT_EXE=$HOME/.cargo/bin/herdbot HERDBOT_BOT=herdbot-dev HERDBOT_HOME=$HOME/Bots \
  timeout 150 pi --no-extensions -e builtin:mcp -e builtin:codemode -e /tmp/herdbot-pi-cm.ts \
  --provider opencode-go --model deepseek-v4-flash \
  -p "Use codemode: call the herdbot list_bots tool and print the result."; echo exit=$?
```

- [ ] **Step 2: Isolate the global MCP servers.** Hypothesis: codemode waits on every server, including the slow `rea` from `~/.pi/agent/mcp.json`. Test it by making `rea` hidden for this project only, in `.pi/mcp.json` of the scratch cwd: `{"mcpServers":{"rea":{"enabled":false}}}`. pi reads project config only for trusted projects, so check `~/.pi/agent/trust.json` covers the scratch dir or run `pi` interactively there once to trust it. Rerun Step 1.

- [ ] **Step 3: Check the model.** Rerun Step 1 with `--model kimi-k2.7-code`. A flash model may loop on codemode scripts.

- [ ] **Step 4: Decide.**
  - **Pass:** add `"-e", "builtin:codemode"` after `builtin:mcp` in `pi_args`, update the Task 1 test's expected argv, and set `exposure: "codemode"` in `herdbot-pi.ts`. The preamble's tool names don't change: codemode scripts find tools with `searchTools()`. If Step 2 was needed, global servers become reachable from scripts, so either disable them per bot cwd or note in the README that code mode exposes every server in `~/.pi/agent/mcp.json`. That's a decision for Shane; ask before shipping it. Then run `cargo test -p herdbot` and repeat Task 1 Step 7.
  - **Fail:** replace the roadmap bullet with what failed and on which pi version.

- [ ] **Step 5: Commit** (pass)

```bash
git add crates/herdbot/src/route.rs crates/herdbot/assets/herdbot-pi.ts README.md
git commit -m "feat(herdbot): run pi bots in code mode"
```

or (fail)

```bash
git add README.md
git commit -m "docs: record why pi code mode is not enabled"
```
