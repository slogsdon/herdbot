//! Minimal MCP server over stdio (newline-delimited JSON-RPC 2.0): initialize, tools/list,
//! tools/call, ping. Nothing else is needed for tool-only servers.

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::home::Result;
use crate::tools::{self, Ctx};

pub async fn serve(ctx: Ctx) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        }; // notification
        let params = &request["params"];
        let reply = match request["method"].as_str().unwrap_or_default() {
            "initialize" => Ok(json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "herdbot", "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools::list_for(&ctx) })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default();
                let outcome = tools::call(&ctx, name, &params["arguments"]).await;
                let (text, is_error) = match outcome {
                    Ok(text) => (text, false),
                    Err(e) => (e.to_string(), true),
                };
                Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
            }
            other => {
                Err(json!({ "code": -32601, "message": format!("method not found: {other}") }))
            }
        };
        let message = match reply {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        };
        stdout.write_all(format!("{message}\n").as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(())
}
