//! routing.toml → (harness, model, effort) → herdr `agent.start` kind + args.

use std::process::Command;
use std::sync::OnceLock;

use toml::Value;

use crate::cooldown;
use crate::home::{self, Result};

pub const DEFAULT_ROUTING: &str = include_str!("../../../routing.toml");

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub harness: String,
    pub model: String,
    pub effort: Option<String>,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.harness, self.model)?;
        if let Some(e) = &self.effort {
            write!(f, ":{e}")?;
        }
        Ok(())
    }
}

pub fn parse(candidate: &str) -> Option<Route> {
    let mut parts = candidate.splitn(3, ':');
    let harness = parts.next()?.to_owned();
    let model = parts.next()?.to_owned();
    Some(Route {
        harness,
        model,
        effort: parts.next().map(str::to_owned),
    })
}

pub fn table() -> Result<Value> {
    let path = home::root().join("routing.toml");
    if !path.exists() {
        std::fs::create_dir_all(home::root())?;
        std::fs::write(&path, DEFAULT_ROUTING)?;
    }
    Ok(std::fs::read_to_string(path)?
        .parse::<toml::Table>()?
        .into())
}

/// Grok Bot subagent types map onto routing classes; anything unknown uses `[default]`.
fn class_key(class: &str) -> &str {
    match class {
        "bash" => "shell",
        "debug" | "explore" | "shell" | "plan" | "code" | "review" | "research"
        | "media_review" | "browser_use" | "computer_use" | "custom" => class,
        _ => "default",
    }
}

/// First available candidate for `class`, honouring `[bots.<slug>]` overrides and an
/// explicit `model` ("harness:model[:effort]") when given.
pub fn resolve(table: &Value, class: &str, bot: &str, explicit: Option<&str>) -> Result<Route> {
    let key = class_key(class);
    let strings = |v: Option<&Value>| -> Vec<String> {
        v.and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut candidates: Vec<String> = explicit.map(str::to_owned).into_iter().collect();
    candidates.extend(strings(
        table
            .get("bots")
            .and_then(|b| b.get(bot))
            .and_then(|b| b.get("classes"))
            .and_then(|c| c.get(key)),
    ));
    if key != "default" {
        candidates.extend(strings(
            table
                .get("classes")
                .and_then(|c| c.get(key))
                .and_then(|c| c.get("candidates")),
        ));
    }
    candidates.extend(strings(
        table.get("default").and_then(|d| d.get("candidates")),
    ));
    let mut rejected = Vec::new();
    for c in &candidates {
        let Some(route) = parse(c) else {
            rejected.push(format!("{c}: not harness:model[:effort]"));
            continue;
        };
        match available(table, &route) {
            Ok(()) => return Ok(route),
            Err(why) => rejected.push(format!("{c}: {why}")),
        }
    }
    Err(format!(
        "no available route for class {class:?}: {}",
        rejected.join("; ")
    )
    .into())
}

fn binary(harness: &str) -> Option<&'static str> {
    Some(match harness {
        "claude" => "claude",
        "codex" => "codex",
        "cursor" => "cursor-agent",
        "pi" => "pi",
        _ => return None,
    })
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

fn available(table: &Value, route: &Route) -> std::result::Result<(), String> {
    let bin = binary(&route.harness).ok_or("unknown harness")?;
    if !on_path(bin) {
        return Err(format!("{bin} not on PATH"));
    }
    if let Some(c) = cooldown::active(&route.harness) {
        return Err(format!(
            "cooling down until {} ({})",
            c.until_local(),
            // The kind only: this error reaches agents' panes, and the raw CLI line would trip
            // detection there.
            c.reason.split(':').next().unwrap_or_default()
        ));
    }
    if route.harness == "pi" {
        let provider = table
            .get("harness")
            .and_then(|h| h.get("pi"))
            .and_then(|p| p.get("provider"))
            .and_then(Value::as_str)
            .unwrap_or("opencode-go");
        if !pi_models(provider).iter().any(|m| m == &route.model) {
            return Err(format!(
                "{} is not a {provider} model in `pi --list-models`",
                route.model
            ));
        }
    }
    Ok(())
}

/// Model ids `pi --list-models` lists for `provider` (read once per process).
fn pi_models(provider: &str) -> &'static [String] {
    static MODELS: OnceLock<Vec<String>> = OnceLock::new();
    MODELS.get_or_init(|| {
        let out = Command::new("pi")
            .arg("--list-models")
            .output()
            .map(|o| o.stdout)
            .unwrap_or_default();
        String::from_utf8_lossy(&out)
            .lines()
            .filter_map(|l| {
                let mut cols = l.split_whitespace();
                (cols.next()? == provider).then(|| cols.next().map(str::to_owned))?
            })
            .collect()
    })
}

