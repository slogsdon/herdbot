//! herdbot: Grok-Bot-style bots on herdr.

mod daemon;
mod doctor;
mod fm;
mod grok;
mod hook;
mod mcp;
mod memory;
mod rooms;
mod route;
mod run;
mod service;
mod setup;
mod tools;

use std::io::Write;

use herdr_client::Client;
use herdbot_core::{cooldown, history, home, snapshot};

use home::Result;

const USAGE: &str = "usage:
  herdbot --version
  herdbot doctor [--json]               first-run and health checks (exit 1 if any fail)
  herdbot setup [--yes] [--only home|cli-link|service|restart-daemon]   fix what doctor reports
  herdbot daemon                         deliver messages, watch subagents, fire schedules, sync Grok Bot
  herdbot service install|uninstall|status   run the daemon as a launchd agent (from ~/.cargo/bin)
  herdbot import-grok                    one-way import of Grok Bot's roster (instructions)
  herdbot new <name> [--cwd DIR] [--class CLASS]
  herdbot list | runs
  herdbot status --json                  one snapshot of bots, runs, cooldowns and blocked agents
  herdbot watch --json                   NDJSON: a snapshot line, then changes (for UIs)
  herdbot history <bot|room:NAME> --json [--before N] [--limit L]   a conversation, newest page first
  herdbot focus <agent>                  focus an agent's pane in herdr
  herdbot up <bot> | down <bot>
  herdbot send <bot> <text…>             queue a prompt for the bot (needs the daemon)
  herdbot room new <name> --members a,b[,c] [--mode channel|round_robin|moderated] [--moderator BOT]
  herdbot room send <room> <text…> | room list | room up <room>
  herdbot task <bot> <class> <prompt…>   start a subagent for the bot from the shell
  herdbot route <class> [--bot BOT]      show which harness:model a class resolves to
  herdbot hook <claude|codex|pi|cursor> [payload]   a harness hook (installed by launch)
  herdbot mcp --bot BOT [--run ID]       stdio MCP server (launched by harnesses)
  herdbot tool <name> --bot BOT [--run ID] <json>   one tool call, for debugging from a shell";

