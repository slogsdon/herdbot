//! What happens when agent.prompt targets an agent that is mid-turn?
//!
//!   cargo run -p herdr-client --example busy

use std::time::Duration;

use herdr_client::{Client, agent_name};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let herdr = Client::from_env();
    let created = herdr
        .workspace_create("herdbot-busy", "/tmp", json!({}))
        .await?;
    let workspace_id = created["workspace"]["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let pane = created["root_pane"]["pane_id"].as_str().unwrap().to_owned();
    let outcome = async {
        let name = agent_name("busy-pi");
        let args: Vec<String> = [
            "--no-extensions",
            "--provider",
            "opencode-go",
            "--model",
            "deepseek-v4-flash",
            "--thinking",
            "low",
        ]
        .map(String::from)
        .to_vec();
        herdr.agent_start(&name, "pi", &pane, &args, 30_000).await?;
        herdr
            .agent_prompt(
                &name,
                "Use bash to run `sleep 20`, then reply with exactly FIRST.",
                None,
            )
            .await?;
        tokio::time::sleep(Duration::from_secs(6)).await;
        println!(
            "status before 2nd prompt: {}",
            herdr.agent_get(&name).await?["agent"]["agent_status"]
        );
        let second = herdr
            .agent_prompt(&name, "Reply with exactly SECOND.", None)
            .await;
        println!("2nd prompt result: {second:?}");
        tokio::time::sleep(Duration::from_secs(40)).await;
        let read = herdr
            .call(
                "agent.read",
                json!({ "target": name, "source": "recent_unwrapped", "lines": 40 }),
            )
            .await?;
        println!(
            "--- output ---\n{}",
            read["read"]["text"].as_str().unwrap_or_default()
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    herdr.workspace_close(&workspace_id).await?;
    outcome
}
