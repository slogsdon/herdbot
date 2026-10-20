//! One loop, three jobs: deliver queued prompts to idle agents, watch subagent runs, fire
//! schedules. It also re-imports the Grok Bot roster when that file changes, and cools down
//! a harness whose agent just hit a subscription quota or auth failure.

use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use herdr_client::Client;

use crate::home::{self, Result, Run};
use crate::{cooldown, grok, run};

const TICK: Duration = Duration::from_secs(2);
/// After a delivery, give the agent this long to pick it up before delivering again.
const DELIVERY_COOLDOWN: i64 = 15;
/// A run counts as finished-without-report once its agent is idle/done this long after start.
const HARVEST_AFTER: i64 = 20;
const STARTUP_GRACE: i64 = 90;

pub async fn run() -> Result<()> {
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
    let mut main_status: HashMap<String, String> = HashMap::new();
    loop {
        if let Err(e) = flush_queues(&herdr, &mut delivered_at, &mut main_status).await {
            eprintln!("queue: {e}");
        }
        if let Err(e) = watch_runs(&herdr).await {
            eprintln!("runs: {e}");
        }
        if let Err(e) = watch_bots(&herdr, &mut main_status).await {
            eprintln!("bots: {e}");
        }
        if let Err(e) = fire_schedules(&mut last_fired, started) {
            eprintln!("schedules: {e}");
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

async fn flush_queues(
    herdr: &Client,
    delivered_at: &mut HashMap<String, i64>,
    main_status: &mut HashMap<String, String>,
) -> Result<()> {
    for entry in fs::read_dir(home::queue_dir())?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let agent = path.file_stem().unwrap().to_string_lossy().into_owned();
        if home::now() - delivered_at.get(&agent).copied().unwrap_or(0) < DELIVERY_COOLDOWN {
            continue;
        }
        let status = match run::status(herdr, &agent).await {
            Some(s) => s,
            None if home::load_state(&agent).is_some() || home::list_bots().contains(&agent) => {
                // A bot's main agent that is not running: bring it up, deliver next tick.
                run::up(herdr, &agent).await?;
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
        // Rename first so prompts queued while we deliver land in a fresh file.
        let taken = path.with_extension("taking");
        fs::rename(&path, &taken)?;
        let texts: Vec<String> = fs::read_to_string(&taken)?
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| v["text"].as_str().map(str::to_owned))
            .collect();
        if !texts.is_empty() {
            herdr
                .agent_prompt(&agent, &texts.join("\n\n"), None)
                .await?;
            delivered_at.insert(agent.clone(), home::now());
            // A reply can come back inside one tick (a quota notice does), so count the
            // delivery as the start of a turn for `watch_bots`.
            if home::load_state(&agent).is_some() {
                main_status.insert(agent.clone(), "working".into());
            }
            println!("delivered {} message(s) to {agent}", texts.len());
        }
        fs::remove_file(taken)?;
    }
    Ok(())
}

async fn watch_runs(herdr: &Client) -> Result<()> {
    for mut r in home::list_runs() {
        if r.pane_closed {
            continue;
        }
        let status = run::status(herdr, &r.agent).await;
        let settled = matches!(status.as_deref(), Some("idle") | Some("done"));
        match (r.status.as_str(), status.as_deref()) {
            // agent.get fails until launch completes (agent_start allows 60s), so only a
            // missing agent past that window counts as an exit.
            ("running", None) if home::now() - r.started_at > STARTUP_GRACE => {
                r.status = "failed".into();
                r.result = Some("subagent exited without reporting".into());
                home::enqueue(
                    &r.parent_agent,
                    &format!(
                        "[subagent {} \"{}\" failed]\nIt exited without reporting.",
                        r.id, r.description
                    ),
                )?;
                r.pane_closed = true;
            }
            ("running", Some("blocked")) if !r.notified_blocked => {
                // claude's limit screen offers choices, so a quota hit can show up as blocked.
                let text = run::tail(herdr, &r.agent, 60).await;
                if !requeue_if_cooled(herdr, &mut r, &text).await? {
                    home::enqueue(
                        &r.parent_agent,
                        &format!(
                            "[subagent {} \"{}\" is blocked]\nIt is waiting for an approval in herdr pane {}. Shane must answer it there.",
                            r.id, r.description, r.pane
                        ),
                    )?;
                    r.notified_blocked = true;
                }
            }
            ("running", _) if settled && home::now() - r.started_at > HARVEST_AFTER => {
                let text = run::tail(herdr, &r.agent, 60).await;
                if requeue_if_cooled(herdr, &mut r, &text).await? {
                    home::save_run(&r)?;
                    continue;
                }
                // Finished its turn without calling report_result: hand back what it printed.
                r.status = "done".into();
                r.result = Some(text.clone());
                home::enqueue(
                    &r.parent_agent,
                    &format!(
                        "[subagent {} \"{}\" finished without report_result; its last output]\n{text}",
                        r.id, r.description
                    ),
                )?;
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

/// If `tail` shows the run's harness out of quota or locked out, cool that harness down,
/// close the run, re-queue its task on the next route and tell the parent. Returns whether
/// it did.
async fn requeue_if_cooled(herdr: &Client, r: &mut Run, tail: &str) -> Result<bool> {
    let harness = r.route.split(':').next().unwrap_or_default();
    let Some(c) = cooldown::detect(harness, tail, chrono::Local::now()) else {
        return Ok(false);
    };
    cooldown::record(harness, &c)?;
    println!("cooldown {harness}: {}", c.reason);
    r.status = "failed".into();
    let _ = herdr.pane_close(&r.pane).await;
    r.pane_closed = true;
    let note = if r.prompt.is_empty() {
        "It predates re-queueing (no stored prompt); start it again if it is still needed."
            .to_owned()
    } else {
        match run::spawn(
            herdr,
            &r.bot,
            &r.parent_agent,
            &r.class,
            None,
            &r.description,
            &r.prompt,
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
    home::enqueue(
        &r.parent_agent,
        &format!(
            "[subagent {} \"{}\" stopped]\n{summary} {note}",
            r.id, r.description
        ),
    )?;
    Ok(true)
}

/// Cool down a main agent's harness when the turn it just finished hit a quota or auth
/// failure. Only a working → settled change counts, so the scrollback a daemon restart finds
/// is not re-read.
// ponytail: cooldown only; the main agent keeps its pane. It moves to the next route the next
// time `run::up` restarts it.
async fn watch_bots(herdr: &Client, last: &mut HashMap<String, String>) -> Result<()> {
    for slug in home::list_bots() {
        let Some(state) = home::load_state(&slug) else {
            continue;
        };
        let Some(status) = run::status(herdr, &state.agent).await else {
            last.remove(&slug);
            continue;
        };
        let was = last.insert(slug, status.clone());
        if was.as_deref() != Some("working")
            || !matches!(status.as_str(), "idle" | "done" | "blocked")
        {
            continue;
        }
        let harness = state.route.split(':').next().unwrap_or_default();
        let tail = run::tail(herdr, &state.agent, 60).await;
        if let Some(c) = cooldown::detect(harness, &tail, chrono::Local::now()) {
            cooldown::record(harness, &c)?;
            println!(
                "cooldown {harness} (main agent {}): {}",
                state.agent, c.reason
            );
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
                home::enqueue(
                    &run::main_agent(&slug),
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
    use chrono::TimeZone;

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
