//! One loop, four jobs: deliver queued prompts to idle agents, watch subagent runs, fire
//! schedules, route room messages. It also re-imports the Grok Bot roster when that file
//! changes, and cools down a harness whose agent just hit a subscription quota or auth failure.

use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use herdr_client::Client;

use crate::home::{self, Result, Run};
use crate::{cooldown, fm, grok, hook, rooms, run};

const TICK: Duration = Duration::from_secs(2);
/// After a delivery, give the agent this long to pick it up before delivering again.
const DELIVERY_COOLDOWN: i64 = 15;
/// A run counts as finished-without-report once its agent is idle/done this long after start.
const HARVEST_AFTER: i64 = 20;
const STARTUP_GRACE: i64 = 90;
/// How long after launch a launch turn's `READY` may still arrive. A marker older than this
/// means that turn end was lost, so it must no longer swallow a real turn.
const LAUNCH_READY_SECS: i64 = 600;

/// One daemon at a time: the memory files have a single writer (OptChat spec §2's method).
/// Other processes probe liveness by connecting, so the socket must be drained: an unaccepted
/// connection stays queued, and once the backlog fills every later probe is refused.
fn take_lock() -> Result<std::os::unix::net::UnixListener> {
    let listener = bind_lock()?;
    drain_in_background(&listener)?;
    Ok(listener)
}

fn drain_in_background(listener: &std::os::unix::net::UnixListener) -> Result<()> {
    let drain = listener.try_clone()?;
    std::thread::spawn(move || while drain.accept().is_ok() {});
    Ok(())
}

fn bind_lock() -> Result<std::os::unix::net::UnixListener> {
    use std::os::unix::net::{UnixListener, UnixStream};
    let path = home::daemon_lock();
    fs::create_dir_all(path.parent().unwrap())?;
    match UnixListener::bind(&path) {
        Ok(listener) => Ok(listener),
        Err(_) if UnixStream::connect(&path).is_ok() => {
            Err("another herdbot daemon is running".into())
        }
        Err(_) => {
            // A socket nobody listens on: its daemon died. Take it over.
            fs::remove_file(&path)?;
            Ok(UnixListener::bind(&path)?)
        }
    }
}

pub async fn run() -> Result<()> {
    let _lock = take_lock()?;
    let _daemon_info = match home::write_daemon_info(&home::DaemonInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        exe: std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        pid: std::process::id(),
    }) {
        Ok(()) => Some(DaemonInfoGuard),
        Err(error) => {
            eprintln!("daemon.json: {error}");
            None
        }
    };
    let herdr = Client::from_env();
    println!(
        "herdbot daemon: herdr {} at {}",
        herdr.ping().await?,
        home::root().display()
    );
    let started = home::now();
    let mut delivered_at: HashMap<String, i64> = HashMap::new();
    let mut last_fired: HashMap<String, i64> = load_fired();
    let mut roster_seen = None;
    let mut mains = MainAgents::default();
    let mut held: HashMap<String, u32> = HashMap::new();
    let mut mems = HashMap::new();
    let mut contexts = HashMap::new();
    loop {
        if let Err(e) = drain_run_outboxes() { eprintln!("run outbox: {e}"); }
        if let Err(e) = flush_queues(&herdr, &mut delivered_at, &mut mains, &mut held).await {
            eprintln!("queue: {e}");
        }
        if let Err(e) = prune_cards(&herdr).await {
            eprintln!("cards: {e}");
        }
        if let Err(e) = consume_events(&herdr, &mut mains).await {
            eprintln!("events: {e}");
        }
        if let Err(e) = crate::memory::tick(&herdr, &mut mems, &mut contexts).await {
            eprintln!("memory: {e}");
        }
        if let Err(e) = watch_runs(&herdr).await {
            eprintln!("runs: {e}");
        }
        if let Err(e) = watch_bots(&herdr, &mut mains).await {
            eprintln!("bots: {e}");
        }
        if let Err(e) = fire_schedules(&mut last_fired, started) {
            eprintln!("schedules: {e}");
        }
        if let Err(e) = rooms::tick(home::now()) {
            eprintln!("rooms: {e}");
        }
        let mtime = grok::roster_mtime();
        if mtime.is_some() && mtime != roster_seen {
            roster_seen = mtime;
            match grok::import() {
                Ok(report) if !report.is_empty() => println!("grok sync: {report}"),
                Ok(_) => {}
                Err(e) => eprintln!("grok sync: {e}"),
            }
        }
        tokio::time::sleep(TICK).await;
    }
}

fn drain_run_outboxes() -> Result<()> {
    for mut run in home::list_runs().into_iter().filter(|run| !run.outbox.is_empty()) {
        home::drain_run_outbox(&mut run)?;
    }
    Ok(())
}

