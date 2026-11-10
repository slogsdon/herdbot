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

#[derive(Debug, PartialEq, Eq)]
enum LaunchIdentity {
    Keep,
    Recover,
    Defer,
}

fn launch_identity(
    recorded: Option<&str>,
    current: Option<&str>,
    status: Option<&str>,
) -> LaunchIdentity {
    if current.is_some() && recorded == current {
        return LaunchIdentity::Keep;
    }
    if current.is_some() && matches!(status, Some("idle" | "done")) {
        LaunchIdentity::Recover
    } else {
        LaunchIdentity::Defer
    }
}

fn resume_args(harness: &str, session: Option<(&str, &str)>) -> Vec<String> {
    let Some((kind, value)) = session.filter(|(kind, value)| *kind == harness && !value.is_empty())
    else {
        return Vec::new();
    };
    let flag = match kind {
        "codex" => "resume",
        "claude" => "--resume",
        "pi" => "--session",
        _ => return Vec::new(),
    };
    vec![flag.into(), value.into()]
}

/// Make sure the bot has a workspace with its main agent running; return its ids.
pub async fn up(herdr: &Client, slug: &str) -> Result<BotState> {
    let bot = home::load_bot(slug)?;
    if let Some(state) = home::load_state(slug) {
        let workspace_alive = match herdr.call(
            "workspace.get",
            json!({ "workspace_id": state.workspace_id }),
        ).await {
            Ok(_) => true,
            Err(HerdrError::Api { code, .. }) if code == "workspace_not_found" => false,
            Err(error) => return Err(error.into()),
        };
        if workspace_alive {
            let found = match herdr.agent_get(&state.agent).await {
                Ok(found) => found,
                Err(HerdrError::Api { code, .. }) if code == "agent_not_found" => {
                    return restart(herdr, slug, state).await;
                }
                Err(error) => return Err(error.into()),
            };
            let agent = &found["agent"];
            match launch_identity(
                state.terminal_id.as_deref(),
                agent["terminal_id"].as_str(),
                agent["agent_status"].as_str(),
            ) {
                LaunchIdentity::Keep | LaunchIdentity::Defer => return Ok(state),
                LaunchIdentity::Recover => {
                    let old_terminal = state.terminal_id.clone();
                    let recovered = restart_with_session(herdr, slug, state, Some(agent)).await?;
                    if recovered.terminal_id != old_terminal {
                        println!("recovery {slug}: restored terminal {} (recorded {:?}); relaunched with herdbot wiring", agent["terminal_id"], old_terminal);
                    }
                    return Ok(recovered);
                }
            }
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
        terminal_id: None,
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
        None,
    )
    .await;
    state.route = match started {
        Ok(route) => route.to_string(),
        Err(e) => {
            if let Some(blocked) = e.downcast_ref::<StartBlocked>() {
                herdbot_core::cards::write_card(&blocked.card)?;
                blocked.route.to_string()
            } else {
                // No recorded route would hide the agent from quota failover. Close the
                // workspace instead; the next queued message brings the bot back up.
                let _ = herdr.workspace_close(&state.workspace_id).await;
                home::clear_state(slug);
                return Err(e);
            }
        }
    };
    state.terminal_id = home::load_state(slug).and_then(|saved| saved.terminal_id);
    home::save_state(slug, &state)?;
    Ok(state)
}

/// Replace the bot's main agent with a freshly routed one in a new pane of the same
/// workspace, so a harness that is cooling down gets skipped. Routing happens before
/// anything is touched: with no route available the old agent keeps running.
pub async fn restart(herdr: &Client, slug: &str, state: BotState) -> Result<BotState> {
    restart_with_session(herdr, slug, state, None).await
}

async fn restart_with_session(
    herdr: &Client,
    slug: &str,
    state: BotState,
    restored: Option<&Value>,
) -> Result<BotState> {
    let bot = home::load_bot(slug)?;
    let class = bot.class.as_deref().unwrap_or("default");
    let resume = restored.and_then(|agent| {
        Some((
            agent["agent_session"]["agent"]
                .as_str()
                .or_else(|| agent["agent"].as_str())?,
            agent["agent_session"]["value"].as_str()?,
        ))
    });
    let explicit = resume
        .filter(|(kind, _)| state.route.split(':').next() == Some(*kind))
        .map(|_| state.route.clone());
    route::resolve(&route::table()?, class, slug, explicit.as_deref())?;
    let pane = split(herdr, &state.root_pane, slug, None, &home::cwd(slug, &bot)).await?;
    if restored.is_some()
        && !matches!(
            status(herdr, &state.agent).await.as_deref(),
            Some("idle" | "done")
        )
    {
        let _ = herdr.pane_close(&pane).await;
        return Ok(state);
    }
    // Closing the pane ends its agent and frees the name for reuse (checked on herdr,
    // 2026-10-20).
    let _ = herdr.pane_close(&state.root_pane).await;
    let mut state = BotState {
        root_pane: pane,
        terminal_id: None,
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
        explicit.as_deref(),
        None,
        None,
        resume,
    )
    .await;
    state.route = match started {
        Ok(route) => route.to_string(),
        Err(e) => {
            if let Some(blocked) = e.downcast_ref::<StartBlocked>() {
                herdbot_core::cards::write_card(&blocked.card)?;
                blocked.route.to_string()
            } else {
                // No recorded route would hide the agent from quota failover. Close the
                // workspace instead; the next queued message brings the bot back up.
                let _ = herdr.workspace_close(&state.workspace_id).await;
                home::clear_state(slug);
                return Err(e);
            }
        }
    };
    state.terminal_id = home::load_state(slug).and_then(|saved| saved.terminal_id);
    home::save_state(slug, &state)?;
    Ok(state)
}

pub async fn down(herdr: &Client, slug: &str) -> Result<()> {
    if let Some(state) = home::load_state(slug) {
        let _ = herdr.workspace_close(&state.workspace_id).await;
    }
    home::clear_state(slug);
    herdbot_core::cards::remove_card(slug);
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
    resume: Option<(&str, &str)>,
) -> Result<route::Route> {
    let table = route::table()?;
    let route = route::resolve(&table, class, slug, explicit_model)?;
    // Keep routing evidence even if launching or delivering the first prompt fails.
    if let Some(id) = run {
        let mut record = home::load_run(id).ok_or("unknown run")?;
        record.route = format!("{}:{}{}", route.harness, route.model, route.effort.as_ref().map(|effort| format!(":{effort}")).unwrap_or_default());
        record.requested_route = requested_route(explicit_model, &route);
        record.fallback_reason = route.fallback_reason.clone();
        home::save_run(&record)?;
    }
    let instructions = match run {
        None => {
            let mut text = format!(
                "{}\n\n{}",
                tools::main_preamble(slug, bot),
                home::instructions(slug, bot)
            );
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
    let mut launch = route::launch(
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
    launch.args.extend(resume_args(launch.kind, resume));
    let mut started = herdr
        .agent_start(agent, launch.kind, pane, &launch.args, 60_000)
        .await;
    let resumed_screen = if launch.kind == "codex" && resume.is_some_and(|(kind, _)| kind == "codex") {
        let screen = retry_codex_resume(herdr, agent).await;
        if let Some(screen) = &screen
            && started.is_err() && !crate::adapters::codex_conversation_open(screen)
        {
            let found = herdr.agent_get(agent).await?;
            if found["agent"]["interactive_ready"] == true && found["agent"]["launch_pending"] != true {
                started = Ok(found["agent"].clone());
            }
        }
        screen
    } else {
        None
    };
    if let Err(HerdrError::Api { code, .. }) = &started {
        let screen = tail(herdr, agent, 60).await;
        if let Some(mut card) = blocked_card(launch.kind, code, &screen, slug, agent) {
            card.harness = Some(route.harness.clone());
            card.run_id = run.map(str::to_owned);
            if run.is_none()
                && let Ok(found) = herdr.agent_get(agent).await
                && let Some(terminal_id) = found["agent"]["terminal_id"].as_str()
                && let Some(mut state) = home::load_state(slug)
            {
                state.terminal_id = Some(terminal_id.to_owned());
                if let Err(error) = home::save_state(slug, &state) {
                    eprintln!("{slug}: could not record blocked agent terminal ID: {error}");
                }
            }
            return Err(Box::new(StartBlocked { route, card }));
        }
    }
    let started = started?;
    if run.is_none() {
        let terminal_id = started["terminal_id"].as_str().ok_or("agent.start: no terminal_id")?;
        let mut state = home::load_state(slug).ok_or("main agent has no bot state")?;
        state.terminal_id = Some(terminal_id.to_owned());
        home::save_state(slug, &state)?;
    }
    if let Some(screen) = resumed_screen
        && let Some(card) = crate::adapters::recognize_prompt(&screen, &route.harness, slug, agent, run)
    {
        return Err(Box::new(StartBlocked { route, card }));
    }
    // The launch turn's READY is not conversation: `consume_events` drops the turn end this
    // marker marks. Every launch clears an older mark first, so a relaunch without a READY
    // prompt must not swallow a real turn.
    let ready = home::root()
        .join(".state")
        .join("launch")
        .join(format!("{agent}.ready"));
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
    if let Some(text) = first
        && let Some(card) = deliver_prompt(herdr, &route.harness, slug, agent, run, &text).await?
    {
        return Err(Box::new(StartBlocked { route, card }));
    }
    Ok(route)
}

/// Retry only the resume lock; never fork or exit a conversation automatically.
async fn retry_codex_resume(herdr: &Client, agent: &str) -> Option<String> {
    let mut screen = tail(herdr, agent, 60).await;
    if !crate::adapters::codex_conversation_open(&screen) {
        return None;
    }
    for _ in 0..3 {
        if !crate::adapters::codex_conversation_open(&screen) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        screen = tail(herdr, agent, 60).await;
        if !crate::adapters::codex_conversation_open(&screen) {
            break;
        }
        if let Err(error) = herdr.agent_send_keys(agent, &["r"]).await {
            eprintln!("{agent}: could not retry Codex resume: {error}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        screen = tail(herdr, agent, 60).await;
    }
    Some(screen)
}

/// A successful socket write is not acceptance. Leave queued batches intact on a card.
pub(crate) async fn deliver_prompt(
    herdr: &Client, harness: &str, bot: &str, agent: &str, run: Option<&str>, text: &str,
) -> Result<Option<herdbot_core::cards::Card>> {
    for _ in 0..2 {
        let before = tail(herdr, agent, 60).await;
        if let Some(card) = crate::adapters::recognize_prompt(&before, harness, bot, agent, run) {
            return Ok(Some(card));
        }
        let events = crate::hook::events_dir().join(format!("{agent}.jsonl"));
        let offset = std::fs::metadata(&events).map(|meta| meta.len() as usize).unwrap_or(0);
        if let Err(error) = herdr.agent_prompt(agent, text, None).await {
            let screen = tail(herdr, agent, 60).await;
            if let Some(card) = crate::adapters::recognize_prompt(&screen, harness, bot, agent, run) {
                return Ok(Some(card));
            }
            return Err(Box::new(error));
        }
        match wait_for_prompt(herdr, harness, bot, agent, run, &before, (&events, offset)).await {
            PromptWait::Accepted => return Ok(None),
            PromptWait::Blocked(card) => return Ok(Some(*card)),
            PromptWait::Idle => {}
        }
        eprintln!("{agent}: prompt not picked up; sending again");
    }
    let screen = tail(herdr, agent, 60).await;
    Ok(Some(crate::adapters::input_timeout_card(&screen, harness, bot, agent, run)))
}

/// Whether a prompt was picked up: the agent left `idle`, or Pi's screen changed.
///
/// herdr reads pi's turn state off a "Working" spinner line, which a narrow pane truncates
/// to the bare spinner glyph, so a pane squeezed by sibling splits stays `idle` through a
/// whole turn. A dropped prompt leaves the screen untouched.
fn prompt_taken(harness: &str, status: Option<&str>, before: &str, after: &str) -> bool {
    !matches!(status, Some("idle") | None) || (harness == "pi" && before != after)
}

enum PromptWait {
    Accepted,
    Blocked(Box<herdbot_core::cards::Card>),
    Idle,
}

/// Wait up to 15 seconds for accepted input or a screen that needs the operator.
async fn wait_for_prompt(herdr: &Client, harness: &str, bot: &str, agent: &str, run_id: Option<&str>, before: &str, events: (&std::path::Path, usize)) -> PromptWait {
    for _ in 0..15 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let status = status(herdr, agent).await;
        let after = tail(herdr, agent, 60).await;
        if let Some(card) = crate::adapters::recognize_prompt(&after, harness, bot, agent, run_id) {
            return PromptWait::Blocked(Box::new(card));
        }
        if status.as_deref() == Some("blocked") {
            return PromptWait::Blocked(Box::new(crate::adapters::blocked_status_card(&after, harness, bot, agent, run_id)));
        }
        // A fast hooked turn can finish between polls and already be idle again.
        let completed = std::fs::read_to_string(events.0).unwrap_or_default();
        let completed = completed.get(events.1..).unwrap_or_default().lines()
            .filter_map(|line| serde_json::from_str::<crate::hook::Event>(line).ok())
            .any(|event| event.agent == agent && matches!(event.kind.as_str(), "turn_end" | "turn_error"));
        if completed || prompt_taken(harness, status.as_deref(), before, &after) {
            return PromptWait::Accepted;
        }
    }
    PromptWait::Idle
}

/// Open a pane next to `parent_agent`'s bot, start a routed agent in it, hand it the task.
pub struct SpawnRequest<'a> {
    pub class: &'a str,
    pub explicit_model: Option<&'a str>,
    pub description: &'a str,
    pub prompt: &'a str,
    pub done_when: Option<&'a str>,
    pub wall_clock_limit: Option<u64>,
}

pub async fn spawn(
    herdr: &Client,
    slug: &str,
    parent_agent: &str,
    request: SpawnRequest<'_>,
) -> Result<Run> {
    let SpawnRequest { class, explicit_model, description, prompt, done_when, wall_clock_limit } = request;
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
    let started_at = home::now();
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
        started_at,
        pane_closed: false,
        notified_blocked: false,
        prompt: prompt.to_owned(),
        result_source: String::new(),
        settled_at: None,
        outbox: Vec::new(),
        event_ids: Vec::new(),
        requested_route: None,
        fallback_reason: None,
        done_when: done_when.map(str::to_owned),
        wall_clock_limit,
        active_seconds: wall_clock_limit.map(|_| 0),
        budget_checked_at: None,
        events: vec![home::RunEvent {
            id: format!("run:{id}:started"), at: started_at, kind: "started".into(),
            summary: format!("Started: {description}"), detail: Some(prompt.to_owned()),
        }],
        ..Run::default()
    };
    home::save_run(&run)?;
    let task = if class == "compact" {
        prompt.to_owned()
    } else {
        format!(
            "{prompt}\n\n---\nWhen you are done, call the herdbot `report_result` tool once with your complete answer; it is delivered to whoever started you."
        )
    };
    let started = start(
        herdr,
        slug,
        &bot,
        &agent,
        &pane,
        class,
        explicit_model,
        Some(&id),
        Some(&task),
        None,
    )
    .await;
    run = home::load_run(&id).unwrap_or(run);
    match started {
        Ok(_) => {
            run.budget_checked_at = wall_clock_limit.map(|_| home::now());
        }
        Err(e) => {
            if let Some(blocked) = e.downcast_ref::<StartBlocked>() {
                run.status = "blocked".into();
                run.route = format!("{}:{}{}", blocked.route.harness, blocked.route.model, blocked.route.effort.as_ref().map(|effort| format!(":{effort}")).unwrap_or_default());
                run.notified_blocked = true;
                run.blocked_since = Some(home::now());
                run.budget_checked_at = wall_clock_limit.map(|_| home::now());
                let mut card = blocked.card.clone();
                card.harness = Some(blocked.route.harness.clone());
                card.run_id = Some(run.id.clone());
                run.events.push(home::RunEvent {
                    id: format!("run:{}:blocked", run.id), at: home::now(), kind: "blocked".into(),
                    summary: card.title.clone(), detail: Some(card.body.clone()),
                });
                herdbot_core::cards::write_card(&card)?;
            } else {
                run.status = "failed".into();
                run.result = Some(format!("failed to start: {e}"));
                run.events.push(home::RunEvent {
                    id: format!("run:{id}:failed"), at: home::now(), kind: "failed".into(),
                    summary: "Task failed to start.".into(),
                    detail: run.result.clone(),
                });
                let _ = herdr.pane_close(&pane).await;
                run.pane_closed = true;
            }
        }
    }
    home::save_run(&run)?;
    if let (Some(requested), Some(reason)) = (&run.requested_route, &run.fallback_reason) {
        let notice = format!("Explicit route {requested} was unavailable; using {}. Reason: {reason}", run.route);
        let notice_id = format!("run:{}:route-fallback", run.id);
        home::enqueue_run_notice(&mut run, &notice_id, &notice)?;
    }
    Ok(run)
}

fn requested_route(explicit_model: Option<&str>, route: &crate::route::Route) -> Option<String> {
    route.requested.clone().or_else(|| explicit_model.map(str::to_owned))
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

/// `start` stopped on a prompt a card can show; the agent is alive and waiting for the user.
#[derive(Debug)]
pub struct StartBlocked {
    pub route: route::Route,
    pub card: herdbot_core::cards::Card,
}
impl std::fmt::Display for StartBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} is waiting for you: {}",
            self.card.agent, self.card.title
        )
    }
}
impl std::error::Error for StartBlocked {}