/// How the harness reaches herdbot's tools.
pub struct Launch {
    /// herdr agent kind.
    pub kind: &'static str,
    pub args: Vec<String>,
    /// Instructions for harnesses without a system-prompt flag go in as the first prompt.
    pub preamble_prompt: Option<String>,
}

/// `tool_args` is the herdbot argv (after the binary) that serves this agent's tools.
///
/// herdr rejects agent args it cannot quote safely for the shell (newlines, for one), so
/// instructions and MCP config go in files under `.state/launch/` named after `agent`.
pub fn launch(
    table: &Value,
    route: &Route,
    agent: &str,
    instructions: &str,
    tool_args: &[String],
    cwd: &str,
) -> Result<Launch> {
    let exe = std::env::current_exe()?.to_string_lossy().into_owned();
    let mcp_args: Vec<String> = std::iter::once("mcp".to_owned())
        .chain(tool_args.iter().cloned())
        .collect();
    // Harnesses may not pass the pane environment to MCP servers (codex does not), so the
    // home directory travels in the server config.
    let home_dir = home::root().to_string_lossy().into_owned();
    let mcp = serde_json::json!({ "mcpServers": { "herdbot": { "command": exe, "args": mcp_args, "env": { "HERDBOT_HOME": home_dir } } } });
    let dir = home::root().join(".state").join("launch");
    std::fs::create_dir_all(&dir)?;
    let prompt_file = dir.join(format!("{agent}.md"));
    std::fs::write(&prompt_file, instructions)?;
    let prompt_file = prompt_file.to_string_lossy().into_owned();
    let s = |v: &str| v.to_owned();
    let effort = route.effort.clone();
    Ok(match route.harness.as_str() {
        "claude" => {
            let mcp_file = dir.join(format!("{agent}.mcp.json"));
            std::fs::write(&mcp_file, serde_json::to_vec_pretty(&mcp)?)?;
            // Pre-allow herdbot's own tools only; everything else keeps claude's normal approvals.
            let mut args = vec![
                s("--model"),
                route.model.clone(),
                s("--mcp-config"),
                mcp_file.to_string_lossy().into_owned(),
                s("--allowedTools"),
                s("mcp__herdbot"),
            ];
            if let Some(e) = effort {
                args.extend([s("--effort"), e]);
            }
            args.extend([s("--append-system-prompt-file"), prompt_file]);
            Launch {
                kind: "claude",
                args,
                preamble_prompt: None,
            }
        }
        "codex" => {
            let toml_args = serde_json::to_string(&mcp_args)?;
            let mut args = vec![
                s("-m"),
                route.model.clone(),
                s("-c"),
                format!(
                    "mcp_servers.herdbot.command={}",
                    serde_json::to_string(&exe)?
                ),
                s("-c"),
                format!("mcp_servers.herdbot.args={toml_args}"),
                s("-c"),
                format!(
                    "mcp_servers.herdbot.env={{HERDBOT_HOME={}}}",
                    serde_json::to_string(&home_dir)?
                ),
                // Pre-approve herdbot's own tools only (claude gets the same via --allowedTools).
                s("-c"),
                s("mcp_servers.herdbot.default_tools_approval_mode=\"approve\""),
            ];
            if let Some(e) = effort {
                args.extend([s("-c"), format!("model_reasoning_effort={e}")]);
            }
            Launch {
                kind: "codex",
                args,
                preamble_prompt: Some(instructions.to_owned()),
            }
        }
        "cursor" => {
            // cursor-agent reads MCP servers from <cwd>/.cursor/mcp.json only. Add a herdbot
            // entry without bot/run args (several agents share one cwd); `herdbot mcp` then
            // takes its identity from HERDBOT_BOT / HERDBOT_RUN in the pane environment.
            let path = std::path::Path::new(cwd).join(".cursor").join("mcp.json");
            let mut config: serde_json::Value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_else(|| serde_json::json!({ "mcpServers": {} }));
            if config["mcpServers"].get("herdbot").is_none() {
                config["mcpServers"]["herdbot"] = serde_json::json!({ "command": exe, "args": ["mcp"], "env": { "HERDBOT_HOME": home_dir } });
                std::fs::create_dir_all(path.parent().unwrap())?;
                std::fs::write(&path, serde_json::to_vec_pretty(&config)?)?;
            }
            Launch {
                kind: "cursor",
                // --trust: a first run in a new cwd otherwise opens a trust modal that eats the
                // preamble prompt's keystrokes.
                args: vec![
                    s("--model"),
                    route.model.clone(),
                    s("--approve-mcps"),
                    s("--trust"),
                ],
                preamble_prompt: Some(instructions.to_owned()),
            }
        }
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
        other => return Err(format!("unknown harness {other}").into()),
    })
}

