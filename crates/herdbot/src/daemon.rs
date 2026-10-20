//! One loop, three jobs: deliver queued prompts to idle agents, watch subagent runs, fire
//! schedules. It also re-imports the Grok Bot roster when that file changes.

use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use herdr_client::Client;

use crate::home::{self, Result};
use crate::{grok, run};

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
    loop {
        if let Err(e) = flush_queues(&herdr, &mut delivered_at).await {
            eprintln!("queue: {e}");
        }
        if let Err(e) = watch_runs(&herdr).await {
            eprintln!("runs: {e}");
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

async fn flush_queues(herdr: &Client, delivered_at: &mut HashMap<String, i64>) -> Result<()> {
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
                home::enqueue(
                    &r.parent_agent,
                    &format!(
                        "[subagent {} \"{}\" is blocked]\nIt is waiting for an approval in herdr pane {}. Shane must answer it there.",
                        r.id, r.description, r.pane
                    ),
                )?;
                r.notified_blocked = true;
            }
            ("running", _) if settled && home::now() - r.started_at > HARVEST_AFTER => {
                // Finished its turn without calling report_result: hand back what it printed.
                let text = run::tail(herdr, &r.agent, 60).await;
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