fn claim_batch(path: &std::path::Path) -> Result<std::path::PathBuf> {
    if path.extension().and_then(|extension| extension.to_str()) == Some("taking") { return Ok(path.to_owned()); }
    let taken = path.with_extension("taking");
    if taken.exists() { return Ok(taken); }
    fs::rename(path, &taken)?;
    Ok(taken)
}

#[derive(Debug, PartialEq, Eq)]
enum Prune {
    Keep,
    Remove,
    Replace(herdbot_core::cards::Card),
}

fn prune_decision(card: &herdbot_core::cards::Card, status: Option<&str>, screen: &str) -> Prune {
    if status != Some("blocked") {
        return Prune::Remove;
    }
    match crate::adapters::recognize(screen, &card.bot, &card.agent) {
        None => Prune::Remove,
        Some(current) if current.id == card.id => Prune::Keep,
        Some(current) => Prune::Replace(current),
    }
}

async fn prune_cards(herdr: &Client) -> Result<()> {
    for card in herdbot_core::cards::read_cards() {
        let status = run::status(herdr, &card.agent).await;
        let screen = run::tail(herdr, &card.agent, 60).await;
        match prune_decision(&card, status.as_deref(), &screen) {
            Prune::Keep => {}
            Prune::Remove => herdbot_core::cards::remove_card(&card.agent),
            Prune::Replace(card) => herdbot_core::cards::write_card(&card)?,
        }
    }
    Ok(())
}

struct DaemonInfoGuard;

impl Drop for DaemonInfoGuard {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(home::daemon_info_path())
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("daemon.json cleanup: {error}");
        }
    }
}

/// What the daemon remembers about each bot's main agent between ticks, keyed by slug.
#[derive(Default)]
struct MainAgents {
    /// Last status seen.
    status: HashMap<String, String>,
    /// Last text delivered: re-queued on the new route if that turn hit a quota or auth
    /// failure. Text typed straight into the pane is not seen here.
    prompt: HashMap<String, String>,
    /// Last text delivered, kept until that turn's reply is logged (`prompt` above is
    /// consumed by quota failover, so turn pairing needs its own copy).
    delivered: HashMap<String, String>,
    /// Queue identity and provenance paired with the delivered prompt until its turn is logged.
    delivered_messages: HashMap<String, Vec<home::QueueReference>>,
}

#[derive(Debug, PartialEq, Eq)]
struct DeliveredTurn {
    prompt: Option<String>,
    messages: Vec<home::QueueReference>,
}

async fn flush_queues(
    herdr: &Client,
    delivered_at: &mut HashMap<String, i64>,
    mains: &mut MainAgents,
    held: &mut HashMap<String, u32>,
) -> Result<()> {
    let compactors: std::collections::HashSet<String> = home::list_runs()
        .into_iter()
        .filter(|r| r.class == "compact" && r.status == "running")
        .map(|r| r.agent)
        .collect();
    for entry in fs::read_dir(home::queue_dir())?.flatten() {
        let path = entry.path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("jsonl" | "taking")) {
            continue;
        }
        let agent = path.file_stem().unwrap().to_string_lossy().into_owned();
        if !compactors.contains(&agent)
            && home::now() - delivered_at.get(&agent).copied().unwrap_or(0) < DELIVERY_COOLDOWN
        {
            continue;
        }
        let status = match run::status(herdr, &agent).await {
            Some(s) => s,
            None if home::load_state(&agent).is_some() || home::list_bots().contains(&agent) => {
                // A bot's main agent that is not running: bring it up, deliver next tick.
                // With every harness cooling down this fails; keep flushing the other queues.
                if let Err(e) = run::up(herdr, &agent).await {
                    eprintln!("{agent}: {e}");
                }
                continue;
            }
            None => {
                eprintln!("dropping queue for vanished agent {agent}");
                fs::remove_file(&path)?;
                continue;
            }
        };
        if status != "idle" && status != "done" {
            continue;
        }
        if fm::available() {
            let tail = run::tail(herdr, &agent, 60).await;
            let times = held.get(&agent).copied().unwrap_or(0);
            let verdict = fm::classify_tail(&tail).await;
            if fm::hold(verdict.as_ref(), &tail, times) {
                held.insert(agent.clone(), times + 1);
                // Retry after DELIVERY_COOLDOWN, not every tick.
                delivered_at.insert(agent.clone(), home::now());
                eprintln!(
                    "{agent}: holding delivery, its pane shows a prompt ({}/{})",
                    times + 1,
                    fm::MAX_HOLDS
                );
                continue;
            }
            // Reset only once fm stops seeing a prompt. After MAX_HOLDS the count stays
            // saturated, so a pane fm keeps misreading is held once, not on every message.
            if !verdict.is_some_and(|v| v.kind == fm::TailKind::Approval) {
                held.remove(&agent);
            }
        }
        // Resume an interrupted batch; otherwise rename before delivery to isolate new writes.
        let taken = claim_batch(&path)?;
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") && path.exists() { continue; }
        let items = fs::read_to_string(&taken)?.lines()
            .filter_map(|line| serde_json::from_str::<home::QueuedItem>(line).ok())
            .collect::<Vec<_>>();
        let items = home::dedupe_queue_items(items);
        let texts: Vec<String> = items.iter().map(|item| item.text.clone()).collect();
        if !texts.is_empty() {
            let text = texts.join("\n\n");
            herdr.agent_prompt(&agent, &text, None).await?;
            delivered_at.insert(agent.clone(), home::now());
            // A reply can come back inside one tick (a quota notice does), so count the
            // delivery as the start of a turn for `watch_bots`.
            if home::load_state(&agent).is_some() {
                mains.status.insert(agent.clone(), "working".into());
                mains.prompt.insert(agent.clone(), text.clone());
                // Mirror `delivered` on disk so a UI can show the prompt while the turn runs.
                let messages: Vec<home::QueueReference> = items.iter().map(|item| home::QueueReference {
                    id: item.id.clone(),
                    source: item.source.clone(),
                    text: Some(item.text.clone()),
                }).collect();
                home::write_inflight_with_messages(&agent, &text, messages.clone())?;
                mains.delivered.insert(agent.clone(), text);
                mains.delivered_messages.insert(agent.clone(), messages);
            }
            println!("delivered {} message(s) to {agent}", texts.len());
        }
        fs::remove_file(taken)?;
    }
    Ok(())
}

