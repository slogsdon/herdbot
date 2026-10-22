//! herdr side of a bot: its workspace, its main agent, and subagent panes.

use herdr_client::{Client, Error as HerdrError};
use serde_json::{Value, json};

use crate::home::{self, Bot, BotState, Result, Run};
use crate::route;
use crate::tools;

/// The bot's main agent is named after its slug.
pub fn main_agent(slug: &str) -> String {
    slug.to_owned()
}

fn marks_launch_ready(has_preamble: bool, is_main_agent: bool) -> bool {
    has_preamble && is_main_agent
}

async fn agent_alive(herdr: &Client, name: &str) -> bool {
    herdr.agent_get(name).await.is_ok()
}

/// Make sure the bot has a workspace with its main agent running; return its ids.
pub async fn up(herdr: &Client, slug: &str) -> Result<BotState> {
    let bot = home::load_bot(slug)?;
    if let Some(state) = home::load_state(slug) {
        let workspace_alive = herdr
            .call(
                "workspace.get",
                json!({ "workspace_id": state.workspace_id }),
            )
            .await
            .is_ok();
        if workspace_alive && agent_alive(herdr, &state.agent).await {
            return Ok(state);
        }
        if workspace_alive {
            // Agent exited; restart it in a fresh pane of the same workspace.
            return restart(herdr, slug, state).await;
        }
        home::clear_state(slug);
    }
    let cwd = home::cwd(slug, &bot);
    let created = herdr
        .workspace_create(&format!("🤖 {}", bot.name), &cwd, pane_env(slug, None))
        .await?;
    let mut state = BotState {
        workspace_id: created["workspace"]["workspace_id"]
            .as_str()
            .ok_or("workspace.create: no id")?
            .to_owned(),
        root_pane: created["root_pane"]["pane_id"]
            .as_str()
            .ok_or("workspace.create: no pane")?
            .to_owned(),
        agent: main_agent(slug),
        route: String::new(),
    };
    home::save_state(slug, &state)?;
    let started = start(
        herdr,
        slug,
        &bot,
        &state.agent,
        &state.root_pane,
        bot.class.as_deref().unwrap_or("default"),
        None,
        None,
        None,
    )
    .await;
    state.route = match started {
        Ok(route) => route.to_string(),
        Err(e) => {
            // No recorded route would hide the agent from quota failover. Close the
            // workspace instead; the next queued message brings the bot back up.
            let _ = herdr.workspace_close(&state.workspace_id).await;
            home::clear_state(slug);
            return Err(e);
        }
    };
    home::save_state(slug, &state)?;
    Ok(state)
}

/// Replace the bot's main agent with a freshly routed one in a new pane of the same
/// workspace, so a harness that is cooling down gets skipped. Routing happens before
/// anything is touched: with no route available the old agent keeps running.
pub async fn restart(herdr: &Client, slug: &str, state: BotState) -> Result<BotState> {
    let bot = home::load_bot(slug)?;
    let class = bot.class.as_deref().unwrap_or("default");
    route::resolve(&route::table()?, class, slug, None)?;
    let pane = split(herdr, &state.root_pane, slug, None, &home::cwd(slug, &bot)).await?;
    // Closing the pane ends its agent and frees the name for reuse (checked on herdr,
    // 2026-10-20).
    let _ = herdr.pane_close(&state.root_pane).await;
    let mut state = BotState {
        root_pane: pane,
        ..state
    };
    // Save the new pane first: if the start fails, the next `up` splits from a live pane.
    home::save_state(slug, &state)?;
    let started = start(
        herdr,
        slug,
        &bot,
        &state.agent,
        &state.root_pane,
        class,
        None,
        None,
        None,
    )
    .await;
    state.route = match started {
        Ok(route) => route.to_string(),
        Err(e) => {
            // No recorded route would hide the agent from quota failover. Close the
            // workspace instead; the next queued message brings the bot back up.
            let _ = herdr.workspace_close(&state.workspace_id).await;
            home::clear_state(slug);
            return Err(e);
        }
    };
    home::save_state(slug, &state)?;
    Ok(state)
}

pub async fn down(herdr: &Client, slug: &str) -> Result<()> {
    if let Some(state) = home::load_state(slug) {
        let _ = herdr.workspace_close(&state.workspace_id).await;
    }
    home::clear_state(slug);
    Ok(())
}

