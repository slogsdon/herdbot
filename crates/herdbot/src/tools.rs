//! The tools every bot agent gets, served over MCP to every harness (pi registers the server
//! from herdbot-pi.ts); `herdbot tool` calls one from a shell. Mirrors Grok Bot's Task / CheckSubagent / StopSubagent /
//! SendToAgent / WakeParent; the wake is automatic when a subagent reports.

use herdr_client::Client;
use serde_json::{Value, json};

use crate::home::{self, Bot, Result};
use crate::run;

pub struct Ctx {
    pub bot: String,
    pub run: Option<String>,
}

pub fn list(in_run: bool) -> Value {
    let mut tools = vec![
        json!({
            "name": "task",
            "description": "Start a subagent in its own pane. Returns a run id immediately; the result arrives later as a message starting with [subagent <id>]. Do not poll in a loop.",
            "inputSchema": { "type": "object", "required": ["description", "prompt"], "properties": {
                "description": { "type": "string", "description": "3-6 word label" },
                "prompt": { "type": "string", "description": "Complete, self-contained task. The subagent sees nothing else." },
                "subagent_type": { "type": "string", "description": "Routing class: explore, shell, plan, code, review, debug, research, media_review, browser_use, computer_use, custom. Default: default." },
                "model": { "type": "string", "description": "Optional harness:model[:effort] override, e.g. pi:kimi-k2.7-code:medium" },
                "done_when": { "type": "string", "description": "Optional completion criterion for this task." },
                "wall_clock_limit": { "type": "integer", "minimum": 1, "description": "Optional active-time limit in seconds. Time waiting for approval is paused." }
            }}
        }),
        json!({
            "name": "check_subagent",
            "description": "Status and result of a subagent run.",
            "inputSchema": { "type": "object", "required": ["run_id"], "properties": { "run_id": { "type": "string" } } }
        }),
        json!({
            "name": "stop_subagent",
            "description": "Stop a subagent run and close its pane.",
            "inputSchema": { "type": "object", "required": ["run_id"], "properties": { "run_id": { "type": "string" } } }
        }),
        json!({
            "name": "send_to_agent",
            "description": "Send a message to another bot. It is delivered when that bot is idle; replies arrive as [message from <bot>].",
            "inputSchema": { "type": "object", "required": ["bot", "message"], "properties": {
                "bot": { "type": "string", "description": "Bot name or slug (see list_bots)" },
                "message": { "type": "string" }
            }}
        }),
        json!({
            "name": "list_bots",
            "description": "Bots you can message, with their titles.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ];
    if in_run {
        tools.push(json!({
            "name": "report_result",
            "description": "Deliver your final answer to whoever started you. Call once, at the end.",
            "inputSchema": { "type": "object", "required": ["result"], "properties": { "result": { "type": "string" }, "summary": { "type": "string", "description": "Optional concise completion summary." } } }
        }));
        tools.push(json!({
            "name": "report_progress",
            "description": "Report a concise progress update. Updates are coalesced and shown in chat; this does not wake the parent agent.",
            "inputSchema": { "type": "object", "required": ["progress"], "properties": { "progress": { "type": "string", "description": "Current progress, blockers, or next step." } } }
        }));
    }
    if !in_run {
        tools.push(json!({
            "name": "room_post",
            "description": "Post to a room you belong to. Members you name with @slug are called on next.",
            "inputSchema": { "type": "object", "required": ["room", "text"], "properties": {
                "room": { "type": "string", "description": "Room slug, from the [room <slug> …] header or list_rooms" },
                "text": { "type": "string" }
            }}
        }));
        tools.push(json!({
            "name": "list_rooms",
            "description": "Rooms you belong to, with their mode and members.",
            "inputSchema": { "type": "object", "properties": {} }
        }));
    }
    Value::Array(tools)
}

/// The tools for this agent: `list`, plus write_node for compactor runs and zoom/date for the
/// main agent of a memory bot.
pub fn list_for(ctx: &Ctx) -> Value {
    let Value::Array(mut tools) = list(ctx.run.is_some()) else { unreachable!() };
    let class = ctx.run.as_deref().and_then(home::load_run).map(|r| r.class);
    if class.as_deref() == Some("compact") {
        tools.push(json!({
            "name": "write_node",
            "description": "Deliver the summary line for your current job.",
            "inputSchema": { "type": "object", "required": ["text"], "properties": { "text": { "type": "string" } } }
        }));
    }
    if ctx.run.is_none() && home::load_bot(&ctx.bot).is_ok_and(|b| b.memory) {
        tools.push(json!({
            "name": "zoom",
            "description": "Open the line id+n of your memory view into the two lines of n/2 under it; n = 1 gives the message whole.",
            "inputSchema": { "type": "object", "required": ["id", "n"], "properties": { "id": { "type": "integer" }, "n": { "type": "integer" } } }
        }));
        tools.push(json!({
            "name": "date",
            "description": "The date and time of message id in your memory.",
            "inputSchema": { "type": "object", "required": ["id"], "properties": { "id": { "type": "integer" } } }
        }));
    }
    Value::Array(tools)
}

pub fn main_preamble(slug: &str, bot: &Bot) -> String {
    format!(
        "You are the bot \"{name}\" ({title}), running under herdbot in your own herdr workspace.\n\
         - Delegate with the herdbot `task` tool: each subagent gets its own pane and a model routed to the task type. Results come back as messages starting with [subagent <id>].\n\
         - Talk to other bots with `send_to_agent`; their messages reach you as [message from <bot>]. `list_bots` lists them.\n\
         - Rooms you belong to send messages as [room <slug> …]. Answer with `room_post`; name others with @slug to call on them. `list_rooms` lists your rooms.\n\
         - Scheduled runs arrive as [scheduled: <name>].\n\
         - Your memory lives in {dir}/memory and your skills in {dir}/skills. Read and update them with your file tools.\n\
         Your standing instructions follow.",
        name = bot.name,
        title = bot.title,
        dir = home::bot_dir(slug).display(),
    )
}

pub fn sub_preamble(slug: &str, run: &str) -> String {
    format!(
        "You are herdbot subagent run {run}, working for the bot \"{slug}\". Do exactly the task you are given, then call `report_result` with your full answer. You may start further subagents with `task` if the work genuinely splits."
    )
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument {key:?}").into())
}

/// A main agent messaging itself would queue the prompt to its own pane and loop.
fn refuse_self(target: &str, ctx: &Ctx) -> Result<()> {
    if target == ctx.bot && ctx.run.is_none() {
        return Err(format!("send_to_agent: {target} is you; act on the task directly").into());
    }
    Ok(())
}

fn should_pick_class(subagent_type: Option<&str>, model: Option<&str>) -> bool {
    subagent_type.is_none() && model.is_none()
}

fn check_run_owner(owner: &str, ctx: &Ctx) -> Result<()> {
    if owner != ctx.bot {
        return Err(format!("run belongs to bot {owner:?}, not {:?}", ctx.bot).into());
    }
    Ok(())
}

/// Resolve a bot name or slug to an existing slug.
fn find_bot(name: &str) -> Result<String> {
    let wanted = home::slug(name);
    home::list_bots()
        .into_iter()
        .find(|s| {
            *s == wanted
                || home::load_bot(s)
                    .map(|b| b.name.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            format!(
                "no bot named {name:?}; known: {}",
                home::list_bots().join(", ")
            )
            .into()
        })
}

pub async fn call(ctx: &Ctx, name: &str, args: &Value) -> Result<String> {
    let herdr = Client::from_env();
    let me = match &ctx.run {
        Some(id) => home::load_run(id).ok_or("unknown run")?.agent,
        None => run::main_agent(&ctx.bot),
    };
    match name {
        "task" => {
            let named = args.get("subagent_type").and_then(Value::as_str);
            let model = args.get("model").and_then(Value::as_str);
            let picked = if should_pick_class(named, model) {
                crate::fm::pick_class(arg(args, "description")?, arg(args, "prompt")?).await
            } else {
                None
            };
            let class = named.or(picked).unwrap_or("default");
            let done_when = args.get("done_when").map(|value| {
                value.as_str().ok_or("done_when must be a string")
            }).transpose()?;
            let wall_clock_limit = match args.get("wall_clock_limit") {
                Some(value) => Some(value.as_u64().filter(|limit| *limit > 0)
                    .ok_or("wall_clock_limit must be a positive integer")?),
                None => None,
            };
            let run = run::spawn(
                &herdr,
                &ctx.bot,
                &me,
                crate::run::SpawnRequest {
                    class,
                    explicit_model: model,
                    description: arg(args, "description")?,
                    prompt: arg(args, "prompt")?,
                    done_when,
                    wall_clock_limit,
                },
            )
            .await?;
            Ok(match run.status.as_str() {
                "failed" => format!("run {} failed: {}", run.id, run.result.unwrap_or_default()),
                _ => match (&run.requested_route, &run.fallback_reason) {
                    (Some(requested), Some(reason)) => format!("started run {} on {} in pane {}. Requested route {} was unavailable: {}. Its result will arrive as a message.", run.id, run.route, run.pane, requested, reason),
                    _ => format!("started run {} on {} in pane {}. Its result will arrive as a message.", run.id, run.route, run.pane),
                },
            })
        }
        "check_subagent" => {
            let run = home::load_run(arg(args, "run_id")?).ok_or("unknown run id")?;
            check_run_owner(&run.bot, ctx)?;
            let live = run::status(&herdr, &run.agent)
                .await
                .unwrap_or_else(|| "gone".into());
            Ok(format!(
                "{} ({}, agent {live}): {}",
                run.status,
                run.route,
                run.result.as_deref().unwrap_or("no result yet")
            ))
        }
        "stop_subagent" => {
            let mut run = home::load_run(arg(args, "run_id")?).ok_or("unknown run id")?;
            check_run_owner(&run.bot, ctx)?;
            let _ = herdr.pane_close(&run.pane).await;
            run.pane_closed = true;
            if run.status == "running" {
                run.status = "failed".into();
                run.result = Some("stopped".into());
                run.events.push(home::RunEvent {
                    id: format!("run:{}:stopped", run.id),
                    at: home::now(),
                    kind: "stopped".into(),
                    summary: "Task stopped.".into(),
                    detail: None,
                });
            }
            home::save_run(&run)?;
            Ok(format!("stopped {}", run.id))
        }
        "send_to_agent" => {
            // Enqueue only; the daemon starts the target if it is down (one launcher, no races).
            let target = find_bot(arg(args, "bot")?)?;
            refuse_self(&target, ctx)?;
            home::enqueue_from(
                &run::main_agent(&target),
                "bot_message",
                &format!(
                    "[message from {}]\n{}\n\n(Reply with send_to_agent to \"{}\" if a reply is needed.)",
                    ctx.bot,
                    arg(args, "message")?,
                    ctx.bot
                ),
            )?;
            Ok(format!("queued for {target}; delivered when it is idle"))
        }
        "list_bots" => Ok(home::list_bots()
            .into_iter()
            .filter_map(|s| {
                home::load_bot(&s)
                    .ok()
                    .map(|b| format!("{s}: {} — {}", b.name, b.title))
            })
            .collect::<Vec<_>>()
            .join("\n")),
        "room_post" => {
            let slug = arg(args, "room")?;
            let room = crate::rooms::load_room(slug)?;
            crate::rooms::check_member(&room, &ctx.bot, ctx.run.is_some())?;
            crate::rooms::append(slug, &ctx.bot, arg(args, "text")?)?;
            Ok(format!("posted to {slug}"))
        }
        "list_rooms" => Ok(crate::rooms::list_rooms()
            .into_iter()
            .filter_map(|s| {
                crate::rooms::load_room(&s)
                    .ok()
                    .filter(|r| r.members.contains(&ctx.bot))
                    .map(|r| format!("{s}: {} ({:?}): {}", r.name, r.mode, r.members.join(", ")))
            })
            .collect::<Vec<_>>()
            .join("\n")),
        "report_result" => {
            let id = ctx
                .run
                .as_deref()
                .ok_or("report_result is only for subagent runs")?;
            let mut run = home::load_run(id).ok_or("unknown run")?;
            let result = arg(args, "result")?.to_owned();
            let summary = args.get("summary").and_then(Value::as_str).map(str::to_owned)
                .unwrap_or_else(|| home::first_line_preview(&result));
            run.status = "done".into();
            run.result = Some(result.clone());
            run.result_source = "report".into();
            run.summary = Some(summary.clone());
            run.events.push(home::RunEvent {
                id: format!("run:{}:completed", run.id), at: home::now(), kind: "completed".into(),
                summary: summary.clone(), detail: Some(result.clone()),
            });
            let notice = format!(
                    "[subagent {} \"{}\" finished]\nSummary: {summary}\n\nFull result:\n{result}",
                    run.id, run.description
                );
            let notice_id = format!("run:{}:completed", run.id);
            home::enqueue_run_notice(&mut run, &notice_id, &notice)?;
            Ok("delivered. You can stop now.".into())
        }
        "report_progress" => {
            let id = ctx.run.as_deref().ok_or("report_progress is only for subagent runs")?;
            let mut run = home::load_run(id).ok_or("unknown run")?;
            home::record_progress(&mut run, arg(args, "progress")?, home::now());
            home::save_run(&run)?;
            Ok("progress recorded".into())
        }
        "write_node" => {
            let id = ctx.run.as_deref().ok_or("write_node is for compactor runs")?;
            let run = home::load_run(id).ok_or("unknown run")?;
            if run.class != "compact" {
                return Err("write_node is for compactor runs".into());
            }
            crate::memory::push_inbox(&run.bot, id, arg(args, "text")?)?;
            Ok("received".into())
        }
        "zoom" | "date" => {
            let bot = home::load_bot(&ctx.bot)?;
            if ctx.run.is_some() || !bot.memory {
                return Err(format!("{name} is for a memory bot's main agent").into());
            }
            let mem = crate::memory::load(&ctx.bot, crate::memory::budget(&bot), false);
            let num = |key: &str| args.get(key).and_then(Value::as_u64).map(|v| v as usize).ok_or(format!("missing integer argument {key:?}"));
            Ok(if name == "zoom" { mem.zoom(num("id")?, num("n")?) } else { mem.date(num("id")?) })
        }
        other => Err(format!("unknown tool {other}").into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_self_only_for_a_main_agent_naming_itself() {
        let main = Ctx { bot: "alpha".into(), run: None };
        let sub = Ctx { bot: "alpha".into(), run: Some("run1".into()) };
        assert_eq!(
            refuse_self("alpha", &main).unwrap_err().to_string(),
            "send_to_agent: alpha is you; act on the task directly"
        );
        assert!(refuse_self("alpha", &sub).is_ok());
        assert!(refuse_self("beta", &main).is_ok());
    }

    #[test]
    fn explicit_model_skips_class_picker_without_named_type() {
        assert!(!should_pick_class(None, Some("pi:deepseek-v4-flash:low")));
        assert!(!should_pick_class(Some("code"), None));
        assert!(should_pick_class(None, None));
    }

    #[test]
    fn run_ownership_is_checked_before_herdr_access() {
        let ctx = Ctx { bot: "alpha".into(), run: None };
        assert!(check_run_owner("alpha", &ctx).is_ok());
        assert_eq!(
            check_run_owner("beta", &ctx).unwrap_err().to_string(),
            "run belongs to bot \"beta\", not \"alpha\""
        );
    }

    #[test]
    fn lifecycle_tool_schemas_are_optional_and_subagent_only() {
        let names = |in_run| list(in_run).as_array().unwrap().iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
        let main_names = names(false);
        assert!(!main_names.iter().any(|name| name == "report_progress"));
        let run_tools = list(true);
        let by_name = |name: &str| run_tools.as_array().unwrap().iter().find(|tool| tool["name"] == name).unwrap();
        for field in ["done_when", "wall_clock_limit"] {
            assert!(by_name("task")["inputSchema"]["properties"].get(field).is_some());
            assert!(!by_name("task")["inputSchema"]["required"].as_array().unwrap().iter().any(|v| v == field));
        }
        assert!(by_name("report_result")["inputSchema"]["properties"].get("summary").is_some());
        assert!(by_name("report_progress")["inputSchema"]["properties"].get("progress").is_some());
    }
}