/// `.state/launch/<agent>.ready`, written by `run::start` when a launch's first prompt asks
/// for `READY`: that turn end is instructions, not conversation.
fn launch_marker(agent: &str) -> std::path::PathBuf {
    home::root()
        .join(".state")
        .join("launch")
        .join(format!("{agent}.ready"))
}

fn marker_time(path: &std::path::Path) -> Option<i64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Whether a turn end is that launch turn. The marker decides, never the presence of a
/// pending delivery: the daemon can deliver a queued question before it consumes the
/// `READY` event, and pairing READY with that question loses the question's own turn.
fn launch_ready(marker_at: Option<i64>, event_at: i64, reply: &str) -> bool {
    marker_at.is_some_and(|at| at <= event_at && event_at - at <= LAUNCH_READY_SECS)
        && reply.trim().starts_with("READY")
}

/// The turn a main agent's event appends to `turns.jsonl`: the pending prompt, if any.
/// `None` for the launch turn's `READY`, whose delivery (if one had landed) stays pending.
fn turn_to_log(
    mains: &mut MainAgents,
    agent: &str,
    marker_at: Option<i64>,
    event_at: i64,
    reply: &str,
) -> Option<DeliveredTurn> {
    if launch_ready(marker_at, event_at, reply) {
        return None;
    }
    let inflight = home::read_inflight(agent);
    let prompt = mains.delivered.remove(agent)
        .or_else(|| inflight.as_ref().map(|sent| sent.prompt.clone()));
    let messages = mains.delivered_messages.remove(agent)
        .filter(|messages| !messages.is_empty())
        .or_else(|| inflight.map(|sent| sent.messages))
        .unwrap_or_default();
    Some(DeliveredTurn { prompt, messages })
}

