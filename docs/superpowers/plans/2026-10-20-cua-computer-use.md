# Computer Use Through cua Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `computer_use` subagents run on claude with a second MCP server, `cua`, which drives a Mac through `cua-driver mcp`. With no host configured that is this Mac. With `[cua] host` set in `herdbot.toml`, it's a remote Mac over `ssh -T`.

**Architecture:** `route::launch` learns the task class. For `computer_use` it adds a `cua` entry next to `herdbot` in the claude MCP config file it already writes. The entry is built by a pure function from `herdbot.toml`'s `[cua]` table. cua's tools are **not** pre-approved (Shane's decision, 2026-10-20). `--allowedTools` stays `mcp__herdbot`, so each cua action waits for approval in the pane.

**Tech Stack:** Rust 2024, claude `--mcp-config`, cua-driver (trycua, MIT), OpenSSH.

## Global Constraints

- `--allowedTools` stays exactly `mcp__herdbot`. Never add `mcp__cua`.
- `computer_use` runs on claude only. Any other harness for that class fails the run with a clear message instead of starting without cua.
- No `[cua]` table means local `cua-driver` on `PATH`. `[cua] host` means `ssh -T <host> <command> mcp`.
- Tests stay in-file `#[cfg(test)]` and never touch `$HERDBOT_HOME` (no env-var mutation in tests). Live checks are labeled **live**.
- Commits: conventional, explicit paths only.

## Prerequisites (Shane, by hand, before Task 3's live checks)

These install software and grant macOS privacy permissions, so the implementer does not run them:

1. Install, from the cua docs: `/bin/bash -c "$(curl -fsSL https://cua.ai/driver/install.sh)"`. It installs `CuaDriver.app` in `/Applications` and the binary at `~/.local/bin/cua-driver`.
2. `cua-driver permissions grant`, then turn on CuaDriver under **Accessibility** and **Screen & System Audio Recording** in System Settings. Check with `cua-driver permissions status` and `cua-driver status`.
3. For a remote Mac, do steps 1 and 2 on that Mac, and make sure `ssh -T <host> true` works without a password prompt (key auth). The remote non-interactive shell may not have `~/.local/bin` on `PATH`, which is why `[cua] command` exists.

---

### Task 1: `[cua]` config and the MCP server entry

**Files:**
- Modify: `crates/herdbot/src/home.rs:276-287` (next to `paths_rewrites`) plus a test
- Modify: `crates/herdbot/src/route.rs` (new `cua_server` and `mcp_config`) plus tests

**Interfaces:**
- Produces:
  - `home::Cua { pub host: Option<String>, pub command: Option<String> }` (`Debug, Clone, Default, PartialEq, Deserialize`)
  - `home::parse_cua(toml_text: &str) -> Cua` (pure) and `home::cua_config() -> Cua` (reads `herdbot.toml`)
  - `route::cua_server(cua: &home::Cua) -> serde_json::Value`
  - `route::mcp_config(exe: &str, mcp_args: &[String], home_dir: &str, cua: Option<serde_json::Value>) -> serde_json::Value`

- [ ] **Step 1: Write the failing tests.** In `home.rs` tests:

```rust
    #[test]
    fn cua_table_is_optional() {
        assert_eq!(parse_cua(""), Cua::default());
        assert_eq!(parse_cua("[paths]\n\"/a\" = \"/b\"\n"), Cua::default());
        assert_eq!(
            parse_cua("[cua]\nhost = \"shane@studio.local\"\ncommand = \"~/.local/bin/cua-driver\"\n"),
            Cua { host: Some("shane@studio.local".into()), command: Some("~/.local/bin/cua-driver".into()) }
        );
    }
```

In `route.rs` tests:

```rust
    #[test]
    fn cua_server_local_and_ssh() {
        use serde_json::json;
        assert_eq!(
            cua_server(&home::Cua::default()),
            json!({ "command": "cua-driver", "args": ["mcp"] })
        );
        let remote = home::Cua { host: Some("shane@studio.local".into()), command: Some("~/.local/bin/cua-driver".into()) };
        // The remote shell expands ~, so the command goes through untouched.
        assert_eq!(
            cua_server(&remote),
            json!({ "command": "ssh", "args": ["-T", "shane@studio.local", "~/.local/bin/cua-driver", "mcp"] })
        );
    }

    #[test]
    fn mcp_config_adds_cua_only_when_given() {
        let args = vec!["mcp".to_owned(), "--bot".to_owned(), "b".to_owned()];
        let plain = mcp_config("/bin/herdbot", &args, "/h", None);
        assert_eq!(plain["mcpServers"]["herdbot"]["command"], "/bin/herdbot");
        assert_eq!(plain["mcpServers"]["herdbot"]["env"]["HERDBOT_HOME"], "/h");
        assert!(plain["mcpServers"].get("cua").is_none());
        let with = mcp_config("/bin/herdbot", &args, "/h", Some(cua_server(&home::Cua::default())));
        assert_eq!(with["mcpServers"]["cua"]["command"], "cua-driver");
    }
```

- [ ] **Step 2: Run them to verify they fail** (**unit**)

Run: `cargo test -p herdbot cua`
Expected: compile errors (`parse_cua`, `Cua`, `cua_server`, `mcp_config` not found).

- [ ] **Step 3: Implement.** In `home.rs`, after `paths_rewrites`:

```rust
/// `[cua]` in herdbot.toml: where computer_use subagents reach cua-driver. No host means this
/// Mac; `command` defaults to `cua-driver` on PATH.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Cua {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

pub fn parse_cua(text: &str) -> Cua {
    #[derive(Deserialize, Default)]
    struct Config {
        #[serde(default)]
        cua: Cua,
    }
    toml::from_str::<Config>(text).unwrap_or_default().cua
}

pub fn cua_config() -> Cua {
    parse_cua(&fs::read_to_string(root().join("herdbot.toml")).unwrap_or_default())
}
```

In `route.rs`, above `pub struct Launch`. `route.rs` imports `toml::Value` as `Value`, so these two use `serde_json::Value` by its full path:

```rust
/// cua-driver's stdio MCP server, locally or through `ssh -T` (no tty, so stdio stays clean).
pub fn cua_server(cua: &home::Cua) -> serde_json::Value {
    let command = cua.command.clone().unwrap_or_else(|| "cua-driver".into());
    match &cua.host {
        None => serde_json::json!({ "command": command, "args": ["mcp"] }),
        Some(host) => serde_json::json!({ "command": "ssh", "args": ["-T", host, command, "mcp"] }),
    }
}

/// The MCP config claude gets: herdbot's server, plus cua for computer_use.
pub fn mcp_config(
    exe: &str,
    mcp_args: &[String],
    home_dir: &str,
    cua: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut config = serde_json::json!({ "mcpServers": { "herdbot": { "command": exe, "args": mcp_args, "env": { "HERDBOT_HOME": home_dir } } } });
    if let Some(cua) = cua {
        config["mcpServers"]["cua"] = cua;
    }
    config
}
```

In `launch`, replace the `let mcp = serde_json::json!({ "mcpServers": … });` line with `let mcp = mcp_config(&exe, &mcp_args, &home_dir, None);`. Task 2 passes the real `cua`. Also change the route tests' `use` lines if needed: `cua_server` returns `serde_json::Value`, and the tests use `serde_json::json!`.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: 16 passed (13 + 3), no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/herdbot/src/home.rs crates/herdbot/src/route.rs
git commit -m "feat(herdbot): read [cua] from herdbot.toml and build cua-driver's MCP entry"
```

---

### Task 2: Give computer_use runs the cua server

**Files:**
- Modify: `crates/herdbot/src/route.rs:191-198` (`launch` signature) and the `"claude"` arm (`:215-237`)
- Modify: `crates/herdbot/src/run.rs:197-204` (the `route::launch(…)` call in `start`)

**Interfaces:**
- Consumes: Task 1's `cua_server`, `mcp_config`, `home::cua_config`, and the existing `class_key`.
- Produces: `route::launch(table, route, agent, instructions, tool_args, cwd, class: &str) -> Result<Launch>`, with `class` as the new last parameter. Plus `fn cua_for(class: &str, harness: &str) -> std::result::Result<bool, String>` (pure, tested).

- [ ] **Step 1: Write the failing test** in `route.rs` tests:

```rust
    #[test]
    fn only_claude_takes_computer_use() {
        assert_eq!(cua_for("computer_use", "claude"), Ok(true));
        assert_eq!(cua_for("code", "claude"), Ok(false));
        assert_eq!(cua_for("code", "codex"), Ok(false));
        assert!(cua_for("computer_use", "codex").unwrap_err().contains("claude"));
    }
```

- [ ] **Step 2: Run it to verify it fails** (**unit**)

Run: `cargo test -p herdbot only_claude_takes`. Expected: compile error.

- [ ] **Step 3: Implement.** In `route.rs`:

```rust
/// Whether this launch gets cua. computer_use on another harness is an error rather than a
/// subagent that silently has no way to see the screen.
fn cua_for(class: &str, harness: &str) -> std::result::Result<bool, String> {
    match (class_key(class) == "computer_use", harness) {
        (false, _) => Ok(false),
        (true, "claude") => Ok(true),
        (true, other) => Err(format!("computer_use runs on claude only (cua is wired for claude), not {other}")),
    }
}
```

Add `class: &str` as the last parameter of `launch`. At the top of its body:

```rust
    let cua = cua_for(class, &route.harness)?.then(|| cua_server(&home::cua_config()));
```

Then pass `cua.clone()` in place of `None` in `mcp_config(…)`. Only the claude arm writes `mcp` to a file. The other arms build their own config and never reach `cua = Some`, because `cua_for` already errored.

In `run.rs` `start`, add `class` as the last argument of the `route::launch(…)` call.

- [ ] **Step 4: Run the tests** (**unit**)

Run: `cargo test -p herdbot && cargo clippy -p herdbot -- -D warnings`
Expected: 17 passed, no warnings.

- [ ] **Step 5: Check the written config** (**live**, no cua needed). With the daemon on the new binary:

```bash
cargo install --path crates/herdbot && launchctl kickstart -k gui/$(id -u)/dev.herdbot.daemon
herdbot task herdbot-dev computer_use "Say hello, then report_result with hello."
ls -t ~/Bots/.state/launch/*.mcp.json | head -1 | xargs cat
```

Expected: that file has both `herdbot` and `cua` under `mcpServers`, with `cua` as `{"command":"cua-driver","args":["mcp"]}`. Stop the run afterwards (`herdbot runs | tail -1`, then `stop_subagent` from a bot or close the pane) if cua isn't installed yet.

- [ ] **Step 6: Commit**

```bash
git add crates/herdbot/src/route.rs crates/herdbot/src/run.rs
git commit -m "feat(herdbot): give computer_use subagents cua-driver over MCP"
```

---

### Task 3: Live end-to-end checks and docs

Needs the Prerequisites.

**Files:**
- Modify: `README.md` (Roadmap bullet → a "Computer use" subsection under "How it works"; Bot layout `herdbot.toml` comment line)

- [ ] **Step 1: Local MCP handshake** (**live**):

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}' '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | timeout 20 cua-driver mcp | head -c 600
```

Expected: an `initialize` result, then a tools list that includes `list_apps`. Record the tool names. The README section names them.

- [ ] **Step 2: Local run, approvals on** (**live**):

```bash
herdbot task herdbot-dev computer_use "Use the cua tools to list the running apps, then call report_result with the list."
```

Expected:
1. The new pane shows claude asking to approve an `mcp__cua__…` call.
2. herdbot-dev receives `[subagent <id> "shell task" is blocked]` once.
3. After you approve in the pane, the run ends `done` with an app list.

Note it down if later approvals in the same run don't notify the parent again. That is current behaviour (`notified_blocked` is set once per run), and it isn't changed here.

- [ ] **Step 3: Remote run** (**live**, only if a remote Mac is available). Add to `~/Bots/herdbot.toml`:

```toml
[cua]
host = "user@other-mac.local"
command = "~/.local/bin/cua-driver"
```

Then repeat Step 1 as `… | ssh -T user@other-mac.local '~/.local/bin/cua-driver' mcp | head -c 600` and Step 2. Expected: the same results, with apps from the remote Mac. If there's no remote Mac, record "remote path not exercised". Don't call it passed.

- [ ] **Step 4: Docs.** In `README.md`:
  - Replace the "Computer use through cua" roadmap bullet with a `### Computer use` subsection. It should cover: `computer_use` routes to claude only; the run's MCP config gains a `cua` server; `[cua] host` / `command` in `herdbot.toml` (local by default, `ssh -T` when a host is set); the prerequisites above; every cua action asks for approval in the pane because `--allowedTools` stays `mcp__herdbot`, and the parent is told about the first wait only; the tool names from Step 1.
  - Bot layout: change the `herdbot.toml` line to `# [paths] rewrites; [cua] host/command for computer_use`.
  - Status: move "computer use through cua" from "Not built yet" to the working list, with the date verified, and only if Step 2 passed.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: document computer use through cua"
```
