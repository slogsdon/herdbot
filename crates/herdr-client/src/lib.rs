//! Async client for the herdr socket API.
//!
//! Wire format (verified against herdr 0.9.1, protocol 22): newline-delimited JSON over a
//! Unix socket. A request is `{"id","method","params"}`; the reply is `{"id","result"}` or
//! `{"id","error":{"code","message"}}`. The server closes a connection after one request,
//! so [`Client::call`] dials per request. `events.subscribe` keeps its connection open and
//! streams `{"event","data"}` lines, exposed as [`Subscription`].
//!
//! Params and results are `serde_json::Value`; the full JSON Schema is vendored in
//! `schema/` for when typed bindings earn their keep.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;

/// The protocol version this client was written against.
pub const PROTOCOL: u64 = 22;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// herdr returned an error body.
    Api {
        code: String,
        message: String,
    },
    /// The reply did not match the documented envelope.
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "herdr socket: {e}"),
            Error::Json(e) => write!(f, "herdr json: {e}"),
            Error::Api { code, message } => write!(f, "herdr {code}: {message}"),
            Error::Protocol(m) => write!(f, "herdr protocol: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Agent lifecycle states reported by herdr.
pub const AGENT_STATUSES: [&str; 5] = ["idle", "working", "blocked", "done", "unknown"];

#[derive(Clone, Debug)]
pub struct Client {
    socket: PathBuf,
}

impl Client {
    /// `$HERDR_SOCKET` if set, else `~/.config/herdr/herdr.sock`.
    pub fn from_env() -> Self {
        let socket = std::env::var_os("HERDR_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME").unwrap_or_default();
                Path::new(&home).join(".config/herdr/herdr.sock")
            });
        Self { socket }
    }

    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    /// Send one request and return its `result`.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let (mut lines, _write) = self.send(method, params).await?;
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| Error::Protocol(format!("{method}: connection closed without reply")))?;
        unwrap_reply(serde_json::from_str(&line)?)
    }

    /// Connect, verify the protocol version, and return the server version.
    pub async fn ping(&self) -> Result<String> {
        let pong = self.call("ping", json!({})).await?;
        let protocol = pong["protocol"].as_u64();
        if protocol != Some(PROTOCOL) {
            return Err(Error::Protocol(format!(
                "server speaks protocol {protocol:?}, client expects {PROTOCOL}"
            )));
        }
        Ok(pong["version"].as_str().unwrap_or_default().to_owned())
    }

    /// Subscribe with full subscription objects, e.g. `{"type":"pane.created"}`. Some types
    /// are per-pane: `pane.agent_status_changed` and `pane.scroll_changed` need `pane_id`,
    /// `pane.output_matched` also needs `source` and `match`. See [`on`] and [`on_pane`].
    pub async fn subscribe(&self, subscriptions: &[Value]) -> Result<Subscription> {
        let (mut lines, write) = self
            .send(
                "events.subscribe",
                json!({ "subscriptions": subscriptions }),
            )
            .await?;
        let line = lines
            .next_line()
            .await?
            .ok_or_else(|| Error::Protocol("subscribe: connection closed".into()))?;
        let started = unwrap_reply(serde_json::from_str(&line)?)?;
        if started["type"] != "subscription_started" {
            return Err(Error::Protocol(format!(
                "subscribe: unexpected reply {started}"
            )));
        }
        Ok(Subscription {
            lines,
            _write: write,
        })
    }

    async fn send(
        &self,
        method: &str,
        params: Value,
    ) -> Result<(
        Lines<BufReader<OwnedReadHalf>>,
        tokio::net::unix::OwnedWriteHalf,
    )> {
        let stream = UnixStream::connect(&self.socket).await?;
        let (read, mut write) = stream.into_split();
        let mut request =
            serde_json::to_vec(&json!({ "id": method, "method": method, "params": params }))?;
        request.push(b'\n');
        write.write_all(&request).await?;
        Ok((BufReader::new(read).lines(), write))
    }

    // Thin wrappers for the methods herdbot uses. Each returns herdr's `result` object.

    pub async fn workspace_create(&self, label: &str, cwd: &str, env: Value) -> Result<Value> {
        self.call(
            "workspace.create",
            json!({ "label": label, "cwd": cwd, "env": env, "focus": false }),
        )
        .await
    }

    pub async fn workspace_close(&self, workspace_id: &str) -> Result<Value> {
        self.call("workspace.close", json!({ "workspace_id": workspace_id }))
            .await
    }

    /// `direction` is `"right"` or `"down"`. Never steals focus.
    pub async fn pane_split(
        &self,
        target_pane_id: &str,
        direction: &str,
        cwd: Option<&str>,
    ) -> Result<Value> {
        self.call(
            "pane.split",
            json!({ "target_pane_id": target_pane_id, "direction": direction, "cwd": cwd, "focus": false }),
        )
        .await
    }

    /// Type a command and press Enter (what `herdr pane run` does).
    pub async fn pane_run(&self, pane_id: &str, command: &str) -> Result<Value> {
        self.call(
            "pane.send_input",
            json!({ "pane_id": pane_id, "text": command, "keys": ["enter"] }),
        )
        .await
    }

    /// `source`: `visible` | `recent` | `recent_unwrapped` | `detection`.
    pub async fn pane_read(
        &self,
        pane_id: &str,
        source: &str,
        lines: Option<u32>,
    ) -> Result<Value> {
        self.call(
            "pane.read",
            json!({ "pane_id": pane_id, "source": source, "lines": lines }),
        )
        .await
    }

    pub async fn pane_wait_for_substring(
        &self,
        pane_id: &str,
        needle: &str,
        timeout_ms: u64,
    ) -> Result<Value> {
        self.call(
            "pane.wait_for_output",
            json!({
                "pane_id": pane_id,
                "source": "recent_unwrapped",
                "match": { "type": "substring", "value": needle },
                "timeout_ms": timeout_ms,
            }),
        )
        .await
    }

    pub async fn pane_close(&self, pane_id: &str) -> Result<Value> {
        self.call("pane.close", json!({ "pane_id": pane_id })).await
    }

    /// `name` must match `[a-z][a-z0-9_-]{0,31}`; `args` go to the harness after `--`.
    /// `timeout_ms` must be in (3000, 300000]; herdr's CLI defaults to 30000.
    ///
    /// Over the socket this returns immediately with `launch_pending: true`; the name is not
    /// promptable until launch completes. Waits (polling `agent.get`) until the agent is
    /// `interactive_ready`, and returns the final agent object.
    pub async fn agent_start(
        &self,
        name: &str,
        kind: &str,
        pane_id: &str,
        args: &[String],
        timeout_ms: u64,
    ) -> Result<Value> {
        self.call(
            "agent.start",
            json!({ "name": name, "kind": kind, "pane_id": pane_id, "args": args, "timeout_ms": timeout_ms }),
        )
        .await?;
        // ponytail: 250ms poll; switch to a pane.agent_detected subscription if latency matters.
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            let mut got = self.agent_get(name).await?;
            let agent = got["agent"].take();
            if agent["interactive_ready"] == true && agent["launch_pending"] != true {
                return Ok(agent);
            }
            if agent["agent_status"] == "blocked" {
                return Err(Error::Api {
                    code: "agent_blocked".into(),
                    message: format!("{name} blocked during startup"),
                });
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(Error::Api {
                    code: "timeout".into(),
                    message: format!("{name} not ready after {timeout_ms}ms: {agent}"),
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    }

    /// Submit a prompt. With `wait_ms`, returns after the first settled idle/done/blocked.
    pub async fn agent_prompt(
        &self,
        target: &str,
        text: &str,
        wait_ms: Option<u64>,
    ) -> Result<Value> {
        let wait = wait_ms.map(|t| json!({ "timeout_ms": t, "until": [] }));
        self.call(
            "agent.prompt",
            json!({ "target": target, "text": text, "wait": wait }),
        )
        .await
    }

    pub async fn agent_get(&self, target: &str) -> Result<Value> {
        self.call("agent.get", json!({ "target": target })).await
    }

    pub async fn agent_send_keys(&self, target: &str, keys: &[&str]) -> Result<Value> {
        self.call("agent.send_keys", json!({ "target": target, "keys": keys }))
            .await
    }
}

/// A global subscription: `on("pane.created")`.
pub fn on(event_type: &str) -> Value {
    json!({ "type": event_type })
}

/// A per-pane subscription: `on_pane("pane.agent_status_changed", "w1:p2")`.
pub fn on_pane(event_type: &str, pane_id: &str) -> Value {
    json!({ "type": event_type, "pane_id": pane_id })
}

/// A live event stream. Drop it to unsubscribe.
pub struct Subscription {
    lines: Lines<BufReader<OwnedReadHalf>>,
    _write: tokio::net::unix::OwnedWriteHalf,
}

/// One streamed event, e.g. `event == "pane_agent_status_changed"`.
#[derive(Debug, Clone)]
pub struct Event {
    pub event: String,
    pub data: Value,
}

impl Subscription {
    /// Next event, or `None` when the server closes the stream.
    pub async fn next(&mut self) -> Result<Option<Event>> {
        let Some(line) = self.lines.next_line().await? else {
            return Ok(None);
        };
        let mut v: Value = serde_json::from_str(&line)?;
        let event = v["event"]
            .as_str()
            .ok_or_else(|| Error::Protocol(format!("event without name: {line}")))?
            .to_owned();
        Ok(Some(Event {
            event,
            data: v["data"].take(),
        }))
    }
}

fn unwrap_reply(mut reply: Value) -> Result<Value> {
    if let Some(err) = reply.get("error") {
        return Err(Error::Api {
            code: err["code"].as_str().unwrap_or("unknown").to_owned(),
            message: err["message"].as_str().unwrap_or_default().to_owned(),
        });
    }
    match reply.get_mut("result") {
        Some(result) => Ok(result.take()),
        None => Err(Error::Protocol(format!(
            "reply has neither result nor error: {reply}"
        ))),
    }
}

/// herdr agent names: `[a-z][a-z0-9_-]{0,31}`. Lowercases, maps other chars to `-`, truncates.
pub fn agent_name(raw: &str) -> String {
    let mut name: String = raw
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        name.insert(0, 'a');
    }
    name.truncate(32);
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_unwrapping() {
        assert_eq!(
            unwrap_reply(json!({"id":"1","result":{"type":"pong"}})).unwrap()["type"],
            "pong"
        );
        let err = unwrap_reply(json!({"id":"1","error":{"code":"not_found","message":"no pane"}}));
        assert!(matches!(err, Err(Error::Api { code, .. }) if code == "not_found"));
        assert!(matches!(
            unwrap_reply(json!({"id":"1"})),
            Err(Error::Protocol(_))
        ));
    }

    #[test]
    fn agent_names_are_valid() {
        assert_eq!(agent_name("SEO Chief"), "seo-chief");
        assert_eq!(agent_name("1st-bot"), "a1st-bot");
        assert_eq!(agent_name(&"x".repeat(40)).len(), 32);
    }
}