/// Drain `.state/events/*.jsonl` (written by `herdbot hook`) into runs, parents and turn logs.
async fn consume_events(herdr: &Client, mains: &mut MainAgents) -> Result<()> {
    let dir = hook::events_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !matches!(path.extension().and_then(|e| e.to_str()), Some("jsonl" | "taking")) {
            continue;
        }
        // Resume an interrupted batch; otherwise rename before consuming new appends.
        let taken = claim_batch(&path)?;
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") && path.exists() { continue; }
        let mut deferred = Vec::new();
        let batch = fs::read_to_string(&taken)?;
        for line in batch.lines() {
            let Ok(event) = serde_json::from_str::<hook::Event>(line) else {
                eprintln!("events: skipped malformed line in {}", taken.display());
                continue;
            };
            let run = home::list_runs()
                .into_iter()
                .find(|r| r.agent == event.agent);
            let is_main = home::load_state(&event.agent).is_some();
            if !event.id.is_empty() && run.as_ref().is_some_and(|r| r.event_ids.contains(&event.id)) { continue; }
            match hook::decide(&event, run.as_ref(), is_main) {
                hook::Action::FinishRun(text) => {
                    let mut r = run.expect("decide only finishes known runs");
                    r.status = "done".into();
                    r.result = Some(text.clone());
                    r.result_source = "hook".into();
                    r.summary = Some(home::first_line_preview(&text));
                    r.events.push(home::RunEvent {
                        id: format!("run:{}:completed", r.id), at: event.at, kind: "completed".into(),
                        summary: r.summary.clone().unwrap(), detail: Some(text.clone()),
                    });
                    let notice = format!("[subagent {} \"{}\" finished]\nSummary: {}\n\nFull result:\n{text}", r.id, r.description, r.summary.as_deref().unwrap_or_default());
                    let notice_id = format!("run:{}:completed", r.id);
                    home::enqueue_run_notice(&mut r, &notice_id, &notice)?;
                    if !event.id.is_empty() && !r.event_ids.contains(&event.id) { r.event_ids.push(event.id.clone()); home::save_run(&r)?; }
                    println!("run {} done (hook)", r.id);
                }
                hook::Action::BlockRun(reason) => {
                    let mut r = run.expect("decide only blocks known runs");
                    let tail = run::tail(herdr, &r.agent, 60).await;
                    if !requeue_if_cooled(herdr, &mut r, &tail).await? {
                        let notice = format!(
                                "[subagent {} \"{}\" is blocked]\nIt is waiting for an approval in herdr pane {} ({reason}). Shane must answer it there.",
                                r.id, r.description, r.pane
                            );
                        let notice_id = format!("run:{}:blocked", r.id);
                        home::enqueue_run_notice(&mut r, &notice_id, &notice)?;
                        if !r.events.iter().any(|event| event.id == notice_id) {
                            r.events.push(home::RunEvent {
                                id: notice_id.clone(), at: event.at, kind: "blocked".into(),
                                summary: "Task is blocked on approval.".into(), detail: Some(reason.clone()),
                            });
                        }
                        r.notified_blocked = true;
                    }
                    if !event.id.is_empty() && !r.event_ids.contains(&event.id) { r.event_ids.push(event.id.clone()); }
                    home::save_run(&r)?;
                }
                hook::Action::Defer => deferred.push(line.to_owned()),
                hook::Action::LogTurn(reply) => {
                    if !event.id.is_empty() && hook::turn_event_seen(&event.agent, &event.id) { continue; }
                    let marker = launch_marker(&event.agent);
                    let mark = marker_time(&marker);
                    let _ = fs::remove_file(&marker);
                    if let Some(turn) = turn_to_log(mains, &event.agent, mark, event.at, &reply) {
                        hook::append_turn_with_messages(&event.agent, event.at, turn.prompt.as_deref(), &reply, &event.id, &turn.messages)?;
                        home::clear_inflight(&event.agent);
                    } else {
                        println!(
                            "launch {}: dropped READY reply: {}",
                            event.agent,
                            reply.trim().chars().take(40).collect::<String>()
                        );
                    }
                }
                hook::Action::LogBlocked(reason) => println!("blocked {}: {reason}", event.agent),
                hook::Action::Ignore => {}
                hook::Action::Drop => {
                    eprintln!("events: dropping event for unknown agent {}", event.agent)
                }
            }
        }
        restore_deferred_events(&taken, &deferred)?;
        fs::remove_file(taken)?;
    }
    Ok(())
}

fn restore_deferred_events(taken: &std::path::Path, deferred: &[String]) -> Result<()> {
    if deferred.is_empty() { return Ok(()); }
    let canonical = taken.with_extension("jsonl");
    let existing = fs::read_to_string(&canonical).unwrap_or_default();
    let mut seen: std::collections::HashSet<String> = existing.lines()
        .filter_map(|line| serde_json::from_str::<hook::Event>(line).ok())
        .map(|event| event.id)
        .filter(|id| !id.is_empty())
        .collect();
    let lines: Vec<&str> = deferred.iter().filter(|line| {
        serde_json::from_str::<hook::Event>(line).ok().is_none_or(|event| {
            event.id.is_empty() || seen.insert(event.id)
        })
    }).map(String::as_str).collect();
    if !lines.is_empty() {
        let mut file = fs::OpenOptions::new().create(true).append(true).open(canonical)?;
        std::io::Write::write_all(&mut file, format!("{}\n", lines.join("\n")).as_bytes())?;
    }
    Ok(())
}