fn blocked_card(
    harness: &str,
    error_code: &str,
    screen: &str,
    bot: &str,
    agent: &str,
) -> Option<herdbot_core::cards::Card> {
    if harness == "codex" && crate::adapters::codex_conversation_open(screen) {
        return crate::adapters::recognize_prompt(screen, harness, bot, agent, None);
    }
    if !error_code.contains("blocked") && !error_code.contains("approval") {
        return None;
    }
    let dialog = crate::adapters::recognize_prompt(screen, harness, bot, agent, None);
    Some(dialog.unwrap_or_else(|| crate::adapters::blocked_status_card(screen, harness, bot, agent, None)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{marks_launch_ready, prompt_taken, requested_route};

    pub(crate) async fn scripted_herdr(script: Vec<(&'static str, serde_json::Value)>) -> (herdr_client::Client, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("ga1a7p-{}-{}.sock", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let client = herdr_client::Client::new(&path);
        let server = tokio::spawn(async move {
            for (method, result) in script {
                let (stream, _) = tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
                let (read, mut write) = stream.into_split();
                let line = BufReader::new(read).lines().next_line().await.unwrap().unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], method);
                if method == "agent.send_keys" {
                    assert_eq!(request["params"]["keys"], serde_json::json!(["r"]));
                }
                let response = serde_json::json!({"id": request["id"], "result": result});
                write.write_all(format!("{response}\n").as_bytes()).await.unwrap();
            }
            std::fs::remove_file(path).unwrap();
        });
        (client, server)
    }

    #[tokio::test]
    async fn codex_resume_retries_only_the_lock_and_stops_when_clear() {
        use serde_json::json;
        let modal = include_str!("../fixtures/panes/codex-conversation-open.txt").trim();
        for attempts in [0, 2, 3] {
            let mut script = vec![("agent.read", json!({"read": {"text": if attempts == 0 { "ready" } else { modal }}}))];
            for attempt in 0..attempts {
                script.push(("agent.read", json!({"read": {"text": modal}})));
                script.push(("agent.send_keys", json!({})));
                script.push(("agent.read", json!({"read": {"text": if attempts == 2 && attempt == 1 { "ready" } else { modal }}})));
            }
            let (client, server) = scripted_herdr(script).await;
            let screen = super::retry_codex_resume(&client, "test").await;
            assert_eq!(screen.as_deref(), match attempts { 0 => None, 3 => Some(modal), _ => Some("ready") });
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn codex_resume_does_not_type_r_if_the_lock_clears_during_delay() {
        use serde_json::json;
        let modal = include_str!("../fixtures/panes/codex-conversation-open.txt");
        let (client, server) = scripted_herdr(vec![
            ("agent.read", json!({"read": {"text": modal}})),
            ("agent.read", json!({"read": {"text": "ready"}})),
        ]).await;
        assert_eq!(super::retry_codex_resume(&client, "test").await.as_deref(), Some("ready"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn delivery_retries_an_idle_prompt_once_before_acceptance() {
        use serde_json::json;
        let mut script = Vec::new();
        for attempt in 0..2 {
            script.extend([
                ("agent.read", json!({"read": {"text": "ready"}})),
                ("agent.prompt", json!({})),
            ]);
            for _ in 0..if attempt == 0 { 15 } else { 1 } {
                script.extend([
                    ("agent.get", json!({"agent": {"agent_status": if attempt == 0 { "idle" } else { "working" }}})),
                    ("agent.read", json!({"read": {"text": "ready"}})),
                ]);
            }
        }
        let (client, server) = scripted_herdr(script).await;
        assert!(super::deliver_prompt(&client, "codex", "test", "ga1a7p-idle-test", None, "task").await.unwrap().is_none());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn fast_hook_turn_is_accepted_even_if_the_agent_is_idle_again() {
        use serde_json::json;
        let path = std::env::temp_dir().join(format!("ga1a7p-event-{}.jsonl", std::process::id()));
        let old = "old event\n";
        std::fs::write(&path, format!("{old}{}\n", json!({"agent": "test", "at": 1, "kind": "turn_end", "harness": "codex"}))).unwrap();
        let (client, server) = scripted_herdr(vec![
            ("agent.get", json!({"agent": {"agent_status": "idle"}})),
            ("agent.read", json!({"read": {"text": "ready"}})),
        ]).await;
        let result = super::wait_for_prompt(&client, "codex", "test", "test", None, "ready", (&path, old.len())).await;
        std::fs::remove_file(path).unwrap();
        assert!(matches!(result, super::PromptWait::Accepted));
        server.await.unwrap();
    }

    #[test]
    fn codex_lock_is_a_card_even_when_launch_times_out() {
        let modal = include_str!("../fixtures/panes/codex-conversation-open.txt");
        let card = super::blocked_card("codex", "timeout", modal, "test", "test").unwrap();
        assert_eq!(card.kind, "operator_prompt");
    }

    #[test]
    fn legacy_bot_state_has_no_launch_identity() {
        let state: crate::home::BotState = serde_json::from_value(serde_json::json!({
            "workspace_id": "w1", "root_pane": "w1:p1", "agent": "helper", "route": "codex:gpt-6-luna"
        }))
        .unwrap();
        assert_eq!(state.terminal_id, None);
    }

    #[test]
    fn restored_launch_decision() {
        use super::LaunchIdentity::*;
        assert_eq!(
            super::launch_identity(Some("t1"), Some("t1"), Some("idle")),
            Keep
        );
        assert_eq!(
            super::launch_identity(Some("t1"), Some("t2"), Some("done")),
            Recover
        );
        assert_eq!(
            super::launch_identity(None, Some("t2"), Some("idle")),
            Recover
        );
        for status in ["working", "blocked", "unknown"] {
            assert_eq!(
                super::launch_identity(None, Some("t2"), Some(status)),
                Defer
            );
        }
        assert_eq!(super::launch_identity(None, None, Some("idle")), Defer);
    }

    #[test]
    fn resume_arguments_preserve_harness_identity() {
        for (harness, expected) in [
            ("codex", vec!["resume", "session"]),
            ("claude", vec!["--resume", "session"]),
            ("pi", vec!["--session", "session"]),
            ("cursor", vec![]),
        ] {
            assert_eq!(
                super::resume_args(harness, Some((harness, "session"))),
                expected
            );
            assert!(super::resume_args(harness, Some(("other", "session"))).is_empty());
            assert!(super::resume_args(harness, None).is_empty());
        }
    }

    #[test]
    fn explicit_request_is_retained_for_exact_and_fallback_routes() {
        let exact = crate::route::Route {
            harness: "pi".into(), model: "deepseek-v4-pro".into(), effort: Some("medium".into()),
            requested: None, fallback_reason: None,
        };
        assert_eq!(requested_route(Some("pi:deepseek-v4-pro:medium"), &exact).as_deref(), Some("pi:deepseek-v4-pro:medium"));
        let fallback = crate::route::Route {
            harness: "codex".into(), model: "gpt-6-luna".into(), effort: Some("medium".into()),
            requested: Some("pi:deepseek-v4-pro:medium".into()), fallback_reason: Some("Pi unavailable".into()),
        };
        assert_eq!(requested_route(Some("pi:deepseek-v4-pro:medium"), &fallback).as_deref(), Some("pi:deepseek-v4-pro:medium"));
    }

    #[test]
    fn blocked_start_errors_create_cards_only_for_blocked_conditions() {
        let trust = include_str!("../fixtures/panes/claude-trust.txt");
        assert!(
            super::blocked_card("claude", "agent_blocked", trust, "helper", "helper").is_some()
        );
        assert!(super::blocked_card("claude", "timeout", trust, "helper", "helper").is_none());
        let other_harness = super::blocked_card("codex", "agent_blocked", trust, "helper", "helper").unwrap();
        assert_eq!(other_harness.kind, "operator_prompt");
        let generic = super::blocked_card(
            "claude", "agent_blocked", "some other prompt", "helper", "helper",
        ).unwrap();
        assert_eq!(generic.kind, "operator_prompt");
    }

    #[test]
    fn launch_ready_marker_requires_ready_prompt_for_main_agent() {
        assert!(marks_launch_ready(true, true)); // includes non-memory bots
        assert!(!marks_launch_ready(false, true));
        assert!(!marks_launch_ready(true, false));
    }

    #[test]
    fn prompt_taken_when_status_leaves_idle() {
        assert!(prompt_taken("claude", Some("working"), "screen", "screen"));
    }

    #[test]
    fn prompt_taken_when_idle_status_hides_a_changed_screen() {
        // narrow pane: herdr never sees the "Working" spinner, but the turn rewrites the screen
        assert!(prompt_taken("pi", Some("idle"), "ready", "ready\nCall list_bots"));
        assert!(!prompt_taken("claude", Some("idle"), "ready", "ready\nCall list_bots"));
        assert!(!prompt_taken("codex", None, "ready", "ready\nCall list_bots"));
    }

    #[test]
    fn prompt_dropped_when_idle_and_screen_unchanged() {
        assert!(!prompt_taken("pi", Some("idle"), "splash", "splash"));
        assert!(!prompt_taken("codex", None, "splash", "splash"));
    }
}