fn flag(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

/// `--bot` / `--run`, falling back to the pane environment (cursor's shared mcp.json).
fn ctx(args: &mut Vec<String>) -> Result<tools::Ctx> {
    let bot = flag(args, "--bot")
        .or_else(|| std::env::var("HERDBOT_BOT").ok())
        .ok_or("--bot (or HERDBOT_BOT) is required")?;
    let run = flag(args, "--run").or_else(|| std::env::var("HERDBOT_RUN").ok());
    Ok(tools::Ctx { bot, run })
}

#[tokio::main]
async fn main() {
    if let Err(e) = cli().await {
        eprintln!("herdbot: {e}");
        std::process::exit(1);
    }
}

async fn cli() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        println!("{USAGE}");
        return Ok(());
    }
    let command = args.remove(0);
    let herdr = Client::from_env();
    let rest = |args: &[String], from: usize| args.get(from..).unwrap_or_default().join(" ");
    match command.as_str() {
        "--version" | "-V" | "version" => {
            println!("herdbot {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "doctor" => {
            let checks = doctor::evaluate(&doctor::gather().await);
            if args.iter().any(|arg| arg == "--json") {
                println!("{}", doctor::to_json(&checks));
            } else {
                print!("{}", doctor::render(&checks));
            }
            if checks.iter().any(|check| check.status == doctor::Status::Fail) {
                std::process::exit(1);
            }
            Ok(())
        }
        "setup" => {
            let only = flag(&mut args, "--only");
            let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
            setup::run(only, yes).await
        }
        "daemon" => daemon::run().await,
        "service" => {
            let out = match args.first().map(String::as_str) {
                Some("install") => service::install()?,
                Some("uninstall") => service::uninstall()?,
                Some("status") => service::status()?,
                _ => return Err(USAGE.into()),
            };
            println!("{out}");
            Ok(())
        }
        "import-grok" => {
            println!("{}", grok::import()?);
            Ok(())
        }
        "new" => {
            let cwd = flag(&mut args, "--cwd");
            let class = flag(&mut args, "--class");
            let name = rest(&args, 0);
            if name.is_empty() {
                return Err(USAGE.into());
            }
            let dir = setup::create_bot(&name, cwd, class, "")?;
            println!("created {} (edit BOT.md for its instructions)", dir.display());
            Ok(())
        }
        "list" => {
            for slug in home::list_bots() {
                let bot = home::load_bot(&slug)?;
                let live = if home::load_state(&slug).is_some() {
                    "up"
                } else {
                    ""
                };
                let schedules = bot.schedules.iter().filter(|s| s.enabled).count();
                println!(
                    "{slug:20} {:24} {:5} {} schedules on, source {}",
                    bot.title, live, schedules, bot.source
                );
            }
            Ok(())
        }
        "runs" => {
            let mut runs = home::list_runs();
            runs.sort_by_key(|r| r.started_at);
            for r in runs {
                println!(
                    "{} {:8} {:16} {:28} {}",
                    r.id, r.status, r.bot, r.route, r.description
                );
            }
            Ok(())
        }
        "status" => {
            if !args.iter().any(|a| a == "--json") {
                return Err(USAGE.into());
            }
            println!("{}", serde_json::to_string(&snapshot::snapshot())?);
            Ok(())
        }
        "watch" => {
            if !args.iter().any(|a| a == "--json") {
                return Err(USAGE.into());
            }
            let mut watcher = snapshot::Watcher::default();
            let mut out = std::io::stdout();
            loop {
                for line in watcher.poll() {
                    // A closed pipe means the reader is gone.
                    if writeln!(out, "{line}").is_err() || out.flush().is_err() {
                        return Ok(());
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
        "history" => {
            if !args.iter().any(|a| a == "--json") {
                return Err(USAGE.into());
            }
            let number = |value: Option<String>| value.map(|v| v.parse::<usize>()).transpose();
            let before = number(flag(&mut args, "--before"))?;
            let limit = number(flag(&mut args, "--limit"))?.unwrap_or(history::DEFAULT_LIMIT);
            let target = args.iter().find(|a| *a != "--json").ok_or(USAGE)?;
            println!("{}", serde_json::to_string(&history::history(target, before, limit)?)?);
            Ok(())
        }
        "focus" => {
            herdr.agent_focus(args.first().ok_or(USAGE)?).await?;
            Ok(())
        }
        "up" => {
            let state = run::up(&herdr, args.first().ok_or(USAGE)?).await?;
            println!(
                "{} in workspace {} pane {}",
                state.agent, state.workspace_id, state.root_pane
            );
            Ok(())
        }
        "down" => run::down(&herdr, args.first().ok_or(USAGE)?).await,
        "send" => {
            let bot = args.first().ok_or(USAGE)?.clone();
            // Only the daemon launches bots: it brings a stopped bot up when it finds its queue.
            home::load_bot(&bot)?;
            home::enqueue(&run::main_agent(&bot), &rest(&args, 1))?;
            println!("queued for {bot} (the daemon delivers it)");
            Ok(())
        }
        "task" => {
            let (bot, class) = (
                args.first().ok_or(USAGE)?.clone(),
                args.get(1).ok_or(USAGE)?.clone(),
            );
            let prompt = rest(&args, 2);
            let r = run::spawn(
                &herdr,
                &bot,
                &run::main_agent(&bot),
                &class,
                None,
                "shell task",
                &prompt,
            )
            .await?;
            println!("run {} {} on {} in {}", r.id, r.status, r.route, r.pane);
            Ok(())
        }
        "route" => {
            let bot = flag(&mut args, "--bot").unwrap_or_default();
            println!(
                "{}",
                route::resolve(&route::table()?, args.first().ok_or(USAGE)?, &bot, None)?
            );
            Ok(())
        }
        "hook" => {
            hook::run(
                args.first().map(String::as_str).unwrap_or_default(),
                args.get(1..).unwrap_or_default(),
            );
            Ok(())
        }
        "room" => {
            if args.is_empty() {
                return Err(USAGE.into());
            }
            let sub = args.remove(0);
            match sub.as_str() {
                "new" => {
                    let members: Vec<String> = flag(&mut args, "--members")
                        .ok_or(USAGE)?
                        .split(',')
                        .map(|m| home::slug(m.trim()))
                        .collect();
                    let mode = match flag(&mut args, "--mode").as_deref() {
                        None | Some("channel") => rooms::Mode::Channel,
                        Some("round_robin") => rooms::Mode::RoundRobin,
                        Some("moderated") => rooms::Mode::Moderated,
                        Some(other) => return Err(format!("unknown mode {other:?}").into()),
                    };
                    let moderator = flag(&mut args, "--moderator").map(|m| home::slug(&m));
                    let name = rest(&args, 0);
                    if name.is_empty() {
                        return Err(USAGE.into());
                    }
                    let slug = home::slug(&name);
                    if rooms::room_dir(&slug).join("room.toml").exists() {
                        return Err(format!("room {slug} already exists").into());
                    }
                    let room = rooms::Room { name, members, mode, moderator, ..Default::default() };
                    rooms::validate(&room, &home::list_bots())?;
                    rooms::save_room(&slug, &room)?;
                    std::fs::write(rooms::room_dir(&slug).join("transcript.md"), "")?;
                    println!("created room {slug} ({:?}, {})", room.mode, room.members.join(", "));
                }
                "send" => {
                    let slug = args.first().ok_or(USAGE)?.clone();
                    rooms::load_room(&slug)?;
                    rooms::append(&slug, "you", &rest(&args, 1))?;
                    println!("posted to {slug} (the daemon routes it)");
                }
                "list" => {
                    for slug in rooms::list_rooms() {
                        let room = rooms::load_room(&slug)?;
                        let state = if rooms::load_state(&slug).paused { "paused" } else { "active" };
                        println!(
                            "{slug:20} {:24} {:12} {:7} {}",
                            room.name,
                            format!("{:?}", room.mode),
                            state,
                            room.members.join(",")
                        );
                    }
                }
                "up" => {
                    let slug = args.first().ok_or(USAGE)?.clone();
                    let room = rooms::load_room(&slug)?;
                    let marker = home::root()
                        .join(".state")
                        .join("rooms")
                        .join(format!("{slug}.workspace"));
                    if let Ok(id) = std::fs::read_to_string(&marker)
                        && herdr
                            .call("workspace.get", serde_json::json!({ "workspace_id": id.trim() }))
                            .await
                            .is_ok()
                    {
                        println!("room {slug} already open in workspace {}", id.trim());
                        return Ok(());
                    }
                    let dir = rooms::room_dir(&slug);
                    let created = herdr
                        .workspace_create(&format!("💬 {}", room.name), &dir.to_string_lossy(), serde_json::json!({}))
                        .await?;
                    let id = created["workspace"]["workspace_id"].as_str().ok_or("workspace.create: no id")?;
                    let pane = created["root_pane"]["pane_id"].as_str().ok_or("workspace.create: no pane")?;
                    herdr.pane_run(pane, "tail -n 50 -f transcript.md").await?;
                    std::fs::create_dir_all(marker.parent().unwrap())?;
                    std::fs::write(&marker, id)?;
                    println!("room {slug} in workspace {id}");
                }
                _ => return Err(USAGE.into()),
            }
            Ok(())
        }
        "mcp" => mcp::serve(ctx(&mut args)?).await,
        "tool" => {
            let ctx = ctx(&mut args)?;
            let name = args.first().ok_or(USAGE)?.clone();
            let params: serde_json::Value =
                serde_json::from_str(args.get(1).map(String::as_str).unwrap_or("{}"))?;
            println!("{}", tools::call(&ctx, &name, &params).await?);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