async fn watch_runs(herdr: &Client) -> Result<()> {
    for mut r in home::list_runs() {
        if r.pane_closed {
            continue;
        }
        let status = run::status(herdr, &r.agent).await;
        // `spawn` records the route once the agent has taken its task. Until then an idle
        // agent is still starting (agent_start alone can take 60 s), not finished.
        let settled =
            matches!(status.as_deref(), Some("idle") | Some("done")) && !r.route.is_empty();
        let now = home::now();
        if r.status == "running" && r.wall_clock_limit.is_some() && !r.route.is_empty() {
            if update_active_budget(&mut r, status.as_deref() == Some("blocked"), now) {
                r.status = "expired".into();
                r.result = Some("explicit active-time limit reached".into());
                r.summary = Some("Task expired after its active-time limit.".into());
                r.events.push(home::RunEvent {
                    id: format!("run:{}:expired", r.id), at: now, kind: "expired".into(),
                    summary: r.summary.clone().unwrap(), detail: r.result.clone(),
                });
                home::save_run(&r)?;
                let _ = herdr.pane_close(&r.pane).await;
                r.pane_closed = true;
                home::save_run(&r)?;
                continue;
            }
            home::save_run(&r)?;
        }
        match (settled, r.settled_at) {
            (true, None) => {
                r.settled_at = Some(now);
                home::save_run(&r)?;
            }
            (false, Some(_)) => {
                r.settled_at = None;
                home::save_run(&r)?;
            }
            _ => {}
        }
        let harness = r.route.split(':').next().unwrap_or_default().to_owned();
        let grace_over = hook::grace_over(&harness, r.settled_at, now);
        match (r.status.as_str(), status.as_deref()) {
            // agent.get fails until launch completes (agent_start allows 60s), so only a
            // missing agent past that window counts as an exit.
            ("running", None) if home::now() - r.started_at > STARTUP_GRACE => {
                r.status = "failed".into();
                r.result = Some("subagent exited without reporting".into());
                r.events.push(home::RunEvent {
                    id: format!("run:{}:failed", r.id), at: now, kind: "failed".into(),
                    summary: "Task exited without reporting.".into(), detail: r.result.clone(),
                });
                let notice = format!(
                        "[subagent {} \"{}\" failed]\nIt exited without reporting.",
                        r.id, r.description
                    );
                let notice_id = format!("run:{}:failed", r.id);
                home::enqueue_run_notice(&mut r, &notice_id, &notice)?;
                r.pane_closed = true;
            }
            ("running", Some("blocked")) if !r.notified_blocked && !r.route.is_empty() => {
                // claude's limit screen offers choices, so a quota hit can show up as blocked.
                let text = run::tail(herdr, &r.agent, 60).await;
                if !requeue_if_cooled(herdr, &mut r, &text).await? {
                    let notice = format!(
                            "[subagent {} \"{}\" is blocked]\nIt is waiting for an approval in herdr pane {}. Shane must answer it there.",
                            r.id, r.description, r.pane
                        );
                    let notice_id = format!("run:{}:blocked", r.id);
                    home::enqueue_run_notice(&mut r, &notice_id, &notice)?;
                    if !r.events.iter().any(|event| event.id == notice_id) {
                        r.events.push(home::RunEvent {
                            id: notice_id.clone(), at: now, kind: "blocked".into(),
                            summary: "Task is blocked on approval.".into(), detail: Some(text.clone()),
                        });
                    }
                    r.notified_blocked = true;
                }
            }
            ("running", _)
                if settled
                    && grace_over
                    && r.class != "compact"
                    && now - r.started_at > HARVEST_AFTER =>
            {
                let text = run::tail(herdr, &r.agent, 60).await;
                if requeue_if_cooled(herdr, &mut r, &text).await? {
                    home::save_run(&r)?;
                    continue;
                }
                // Finished its turn without calling report_result: hand back what it printed.
                r.status = "done".into();
                r.result = Some(text.clone());
                r.result_source = "tail".into();
                r.summary = Some(home::first_line_preview(&text));
                r.events.push(home::RunEvent {
                    id: format!("run:{}:completed", r.id), at: now, kind: "completed".into(),
                    summary: r.summary.clone().unwrap(), detail: Some(text.clone()),
                });
                let notice = format!(
                        "[subagent {} \"{}\" finished without report_result]\nSummary: {}\n\nFull result:\n{text}",
                        r.id, r.description, r.summary.as_deref().unwrap_or_default()
                    );
                let notice_id = format!("run:{}:completed", r.id);
                home::enqueue_run_notice(&mut r, &notice_id, &notice)?;
            }
            ("done" | "failed", _) if settled || status.is_none() => {
                let _ = herdr.pane_close(&r.pane).await;
                r.pane_closed = true;
            }
            _ => continue,
        }
        home::save_run(&r)?;
    }
    Ok(())
}

/// Advance active-time accounting; blocked intervals are excluded and never consume the card.
fn update_active_budget(run: &mut home::Run, blocked: bool, now: i64) -> bool {
    let Some(limit) = run.wall_clock_limit else { return false };
    if blocked {
        run.blocked_since.get_or_insert(now);
        run.budget_checked_at = Some(now);
        return false;
    }
    if run.blocked_since.take().is_some() {
        run.budget_checked_at = Some(now);
    } else if let Some(previous) = run.budget_checked_at {
        let elapsed = now.saturating_sub(previous).max(0) as u64;
        run.active_seconds = Some(run.active_seconds.unwrap_or_default().saturating_add(elapsed));
        run.budget_checked_at = Some(now);
    } else {
        run.budget_checked_at = Some(now);
    }
    run.active_seconds.unwrap_or_default() >= limit
}