/// Environment for a pane: bot/run identity and the MCP server executable and home.
fn pane_env(slug: &str, run: Option<&str>) -> Value {
    let mut env = json!({
        "HERDBOT_BOT": slug,
        "HERDBOT_EXE": std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        "HERDBOT_HOME": home::root().to_string_lossy(),
    });
    if let Some(run) = run {
        env["HERDBOT_RUN"] = json!(run);
    }
    env
}

async fn split(
    herdr: &Client,
    target: &str,
    slug: &str,
    run: Option<&str>,
    cwd: &str,
) -> Result<String> {
    let split = herdr
        .call(
            "pane.split",
            json!({ "target_pane_id": target, "direction": "right", "cwd": cwd, "env": pane_env(slug, run), "focus": false }),
        )
        .await?;
    Ok(split["pane"]["pane_id"]
        .as_str()
        .ok_or("pane.split: no pane id")?
        .to_owned())
}

/// Start an agent in `pane`. For a subagent, `task` is its prompt and `run` its id.
#[allow(clippy::too_many_arguments)]
async fn start(
    herdr: &Client,
    slug: &str,
    bot: &Bot,
    agent: &str,
    pane: &str,
    class: &str,
    explicit_model: Option<&str>,
    run: Option<&str>,
    task: Option<&str>,
) -> Result<route::Route> {
    let table = route::table()?;
    let route = route::resolve(&table, class, slug, explicit_model)?;
    let instructions = match run {
        None => {
            let mut text = format!("{}\n\n{}", tools::main_preamble(slug, bot), home::instructions(slug, bot));
            if let Some(view) = crate::memory::view_block(slug, bot) {
                text.push_str("\n\n");
                text.push_str(&view);
            }
            text
        }
        Some(_) if class == "compact" => crate::memory::compact_preamble(&bot.name),
        Some(run) => tools::sub_preamble(slug, run),
    };
    let mut tool_args = vec!["--bot".to_owned(), slug.to_owned()];
    if let Some(run) = run {
        tool_args.extend(["--run".to_owned(), run.to_owned()]);
    }
    let launch = route::launch(
        &table,
        &route,
        agent,
        &instructions,
        &tool_args,
        &home::cwd(slug, bot),
        home::approvals(bot),
        home::codex_unsandboxed(),
        class,
    )?;
    herdr
        .agent_start(agent, launch.kind, pane, &launch.args, 60_000)
        .await?;
    // The launch turn's READY is not conversation: `consume_events` drops the turn end this
    // marker marks. Every launch clears an older mark first, so a relaunch without a READY
    // prompt must not swallow a real turn.
    let ready = home::root().join(".state").join("launch").join(format!("{agent}.ready"));
    let _ = std::fs::remove_file(&ready);
    if marks_launch_ready(launch.preamble_prompt.is_some(), task.is_none()) {
        let _ = std::fs::write(&ready, home::now().to_string());
    }
    // One prompt only: a second prompt while the first turn runs steers it (verified with pi).
    let first = match (launch.preamble_prompt, task) {
        (Some(pre), Some(task)) => Some(format!("{pre}\n\n---\n\n{task}")),
        (Some(pre), None) => Some(format!(
            "{pre}\n\nThese are standing instructions, not a task. Reply READY and wait for messages."
        )),
        (None, task) => task.map(str::to_owned),
    };
    if let Some(text) = first {
        // A ready agent can still drop its first prompt (seen once with codex: splash
        // screen, empty input, no turn). An idle agent has no turn to steer, so one resend
        // is safe; past that, fail instead of letting the daemon harvest the splash.
        for _ in 0..2 {
            let before = tail(herdr, agent, 40).await;
            herdr.agent_prompt(agent, &text, None).await?;
            if took_prompt(herdr, agent, &before).await {
                return Ok(route);
            }
            eprintln!("{agent}: first prompt not picked up; sending again");
        }
        return Err(format!("{agent} did not start a turn after two prompts").into());
    }
    Ok(route)
}

