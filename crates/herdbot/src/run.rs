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
    state.route = start(
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
    .await?
    .to_string();
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
    state.route = start(
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
    .await?
    .to_string();
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

/// Environment for a pane: who it belongs to, and the tool list for the pi extension.
fn pane_env(slug: &str, run: Option<&str>) -> Value {
    let mut env = json!({
        "HERDBOT_BOT": slug,
        "HERDBOT_TOOLS": tools::list(run.is_some()).to_string(),
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
        None => format!(
            "{}\n\n{}",
            tools::main_preamble(slug, bot),
            home::instructions(slug)
        ),
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
    )?;
    herdr
        .agent_start(agent, launch.kind, pane, &launch.args, 60_000)
        .await?;
    // One prompt only: a second prompt while the first turn runs steers it (verified with pi).
    let first = match (launch.preamble_prompt, task) {
        (Some(pre), Some(task)) => Some(format!("{pre}\n\n---\n\n{task}")),
        (Some(pre), None) => Some(format!(
            "{pre}\n\nThese are standing instructions, not a task. Reply READY and wait for messages."
        )),
        (None, task) => task.map(str::to_owned),
    };
    if let Some(text) = first {
        herdr.agent_prompt(agent, &text, None).await?;
    }
    Ok(route)
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
    };
    home::save_run(&run)?;
    let task = format!(
        "{prompt}\n\n---\nWhen you are done, call the herdbot `report_result` tool once with your complete answer; it is delivered to whoever started you."
    );
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