/// The harness's own failure text first; fm only when that finds nothing. Subagent runs only:
/// a misread here re-runs one task, while for a main agent it would cost the conversation.
async fn detect_failure(harness: &str, tail: &str) -> Option<cooldown::Cooldown> {
    let now = chrono::Local::now();
    match cooldown::detect(harness, tail, now) {
        Some(c) => Some(c),
        None => fm::cooldown_backstop(tail, now.timestamp()).await,
    }
}

/// If `tail` shows the run's harness out of quota or locked out, cool that harness down,
/// close the run, re-queue its task on the next route and tell the parent. Returns whether
/// it did.
async fn requeue_if_cooled(herdr: &Client, r: &mut Run, tail: &str) -> Result<bool> {
    let harness = r.route.split(':').next().unwrap_or_default();
    let Some(c) = detect_failure(harness, tail).await else {
        return Ok(false);
    };
    cooldown::record(harness, &c)?;
    println!("cooldown {harness}: {}", c.reason);
    r.status = "failed".into();
    let stopped_at = home::now();
    r.events.push(home::RunEvent {
        id: format!("run:{}:failed", r.id), at: stopped_at, kind: "failed".into(),
        summary: "Task failed after a harness quota or authentication limit.".into(), detail: None,
    });
    let _ = herdr.pane_close(&r.pane).await;
    r.pane_closed = true;
    let note = if r.class == "compact" {
        "The memory tick starts a fresh compactor.".to_owned()
    } else if r.prompt.is_empty() {
        "It predates re-queueing (no stored prompt); start it again if it is still needed."
            .to_owned()
    } else {
        match run::spawn(
            herdr,
            &r.bot,
            &r.parent_agent,
            run::SpawnRequest {
                class: &r.class,
                explicit_model: None,
                description: &r.description,
                prompt: &r.prompt,
                done_when: None,
                wall_clock_limit: None,
            },
        )
        .await
        {
            Ok(next) if next.status != "failed" => format!(
                "Re-queued as subagent {} on {}; its result will follow.",
                next.id, next.route
            ),
            Ok(next) => format!("Re-queueing failed: {}", next.result.unwrap_or_default()),
            Err(e) => format!("Re-queueing failed: {e}"),
        }
    };
    // Only the kind ("quota" / "auth") goes to the parent: the raw CLI line would sit in the
    // parent's own pane and trip detection there.
    let kind = c.reason.split(':').next().unwrap_or_default();
    let summary = format!(
        "{harness} hit a {kind} failure; it is skipped until {}.",
        c.until_local()
    );
    r.result = Some(format!("{summary} {note}"));
    let notice = format!(
            "[subagent {} \"{}\" stopped]\n{summary} {note}",
            r.id, r.description
        );
    let notice_id = format!("run:{}:stopped", r.id);
    home::enqueue_run_notice(r, &notice_id, &notice)?;
    Ok(true)
}

/// When the turn a main agent just finished hit a quota or auth failure: cool its harness
/// down, restart it on the next route and re-queue the prompt that failed. Only a working →
/// settled change counts, so the scrollback a daemon restart finds is not re-read.
async fn watch_bots(herdr: &Client, mains: &mut MainAgents) -> Result<()> {
    for slug in home::list_bots() {
        let Some(state) = home::load_state(&slug) else {
            continue;
        };
        let Some(status) = run::status(herdr, &state.agent).await else {
            mains.status.remove(&slug);
            continue;
        };
        let was = mains.status.insert(slug.clone(), status.clone());
        if was.as_deref() != Some("working")
            || !matches!(status.as_str(), "idle" | "done" | "blocked")
        {
            continue;
        }
        // A stored prompt belongs to this turn only; an older one was answered already.
        let prompt = mains.prompt.remove(&slug);
        let harness = state.route.split(':').next().unwrap_or_default();
        let tail = run::tail(herdr, &state.agent, 60).await;
        let Some(c) = cooldown::detect(harness, &tail, chrono::Local::now()) else {
            continue;
        };
        cooldown::record(harness, &c)?;
        println!("cooldown {harness} (main agent {slug}): {}", c.reason);
        let harness = harness.to_owned();
        match run::restart(herdr, &slug, state).await {
            Ok(new) => {
                println!("restarted {slug} on {}", new.route);
                mains.status.remove(&slug);
                if let Some(prompt) = prompt {
                    let kind = c.reason.split(':').next().unwrap_or_default();
                    home::enqueue(
                        &slug,
                        &format!(
                            "[herdbot: the previous session on {harness} stopped with a {kind} failure. This is a new session on {}, without that conversation. The message it could not answer follows.]\n\n{prompt}",
                            new.route
                        ),
                    )?;
                }
            }
            Err(e) => eprintln!("{slug}: staying on {harness}: {e}"),
        }
    }
    Ok(())
}