/// Whether a prompt was picked up: the agent left `idle`, or its screen changed.
///
/// herdr reads pi's turn state off a "Working" spinner line, which a narrow pane truncates
/// to the bare spinner glyph, so a pane squeezed by sibling splits stays `idle` through a
/// whole turn. A dropped prompt leaves the screen untouched.
fn prompt_taken(status: Option<&str>, before: &str, after: &str) -> bool {
    !matches!(status, Some("idle") | None) || before != after
}

/// Whether `agent` takes the prompt within 15 s.
async fn took_prompt(herdr: &Client, agent: &str, before: &str) -> bool {
    for _ in 0..15 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let status = status(herdr, agent).await;
        if prompt_taken(status.as_deref(), before, &tail(herdr, agent, 40).await) {
            return true;
        }
    }
    false
}

/// Open a pane next to `parent_agent`'s bot, start a routed agent in it, hand it the task.
pub async fn spawn(
    herdr: &Client,
    slug: &str,
    parent_agent: &str,
    class: &str,
    explicit_model: Option<&str>,
    description: &str,
    prompt: &str,
) -> Result<Run> {
    let bot = home::load_bot(slug)?;
    let state = up(herdr, slug).await?;
    let id = home::run_id();
    let agent = format!("{slug}-{id}");
    let pane = split(
        herdr,
        &state.root_pane,
        slug,
        Some(&id),
        &home::cwd(slug, &bot),
    )
    .await?;
    let mut run = Run {
        id: id.clone(),
        bot: slug.to_owned(),
        parent_agent: parent_agent.to_owned(),
        agent: agent.clone(),
        pane: pane.clone(),
        class: class.to_owned(),
        route: String::new(),
        description: description.to_owned(),
        status: "running".into(),
        result: None,
        started_at: home::now(),
        pane_closed: false,
        notified_blocked: false,
        prompt: prompt.to_owned(),
        result_source: String::new(),
        settled_at: None,
    };
    home::save_run(&run)?;
    let task = if class == "compact" { prompt.to_owned() } else { format!(
        "{prompt}\n\n---\nWhen you are done, call the herdbot `report_result` tool once with your complete answer; it is delivered to whoever started you."
    ) };
    match start(
        herdr,
        slug,
        &bot,
        &agent,
        &pane,
        class,
        explicit_model,
        Some(&id),
        Some(&task),
    )
    .await
    {
        Ok(route) => run.route = route.to_string(),
        Err(e) => {
            run.status = "failed".into();
            run.result = Some(format!("failed to start: {e}"));
            let _ = herdr.pane_close(&pane).await;
            run.pane_closed = true;
        }
    }
    home::save_run(&run)?;
    Ok(run)
}

/// `idle`, `working`, `blocked`, `done`, `unknown`, or `None` if the agent is gone.
pub async fn status(herdr: &Client, agent: &str) -> Option<String> {
    match herdr.agent_get(agent).await {
        Ok(v) => v["agent"]["agent_status"].as_str().map(str::to_owned),
        Err(HerdrError::Api { .. }) => None,
        Err(_) => Some("unknown".into()),
    }
}

/// Last lines an agent printed, for runs that finish without calling report_result.
pub async fn tail(herdr: &Client, agent: &str, lines: u32) -> String {
    herdr
        .call(
            "agent.read",
            json!({ "target": agent, "source": "recent_unwrapped", "lines": lines }),
        )
        .await
        .map(|v| {
            v["read"]["text"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{marks_launch_ready, prompt_taken};

    #[test]
    fn launch_ready_marker_requires_ready_prompt_for_main_agent() {
        assert!(marks_launch_ready(true, true)); // includes non-memory bots
        assert!(!marks_launch_ready(false, true));
        assert!(!marks_launch_ready(true, false));
    }

    #[test]
    fn prompt_taken_when_status_leaves_idle() {
        assert!(prompt_taken(Some("working"), "screen", "screen"));
    }

    #[test]
    fn prompt_taken_when_idle_status_hides_a_changed_screen() {
        // narrow pane: herdr never sees the "Working" spinner, but the turn rewrites the screen
        assert!(prompt_taken(Some("idle"), "ready", "ready\nCall list_bots"));
    }

    #[test]
    fn prompt_dropped_when_idle_and_screen_unchanged() {
        assert!(!prompt_taken(Some("idle"), "splash", "splash"));
        assert!(!prompt_taken(None, "splash", "splash"));
    }
}
