//! End-to-end spike against a live herdr server.
//!
//!   cargo run -p herdr-client --example spike                 # shell pane only
//!   cargo run -p herdr-client --example spike -- --agent pi   # also starts pi (opencode-go) in a pane
//!
//! Creates a throwaway workspace, splits a pane, runs a command, optionally starts an agent and
//! prompts it, prints every streamed event, then closes the workspace.

use std::time::Duration;

use herdr_client::{Client, Subscription, agent_name, on, on_pane};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = std::env::args().skip_while(|a| a != "--agent").nth(1);
    let herdr = Client::from_env();
    println!(
        "herdr {} (protocol {})",
        herdr.ping().await?,
        herdr_client::PROTOCOL
    );

    print_events(
        herdr
            .subscribe(&[
                on("pane.created"),
                on("pane.agent_detected"),
                on("pane.exited"),
            ])
            .await?,
    );

    let created = herdr
        .workspace_create(
            "herdbot-spike",
            "/tmp",
            json!({ "HERDBOT_BOT_ID": "spike" }),
        )
        .await?;
    let workspace_id = created["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned();
    println!("workspace {workspace_id}");

    let outcome = run(&herdr, &created, agent.as_deref()).await;
    // Let trailing events print, then always clean up.
    tokio::time::sleep(Duration::from_millis(300)).await;
    herdr.workspace_close(&workspace_id).await?;
    println!("closed {workspace_id}");
    outcome
}

async fn run(
    herdr: &Client,
    created: &Value,
    agent: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let root = created["root_pane"]["pane_id"].as_str().unwrap();
    let split = herdr.pane_split(root, "right", Some("/tmp")).await?;
    let pane = split["pane"]["pane_id"].as_str().unwrap().to_owned();
    println!("split {root} -> {pane}");

    // The agent goes into the brand-new split pane straight away, which exercises the
    // agent_pane_busy retry while its shell is still starting.
    let name = match agent {
        Some(kind) => Some(start_agent(herdr, kind, &pane).await?),
        None => None,
    };

    herdr.pane_run(root, "echo herdbot-$((40+2))").await?;
    herdr
        .pane_wait_for_substring(root, "herdbot-42", 10_000)
        .await?;
    println!("pane run ok: matched herdbot-42");

    let Some(name) = name else { return Ok(()) };
    // A turn that finishes (or errors) inside herdr's 5s activity gate reports
    // agent_prompt_stalled even though the prompt was delivered.
    match herdr
        .agent_prompt(
            &name,
            "Reply with exactly the word PONG and nothing else.",
            Some(120_000),
        )
        .await
    {
        Ok(reply) => println!("prompt settled: {}", reply["agent"]["agent_status"]),
        Err(herdr_client::Error::Api { code, .. }) if code == "agent_prompt_stalled" => {
            tokio::time::sleep(Duration::from_secs(2)).await;
            println!("prompt stalled (turn too fast to observe); reading output anyway");
        }
        Err(e) => return Err(e.into()),
    }
    let read = herdr
        .call(
            "agent.read",
            json!({ "target": name, "source": "recent_unwrapped", "lines": 15 }),
        )
        .await?;
    println!(
        "--- last output ---\n{}",
        read["read"]["text"].as_str().unwrap_or_default()
    );
    Ok(())
}

async fn start_agent(
    herdr: &Client,
    kind: &str,
    pane: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Cheapest models from routing.toml's explore class.
    let args: Vec<&str> = match kind {
        // --no-extensions: a user pi extension overrides the CLI model and fails over to other
        // providers. It also drops MCP (`-e builtin:mcp` did not restore it), so herdbot's pi
        // bridge must ship as its own `-e` extension.
        "pi" => vec![
            "--no-extensions",
            "--provider",
            "opencode-go",
            "--model",
            "deepseek-v4-flash",
            "--thinking",
            "low",
        ],
        "claude" => vec!["--model", "claude-haiku-5-5"],
        "codex" => vec!["-m", "gpt-6-luna", "-c", "model_reasoning_effort=low"],
        "cursor" => vec!["--model", "gemini-3.7-flash-high"],
        other => return Err(format!("unknown kind {other}").into()),
    };
    let args: Vec<String> = args.into_iter().map(String::from).collect();
    let name = agent_name(&format!("spike-{kind}"));
    print_events(
        herdr
            .subscribe(&[on_pane("pane.agent_status_changed", pane)])
            .await?,
    );
    herdr.agent_start(&name, kind, pane, &args, 30_000).await?;
    println!("agent {name} started in {pane} ({kind} {args:?})");
    Ok(name)
}

fn print_events(mut events: Subscription) {
    tokio::spawn(async move {
        while let Ok(Some(e)) = events.next().await {
            let d = &e.data;
            let pane = d
                .pointer("/pane/pane_id")
                .or(d.get("pane_id"))
                .cloned()
                .unwrap_or(Value::Null);
            let status = d
                .pointer("/pane/agent_status")
                .or(d.get("agent_status"))
                .cloned()
                .unwrap_or(Value::Null);
            println!("  event {} pane={pane} status={status}", e.event);
        }
    });
}