fn fired_path() -> std::path::PathBuf {
    home::root().join(".state").join("schedules.json")
}

fn load_fired() -> HashMap<String, i64> {
    fs::read_to_string(fired_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Fire each enabled schedule whose next occurrence after its last firing has passed. No
/// catch-up: a schedule never fired before counts from daemon start.
fn fire_schedules(last_fired: &mut HashMap<String, i64>, started: i64) -> Result<()> {
    let now = chrono::Local::now();
    let mut changed = false;
    for slug in home::list_bots() {
        let Ok(bot) = home::load_bot(&slug) else {
            continue;
        };
        for schedule in bot.schedules.iter().filter(|s| s.enabled) {
            let key = format!("{slug}/{}", schedule.name);
            let since = last_fired.get(&key).copied().unwrap_or(started);
            let Some(since) = chrono::DateTime::from_timestamp(since, 0) else {
                continue;
            };
            let cron = match schedule.cron.parse::<croner::Cron>() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{key}: bad cron {:?}: {e}", schedule.cron);
                    continue;
                }
            };
            let Ok(next) = cron.find_next_occurrence(&since.with_timezone(&chrono::Local), false)
            else {
                continue;
            };
            if next <= now {
                home::enqueue_from(
                    &run::main_agent(&slug),
                    "schedule",
                    &format!("[scheduled: {}]\n{}", schedule.name, schedule.prompt),
                )?;
                println!("fired {key}");
                last_fired.insert(key, now.timestamp());
                changed = true;
            }
        }
    }
    if changed {
        fs::write(fired_path(), serde_json::to_vec(last_fired)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn prune_keeps_a_card_only_while_the_same_prompt_is_on_a_blocked_agent() {
        let trust = include_str!("../fixtures/panes/claude-trust.txt");
        let card = crate::adapters::recognize(trust, "helper", "helper").unwrap();
        assert_eq!(prune_decision(&card, Some("blocked"), trust), Prune::Keep);
        assert_eq!(prune_decision(&card, Some("idle"), trust), Prune::Remove);
        assert_eq!(prune_decision(&card, None, trust), Prune::Remove);
        assert_eq!(
            prune_decision(&card, Some("blocked"), "gone"),
            Prune::Remove
        );
        let moved = trust.replace("trustfresh-92905", "trustfresh-1");
        assert!(
            matches!(prune_decision(&card, Some("blocked"), &moved), Prune::Replace(c) if c.id != card.id)
        );
    }
    #[test]
    fn liveness_probes_never_fill_the_lock_backlog() {
        use std::os::unix::net::{UnixListener, UnixStream};
        let path =
            std::env::temp_dir().join(format!("herdbot-lock-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        drain_in_background(&listener).unwrap();
        // More than the 128-entry backlog; each probe is dropped, as `status` and `doctor` do.
        for n in 0..400 {
            assert!(UnixStream::connect(&path).is_ok(), "probe {n} refused");
        }
        let _ = std::fs::remove_file(&path);
    }

    use super::*;
    use chrono::TimeZone;

    #[test]
    fn only_fresh_ready_reply_is_dropped_and_delivery_stays_pending() {
        // Live bug: a queued question was delivered in the same tick as the READY turn end,
        // so `delivered` held a prompt. The old `prompt.is_none()` guard then logged READY
        // paired with that question and consumed its prompt for good.
        let mut mains = MainAgents::default();
        mains
            .delivered
            .insert("mem-test".into(), "what next?".into());
        assert_eq!(
            turn_to_log(&mut mains, "mem-test", Some(100), 104, "  READY, waiting."),
            None
        );
        assert_eq!(
            mains.delivered.get("mem-test").map(String::as_str),
            Some("what next?"),
            "the delivery must stay pending for the reply it belongs to"
        );
        // A non-READY first event clears the marker but is logged and paired normally.
        let mut pong = MainAgents::default();
        pong.delivered.insert("mem-test".into(), "say PONG".into());
        assert_eq!(
            turn_to_log(&mut pong, "mem-test", Some(100), 104, "PONG"),
            Some(DeliveredTurn { prompt: Some("say PONG".into()), messages: Vec::new() })
        );
        assert!(!launch_ready(Some(100), 104, "PONG"));

        // A marker whose turn end never came (stale) and no marker: log.
        let mut stale = MainAgents::default();
        assert_eq!(
            turn_to_log(
                &mut stale,
                "mem-test",
                Some(100),
                100 + LAUNCH_READY_SECS + 1,
                "READY late"
            ),
            Some(DeliveredTurn { prompt: None, messages: Vec::new() })
        );
        assert_eq!(
            turn_to_log(&mut stale, "mem-test", None, 104, "READY"),
            Some(DeliveredTurn { prompt: None, messages: Vec::new() })
        );
        let mut fresh = MainAgents::default();
        fresh.delivered.insert("mem-test".into(), "q".into());
        assert_eq!(
            turn_to_log(&mut fresh, "mem-test", Some(104), 100, "READY"),
            Some(DeliveredTurn { prompt: Some("q".into()), messages: Vec::new() }),
            "a mark written after the event (clock skew) is not the launch turn"
        );
    }

    #[test]
    fn delivered_turn_keeps_each_queue_message_identity_and_source() {
        let mut mains = MainAgents::default();
        mains.delivered.insert("mem-test".into(), "joined prompt".into());
        let messages = vec![home::QueueReference {
            id: Some("queue-1".into()),
            source: "bot_message".into(),
            text: Some("[message from beta] review this".into()),
        }];
        mains.delivered_messages.insert("mem-test".into(), messages.clone());
        assert_eq!(
            turn_to_log(&mut mains, "mem-test", None, 104, "done"),
            Some(DeliveredTurn { prompt: Some("joined prompt".into()), messages })
        );
    }

    #[test]
    fn event_batch_survives_error_after_taking_rename_and_is_reused_on_restart() {
        let dir = std::env::temp_dir().join(format!("herdbot-taking-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("agent.jsonl");
        std::fs::write(&path, "{\"id\":\"event-1\",\"at\":1,\"agent\":\"agent\",\"kind\":\"turn_end\",\"harness\":\"claude\"}\n").unwrap();
        let taken = claim_batch(&path).unwrap();
        assert_eq!(taken.extension().and_then(|ext| ext.to_str()), Some("taking"));
        // Simulate a consumer error: it returns without deleting the renamed batch.
        let restarted = claim_batch(&taken).unwrap();
        assert_eq!(restarted, taken);
        let recovered = std::fs::read_to_string(&restarted).unwrap();
        assert_eq!(recovered.lines().count(), 1);
        let turn_log = dir.join("turns.jsonl");
        let event_id = "event-1";
        hook::append_turn_record(&turn_log, 1, Some("hello"), "world", event_id, &[]).unwrap();
        // The process crashed after writing visible history but before unlinking .taking.
        let restarted = claim_batch(&taken).unwrap();
        assert_eq!(restarted, taken);
        hook::append_turn_record(&turn_log, 1, Some("hello"), "world", event_id, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(turn_log).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn active_budget_pauses_while_blocked_and_expires_only_after_active_limit() {
        let mut run = home::Run {
            wall_clock_limit: Some(10), active_seconds: Some(0), budget_checked_at: Some(100),
            ..Default::default()
        };
        assert!(!update_active_budget(&mut run, false, 106));
        assert_eq!(run.active_seconds, Some(6));
        assert!(!update_active_budget(&mut run, true, 107));
        assert_eq!(run.blocked_since, Some(107));
        assert!(!update_active_budget(&mut run, true, 1000));
        assert_eq!(run.active_seconds, Some(6));
        assert!(!update_active_budget(&mut run, false, 1001));
        assert_eq!(run.active_seconds, Some(6));
        assert!(update_active_budget(&mut run, false, 1005));
        assert_eq!(run.active_seconds, Some(10));
    }

    #[test]
    fn no_budget_never_expires() {
        let mut run = home::Run { started_at: 0, ..Default::default() };
        assert!(!update_active_budget(&mut run, false, i64::MAX));
        assert_eq!(run.active_seconds, None);
    }

    #[test]
    fn deferred_events_recover_to_canonical_file_without_duplicates() {
        let dir = std::env::temp_dir().join(format!("herdbot-deferred-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let taken = dir.join("agent.taking");
        let canonical = dir.join("agent.jsonl");
        let deferred = r#"{"id":"event-2","at":2,"agent":"agent","kind":"turn_end","harness":"claude"}"#.to_owned();
        std::fs::write(&taken, "batch").unwrap();
        restore_deferred_events(&taken, &[deferred.clone(), deferred.clone()]).unwrap();
        assert_eq!(std::fs::read_to_string(&canonical).unwrap(), format!("{deferred}\n"));
        restore_deferred_events(&taken, std::slice::from_ref(&deferred)).unwrap();
        assert_eq!(std::fs::read_to_string(&canonical).unwrap().lines().count(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cron_next_occurrence() {
        let cron: croner::Cron = "0 9 * * 1-5".parse().unwrap();
        // Saturday 2026-10-17 10:00 → Monday 09:00.
        let sat = chrono::Local
            .with_ymd_and_hms(2026, 10, 17, 10, 0, 0)
            .unwrap();
        let next = cron.find_next_occurrence(&sat, false).unwrap();
        assert_eq!(
            next,
            chrono::Local
                .with_ymd_and_hms(2026, 10, 19, 9, 0, 0)
                .unwrap()
        );
    }
}