/// pi's argv. --no-extensions stops user extensions from swapping the model or failing over
/// to other providers; builtin:mcp and builtin:codemode let herdbot-pi.ts register
/// herdbot's MCP server for scripts (pi's --mcp-config flag comes from a third-party extension
/// that --no-extensions drops).
fn pi_args(
    provider: &str,
    model: &str,
    effort: Option<String>,
    extension: String,
    prompt_file: String,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "--no-extensions",
        "-e",
        "builtin:mcp",
        "-e",
        "builtin:codemode",
        "-e",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(extension);
    args.extend([
        "--provider".into(),
        provider.to_owned(),
        "--model".into(),
        model.to_owned(),
    ]);
    if let Some(e) = effort {
        args.extend(["--thinking".into(), e]);
    }
    args.extend(["--append-system-prompt".into(), prompt_file]);
    args
}

const PI_EXTENSION: &str = include_str!("../assets/herdbot-pi.ts");

fn pi_extension() -> Result<String> {
    let path = home::root().join(".state").join("herdbot-pi.ts");
    std::fs::create_dir_all(path.parent().unwrap())?;
    if std::fs::read_to_string(&path).ok().as_deref() != Some(PI_EXTENSION) {
        std::fs::write(&path, PI_EXTENSION)?;
    }
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                "--no-extensions",
                "-e",
                "builtin:mcp",
                "-e",
                "builtin:codemode",
                "-e",
                "/h/.state/herdbot-pi.ts",
                "--provider",
                "opencode-go",
                "--model",
                "kimi-k2.7-code",
                "--thinking",
                "medium",
                "--append-system-prompt",
                "/h/.state/launch/a.md",
            ]
        );
        assert!(!args.iter().any(|a| a == "--mcp-config"));
    }

    #[test]
    fn candidates_parse() {
        assert_eq!(
            parse("codex:gpt-6.1-sol:medium"),
            Some(Route {
                harness: "codex".into(),
                model: "gpt-6.1-sol".into(),
                effort: Some("medium".into())
            })
        );
        assert_eq!(parse("claude:claude-opus-5-5").unwrap().effort, None);
        assert_eq!(parse("claude"), None);
        assert_eq!(class_key("bash"), "shell");
        assert_eq!(class_key("watch_video"), "default");
    }

    #[test]
    fn default_table_parses() {
        let t: Value = DEFAULT_ROUTING.parse::<toml::Table>().unwrap().into();
        assert!(
            t["classes"]["explore"]["candidates"]
                .as_array()
                .unwrap()
                .len()
                > 1
        );
    }
}
