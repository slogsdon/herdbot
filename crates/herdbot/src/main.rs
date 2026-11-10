//! herdbot: Grok-Bot-style bots on herdr.

mod adapters;
mod config;
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
mod shellenv;
mod tools;
mod template;

use std::io::{Read, Write};

use herdbot_core::{cooldown, history, home, snapshot};
use herdr_client::Client;

use home::Result;

const USAGE: &str = "usage:
  herdbot --version
  herdbot doctor [--json]               first-run and health checks (exit 1 if any fail)
  herdbot setup [--yes] [--only home|cli-link|service|restart-daemon]   fix what doctor reports
  herdbot uninstall [--purge]            remove the service and the herdbot link; --purge also deletes the home
  herdbot daemon                         deliver messages, watch subagents, fire schedules, sync Grok Bot
  herdbot service install|disable|enable|uninstall|status   control the launchd agent (from ~/.cargo/bin)
  herdbot import-grok                    one-way import of Grok Bot's roster (instructions)
  herdbot import-template <url> [--name NAME] [--cwd DIR] [--class CLASS] [--paste FILE] [--dry-run] [--json]
  herdbot new <name> [--cwd DIR] [--class CLASS] [--instructions TEXT] [--json]
  herdbot list | runs
  herdbot config list [--bot BOT] [--json]    supported settings, values and apply timing
  herdbot config get <key> [--bot BOT] [--json]   one setting
  herdbot config set <key> <value> [--bot BOT] [--json]   validate and write one setting
  herdbot config unset <key> [--bot BOT] [--json]         remove one setting (its default applies)
  herdbot config sessions --json                   herdr sessions and their reported availability
  herdbot session create <name> [--json]     create or start a named headless herdr session
  herdbot status --json                  one snapshot of bots, runs, cooldowns and blocked agents
  herdbot watch --json                   NDJSON: a snapshot line, then changes (for UIs)
  herdbot history <bot|room:NAME> --json [--before N] [--limit L]   a conversation, newest page first
  herdbot focus <agent>                  focus an agent's pane in herdr
  herdbot up <bot> | down <bot>
  herdbot card list [--json]            prompts waiting for you
  herdbot card answer <card-id> <option-id>   answer one, after checking it is still on screen
  herdbot send <bot> <text…>             queue a prompt for the bot (needs the daemon)
  herdbot collaborate --json              fan out one JSON request from stdin
  herdbot room new <name> --members a,b[,c] [--mode channel|round_robin|moderated] [--moderator BOT] [--json]
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

/// Consume creation options in order so an option's value is never parsed as another flag.
fn creation_options(args: &mut Vec<String>, value_flags: &[&str], bool_flags: &[&str]) -> Result<std::collections::HashMap<String, String>> {
    let mut options = std::collections::HashMap::new();
    let mut names = Vec::new();
    let mut input = args.iter();
    while let Some(arg) = input.next() {
        if bool_flags.contains(&arg.as_str()) {
            options.insert(arg.clone(), String::new());
        } else if value_flags.contains(&arg.as_str()) {
            let value = input.next().ok_or_else(|| format!("{arg} needs a value"))?;
            if arg != "--instructions" && (value_flags.contains(&value.as_str()) || bool_flags.contains(&value.as_str())) {
                return Err(format!("{arg} needs a value").into());
            }
            if options.insert(arg.clone(), value.clone()).is_some() {
                return Err(format!("{arg} was supplied twice; use it once").into());
            }
        } else if arg.starts_with("--") {
            return Err(format!("unknown option {arg:?}; check herdbot usage").into());
        } else {
            names.push(arg.clone());
        }
    }
    *args = names;
    Ok(options)
}

/// `--bot` / `--run`, falling back to the pane environment (cursor's shared mcp.json).
fn ctx(args: &mut Vec<String>) -> Result<tools::Ctx> {
    let bot = flag(args, "--bot")
        .or_else(|| std::env::var("HERDBOT_BOT").ok())
        .ok_or("--bot (or HERDBOT_BOT) is required")?;
    let run = flag(args, "--run").or_else(|| std::env::var("HERDBOT_RUN").ok());
    Ok(tools::Ctx { bot, run })
}

/// `herdbot new`. The name is every word left over, as before, so a quoted multi-word name works.
#[derive(Debug, PartialEq, Eq)]
struct NewBotArgs {
    name: String,
    cwd: Option<String>,
    class: Option<String>,
    /// Written verbatim; absent instructions keep the old empty `BOT.md`.
    instructions: Option<String>,
    json: bool,
}

fn parse_new_args(args: &mut Vec<String>) -> Result<NewBotArgs> {
    let mut options = creation_options(args, &["--cwd", "--class", "--instructions"], &["--json"])?;
    let json = options.remove("--json").is_some();
    let cwd = options.remove("--cwd");
    let class = options.remove("--class");
    let instructions = options.remove("--instructions");
    let name = args.join(" ");
    if name.trim().is_empty() {
        return Err("herdbot new needs a name: herdbot new <name> [--cwd DIR] [--class CLASS] [--instructions TEXT] [--json]".into());
    }
    Ok(NewBotArgs { name, cwd, class, instructions, json })
}

/// `herdbot new --json`: the bot's slug and the folder written.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct CreatedBot {
    slug: String,
    path: String,
}

/// Validate and write a new bot, the work `herdbot new` does once its instructions are in hand.
fn create_bot(new: &NewBotArgs, instructions: &str) -> Result<(String, std::path::PathBuf)> {
    let dir = setup::create_bot(&new.name, new.cwd.clone(), new.class.clone(), instructions)?;
    Ok((home::slug(&new.name), dir))
}

/// `herdbot room new --json`: the room's slug.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct CreatedRoom {
    slug: String,
}

/// `herdbot room new`.
#[derive(Debug, PartialEq, Eq)]
struct NewRoomArgs {
    name: String,
    members: Vec<String>,
    mode: rooms::Mode,
    moderator: Option<String>,
    json: bool,
}

fn parse_room_new_args(args: &mut Vec<String>) -> Result<NewRoomArgs> {
    let mut options = creation_options(args, &["--members", "--mode", "--moderator"], &["--json"])?;
    let json = options.remove("--json").is_some();
    let member_text = options.remove("--members")
        .ok_or("herdbot room new needs --members a,b[,c] (at least two existing bots)")?;
    if member_text.split(',').any(|member| member.trim().is_empty()) {
        return Err("--members contains an empty member; use at least two bot names, for example --members alpha,beta".into());
    }
    let members = member_text.split(',').map(|member| home::slug(member.trim())).collect();
    let mode = match options.remove("--mode").as_deref() {
        None | Some("channel") => rooms::Mode::Channel,
        Some("round_robin") => rooms::Mode::RoundRobin,
        Some("moderated") => rooms::Mode::Moderated,
        Some(other) => {
            return Err(format!(
                "unknown mode {other:?}; use channel, round_robin or moderated"
            )
            .into());
        }
    };
    let moderator = options.remove("--moderator").map(|m| home::slug(&m));
    let name = args.join(" ");
    if name.trim().is_empty() {
        return Err("herdbot room new needs a name: herdbot room new <name> --members a,b[,c] [--mode channel|round_robin|moderated] [--moderator BOT] [--json]".into());
    }
    Ok(NewRoomArgs { name, members, mode, moderator, json })
}

/// Validate and write a new room, the work `herdbot room new` does.
fn create_room(new: &NewRoomArgs) -> Result<(String, rooms::Room)> {
    let slug = home::slug(&new.name);
    if rooms::room_dir(&slug).join("room.toml").exists() {
        return Err(format!(
            "room {slug} already exists; run `herdbot room list` or pick another name"
        )
        .into());
    }
    let room = rooms::Room {
        name: new.name.clone(),
        members: new.members.clone(),
        mode: new.mode,
        moderator: new.moderator.clone(),
        ..Default::default()
    };
    rooms::validate(&room, &home::list_bots())?;
    rooms::save_room(&slug, &room)?;
    std::fs::write(rooms::room_dir(&slug).join("transcript.md"), "")?;
    Ok((slug, room))
}

fn main() {
    let command = std::env::args().nth(1).unwrap_or_default();
    let result = (|| -> Result<()> {
        if uses_herdr(&command) {
            configure_herdr_socket(!matches!(command.as_str(), "doctor" | "status" | "watch"))?;
        }
        tokio::runtime::Runtime::new()?.block_on(cli())
    })();
    if let Err(e) = result {
        eprintln!("herdbot: {e}");
        std::process::exit(1);
    }
}

fn uses_herdr(command: &str) -> bool {
    matches!(command, "daemon" | "up" | "down" | "focus" | "room" | "task" | "tool" | "mcp" | "doctor" | "card")
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
            if checks
                .iter()
                .any(|check| check.status == doctor::Status::Fail)
            {
                std::process::exit(1);
            }
            Ok(())
        }
        "setup" => {
            let only = flag(&mut args, "--only");
            let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
            setup::run(only, yes).await
        }
        "uninstall" => setup::uninstall(args.iter().any(|arg| arg == "--purge")),
        "daemon" => daemon::run().await,
        "service" => {
            let out = match args.first().map(String::as_str) {
                Some("install") => service::install()?,
                Some("disable") => service::disable()?,
                Some("enable") => service::enable()?,
                Some("uninstall") => service::uninstall()?,
                Some("status") => service::status()?,
                _ => return Err(USAGE.into()),
            };
            println!("{out}");
            Ok(())
        }
        "session" => {
            let (name, json) = parse_session_create_args(&args)?;
            let session = create_session(&name)?;
            if json {
                println!("{}", serde_json::to_string(&session)?);
            } else {
                println!(
                    "session {} running at {}",
                    session.name, session.socket_path
                );
            }
            Ok(())
        }
        "import-grok" => {
            println!("{}", grok::import()?);
            Ok(())
        }
        "import-template" => {
            let mut options = creation_options(&mut args, &["--name", "--cwd", "--class", "--paste"], &["--dry-run", "--json"])?;
            if args.len() != 1 {
                return Err("import-template needs one URL: https://x.ai/bot/<id> or https://x.ai/bot/marketplace/bots/<slug>".into());
            }
            let paste = options.remove("--paste");
            if options.remove("--dry-run").is_some() {
                println!("{}", serde_json::to_string_pretty(&template::preview(&args[0], paste.as_deref())?)?);
            } else {
                let (slug, dir) = template::create(
                    &args[0], options.remove("--name"), options.remove("--cwd"),
                    options.remove("--class"), paste.as_deref(),
                )?;
                if options.remove("--json").is_some() {
                    println!("{}", serde_json::to_string(&CreatedBot { slug, path: dir.display().to_string() })?);
                } else {
                    println!("imported {} (local copy; edit BOT.md for its instructions)", dir.display());
                }
            }
            Ok(())
        }
        "new" => {
            let new = parse_new_args(&mut args)?;
            let (slug, dir) = create_bot(&new, new.instructions.as_deref().unwrap_or_default())?;
            if new.json {
                let created = CreatedBot { slug, path: dir.display().to_string() };
                println!("{}", serde_json::to_string(&created)?);
            } else {
                println!(
                    "created {} (edit BOT.md for its instructions)",
                    dir.display()
                );
            }
            Ok(())
        }
        "config" => config::run(args),
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
            println!(
                "{}",
                serde_json::to_string(&history::history(target, before, limit)?)?
            );
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
        "card" => match args.first().map(String::as_str) {
            Some("list") => {
                let cards = herdbot_core::cards::read_cards();
                if args.iter().any(|arg| arg == "--json") {
                    println!("{}", serde_json::to_string(&cards)?);
                } else {
                    for card in cards {
                        let options = card
                            .options
                            .iter()
                            .map(|option| option.id.as_str())
                            .collect::<Vec<_>>()
                            .join(",");
                        println!("{}  {}  {}  [{}]", card.id, card.bot, card.title, options);
                    }
                }
                Ok(())
            }
            Some("answer") => {
                let id = args.get(1).ok_or(USAGE)?;
                let option = args.get(2).ok_or(USAGE)?;
                let card = herdbot_core::cards::read_cards()
                    .into_iter()
                    .find(|card| card.id == *id)
                    .ok_or_else(|| format!("no pending card {id}"))?;
                let tail = run::tail(&herdr, &card.agent, 60).await;
                let live = adapters::recognize(&tail, &card.bot, &card.agent);
                let keys = adapters::plan_answer(&card, option, id, live.as_ref())?;
                herdr.agent_send_keys(&card.agent, keys).await?;
                herdbot_core::cards::log_answer(&card, option, "cli")?;
                herdbot_core::cards::remove_card(&card.agent);
                println!("answered {option}");
                Ok(())
            }
            _ => Err(USAGE.into()),
        },
        "send" => {
            let bot = args.first().ok_or(USAGE)?.clone();
            // Only the daemon launches bots: it brings a stopped bot up when it finds its queue.
            home::load_bot(&bot)?;
            home::enqueue_from(&run::main_agent(&bot), "user", &rest(&args, 1))?;
            println!("queued for {bot} (the daemon delivers it)");
            Ok(())
        }
        "collaborate" => collaborate(&args),
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
                run::SpawnRequest {
                    class: &class,
                    explicit_model: None,
                    description: "shell task",
                    prompt: &prompt,
                    done_when: None,
                    wall_clock_limit: None,
                },
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
                    let new = parse_room_new_args(&mut args)?;
                    let (slug, room) = create_room(&new)?;
                    if new.json {
                        println!("{}", serde_json::to_string(&CreatedRoom { slug })?);
                    } else {
                        println!(
                            "created room {slug} ({:?}, {})",
                            room.mode,
                            room.members.join(", ")
                        );
                    }
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
                        let state = if rooms::load_state(&slug).paused {
                            "paused"
                        } else {
                            "active"
                        };
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
                            .call(
                                "workspace.get",
                                serde_json::json!({ "workspace_id": id.trim() }),
                            )
                            .await
                            .is_ok()
                    {
                        println!("room {slug} already open in workspace {}", id.trim());
                        return Ok(());
                    }
                    let dir = rooms::room_dir(&slug);
                    let created = herdr
                        .workspace_create(
                            &format!("💬 {}", room.name),
                            &dir.to_string_lossy(),
                            serde_json::json!({}),
                        )
                        .await?;
                    let id = created["workspace"]["workspace_id"]
                        .as_str()
                        .ok_or("workspace.create: no id")?;
                    let pane = created["root_pane"]["pane_id"]
                        .as_str()
                        .ok_or("workspace.create: no pane")?;
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

#[derive(serde::Deserialize)]
struct CollaborationRequest {
    request_id: String,
    text: String,
    targets: Vec<CollaborationTarget>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, Clone, PartialEq, Eq, Hash)]
struct CollaborationTarget {
    kind: String,
    slug: String,
}

#[derive(serde::Serialize)]
struct CollaborationResult {
    version: u8,
    request_id: String,
    results: Vec<CollaborationTargetResult>,
}

#[derive(serde::Serialize)]
struct CollaborationTargetResult {
    target: CollaborationTarget,
    status: &'static str,
    error: Option<String>,
}

fn collaboration_target_id(request_id: &str, target: &CollaborationTarget) -> String {
    format!("collab:{request_id}:{}:{}", target.kind, target.slug)
}

fn unique_collaboration_targets(targets: Vec<CollaborationTarget>) -> Vec<CollaborationTarget> {
    let mut seen = std::collections::HashSet::new();
    targets.into_iter().filter(|target| seen.insert(target.clone())).collect()
}

fn collaborate(args: &[String]) -> Result<()> {
    if args != ["--json"] { return Err(USAGE.into()); }
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let request: CollaborationRequest = serde_json::from_str(&input)?;
    if request.request_id.is_empty() || request.request_id.len() > 80 ||
        !request.request_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("request_id must be 1-80 ASCII letters, digits, '_' or '-'".into());
    }
    if request.text.trim().is_empty() || request.targets.is_empty() {
        return Err("text and at least one target are required".into());
    }
    let mut results = Vec::new();
    let explicitly_targeted_bots: Vec<String> = request.targets.iter()
        .filter(|target| target.kind == "bot")
        .map(|target| target.slug.clone())
        .collect();
    for target in unique_collaboration_targets(request.targets) {
        let stable_id = collaboration_target_id(&request.request_id, &target);
        let slug = home::slug(&target.slug);
        let outcome = match target.kind.as_str() {
            "bot" if !slug.is_empty() && slug == target.slug => {
                home::load_bot(&slug).and_then(|_| home::enqueue_with_id(&run::main_agent(&slug), stable_id, "user", &request.text))
            }
            "room" if !slug.is_empty() && slug == target.slug => {
                rooms::load_room(&slug).and_then(|_| rooms::append_with_id(&slug, "you", &request.text, Some(&stable_id), true, &explicitly_targeted_bots))
            }
            "bot" | "room" => Err(format!("invalid or unknown target {:?}", target.slug).into()),
            kind => Err(format!("unknown target kind {kind:?}").into()),
        };
        match outcome {
            Ok(()) => results.push(CollaborationTargetResult { target, status: "queued", error: None }),
            Err(error) => results.push(CollaborationTargetResult { target, status: "failed", error: Some(error.to_string()) }),
        }
    }
    println!("{}", serde_json::to_string(&CollaborationResult { version: 1, request_id: request.request_id, results })?);
    Ok(())
}

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct ReadySession {
    name: String,
    socket_path: String,
}

fn parse_session_create_args(args: &[String]) -> Result<(String, bool)> {
    if args.first().map(String::as_str) != Some("create")
        || !(args.len() == 2 || (args.len() == 3 && args[2] == "--json"))
    {
        return Err(USAGE.into());
    }
    Ok((args[1].clone(), args.len() == 3))
}

fn validate_session_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "default"
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(
            "session name must be 1-64 ASCII letters, digits, '_' or '-', and cannot be 'default'"
                .into(),
        );
    }
    Ok(())
}

fn ready_session(sessions: &serde_json::Value, name: &str) -> Option<ReadySession> {
    sessions["sessions"].as_array()?.iter().find_map(|session| {
        (session["name"].as_str() == Some(name) && session["running"].as_bool() == Some(true))
            .then(|| {
                session["socket_path"]
                    .as_str()
                    .filter(|path| !path.is_empty())
            })
            .flatten()
            .map(|socket_path| ReadySession {
                name: name.to_owned(),
                socket_path: socket_path.to_owned(),
            })
    })
}

fn create_session(name: &str) -> Result<ReadySession> {
    create_session_with(
        name,
        || herdr_client::list_sessions().map_err(|error| error.to_string()),
        |name| {
            std::process::Command::new("herdr")
                .args(["--session", name, "server"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|error| error.to_string())
        },
        std::thread::sleep,
    )
}

fn create_session_with<L, S, W>(
    name: &str,
    mut list: L,
    mut spawn: S,
    mut wait: W,
) -> Result<ReadySession>
where
    L: FnMut() -> std::result::Result<serde_json::Value, String>,
    S: FnMut(&str) -> std::result::Result<(), String>,
    W: FnMut(std::time::Duration),
{
    validate_session_name(name)?;
    let sessions = list().map_err(|error| format!("could not list Herdr sessions: {error}"))?;
    if let Some(ready) = ready_session(&sessions, name) {
        return Ok(ready);
    }
    spawn(name).map_err(|error| format!("could not start Herdr session {name:?}: {error}"))?;
    for _ in 0..40 {
        wait(std::time::Duration::from_millis(250));
        if let Ok(sessions) = list()
            && let Some(ready) = ready_session(&sessions, name)
        {
            return Ok(ready);
        }
    }
    Err(
        format!("Herdr session {name:?} did not report running with socket_path within 10 seconds")
            .into(),
    )
}

/// Keep the default socket behavior and explicit operator override unchanged.
fn configure_herdr_socket(bind: bool) -> Result<()> {
    let default_socket = home::home_dir().join(".config/herdr/herdr.sock");
    let explicit = std::env::var_os("HERDR_SOCKET");
    let socket = if let Some(socket) = explicit.as_ref() {
        std::path::PathBuf::from(socket)
    } else {
        let selected = config::get(&home::root(), None, "herdr.session")?;
        match selected.value.as_str().unwrap_or("default") {
            "default" => default_socket.clone(),
            session => herdr_client::session_socket(session)?,
        }
    };
    if bind {
        bind_session_socket(&home::root(), &socket.to_string_lossy(), &default_socket.to_string_lossy())?;
    }
    // This runs before the Tokio runtime starts any worker threads.
    unsafe { std::env::set_var("HERDR_SOCKET", socket) };
    Ok(())
}

/// Workspace ids belong to one server; a socket switch must never recreate them elsewhere.
fn bind_session_socket(root: &std::path::Path, socket: &str, legacy: &str) -> Result<()> {
    let state = root.join(".state");
    let binding = state.join("herdr-socket");
    let previous = match std::fs::read_to_string(&binding) {
        Ok(previous) => previous,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => legacy.to_owned(),
        Err(error) => return Err(error.into()),
    };
    if previous != socket {
        if binding.exists() || std::os::unix::net::UnixStream::connect(state.join("daemon.lock")).is_ok() {
            return Err("This HERDBOT_HOME is bound to another Herdr socket; restore the previous session/socket or use a separate HERDBOT_HOME. No workspaces were migrated.".into());
        }
        for directory in [state.clone(), state.join("rooms")] {
            let entries = match std::fs::read_dir(directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                if entry?.path().extension().is_some_and(|extension| extension == "json" || extension == "workspace") {
                    return Err("Herdr socket changed while workspace state exists; restore the previous session/socket or use a separate HERDBOT_HOME. No workspaces were migrated.".into());
                }
            }
        }
    }
    std::fs::create_dir_all(state)?;
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&binding) {
        Ok(mut file) => file.write_all(socket.as_bytes())?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::read_to_string(binding)? != socket {
                return Err("Herdr socket binding changed concurrently; retry with the original session/socket".into());
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Run `f` with `HERDBOT_HOME` pointing at a fresh temp directory. Serialised because the
/// environment is process-global, so every test in this crate that needs a home goes through it.
#[cfg(test)]
fn with_temp_home<F: FnOnce() -> R, R>(f: F) -> R {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let tmp = std::env::temp_dir().join(format!("herdbot-create-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let old = std::env::var_os("HERDBOT_HOME");
    // SAFETY: the lock above keeps every home-swapping test in this process apart.
    unsafe { std::env::set_var("HERDBOT_HOME", &tmp) };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    match old {
        Some(old) => unsafe { std::env::set_var("HERDBOT_HOME", old) },
        None => unsafe { std::env::remove_var("HERDBOT_HOME") },
    }
    let _ = std::fs::remove_dir_all(&tmp);
    match result {
        Ok(r) => r,
        Err(e) => std::panic::resume_unwind(e),
    }
}

#[cfg(test)]
mod create_tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn import_options_keep_values_literal_and_reject_missing_or_unknown_options() {
        let values = &["--name", "--cwd", "--class", "--paste"];
        let booleans = &["--dry-run", "--json"];
        let mut input = args(&["https://x.ai/bot/id", "--paste", "/tmp/paste file", "--dry-run", "--json"]);
        let options = creation_options(&mut input, values, booleans).unwrap();
        assert_eq!(input, ["https://x.ai/bot/id"]);
        assert_eq!(options["--paste"], "/tmp/paste file");
        assert!(options.contains_key("--dry-run"));
        for input in [vec!["--paste"], vec!["--paste", "--dry-run"], vec!["--bad"], vec!["--name", "A", "--name", "B"]] {
            assert!(creation_options(&mut args(&input), values, booleans).is_err());
        }
        assert!(creation_options(&mut args(&["--dry-run"]), &["--instructions"], &["--json"]).is_err());
    }

    #[test]
    fn new_args_take_flags_anywhere_and_keep_a_multi_word_name() {
        let mut all = args(&[
            "My Bot",
            "--json",
            "--cwd",
            "/tmp",
            "--class",
            "explore",
            "--instructions",
            "Be terse.",
        ]);
        assert_eq!(
            parse_new_args(&mut all).unwrap(),
            NewBotArgs {
                name: "My Bot".into(),
                cwd: Some("/tmp".into()),
                class: Some("explore".into()),
                instructions: Some("Be terse.".into()),
                json: true,
            }
        );
        // Only flags are consumed; the name stays, as it always has for the leftover words.
        assert_eq!(all, ["My Bot"]);
        // `--json` never eats the word after it.
        assert_eq!(parse_new_args(&mut args(&["--json", "helper"])).unwrap().name, "helper");
        let error = parse_new_args(&mut args(&["--json"])).unwrap_err().to_string();
        assert!(error.contains("needs a name"), "{error}");
    }

    #[test]
    fn creation_options_preserve_literal_instructions_and_reject_incomplete_flags() {
        for text in ["--json", "-", "line one\nline two", "--mode"] {
            let parsed = parse_new_args(&mut args(&["helper", "--instructions", text, "--json"])).unwrap();
            assert_eq!(parsed.instructions.as_deref(), Some(text));
            assert!(parsed.json);
        }
        for input in [vec!["helper", "--cwd"], vec!["helper", "--class", "--json"], vec!["helper", "--instructions"], vec!["helper", "--typo"]] {
            assert!(parse_new_args(&mut args(&input)).is_err(), "{input:?}");
        }
        assert!(parse_room_new_args(&mut args(&["crew", "--members", "a,b", "--mode"])).unwrap_err().to_string().contains("needs a value"));
    }

    #[test]
    fn room_args_default_to_channel_and_reject_bad_input() {
        let mut plain = args(&["Crew", "--members", "ann, bob,cy", "--json"]);
        assert_eq!(
            parse_room_new_args(&mut plain).unwrap(),
            NewRoomArgs {
                name: "Crew".into(),
                members: vec!["ann".into(), "bob".into(), "cy".into()],
                mode: rooms::Mode::Channel,
                moderator: None,
                json: true,
            }
        );
        let mut rr = args(&["Crew", "--members", "ann,bob", "--mode", "round_robin"]);
        assert_eq!(parse_room_new_args(&mut rr).unwrap().mode, rooms::Mode::RoundRobin);
        let mut moderated = args(&[
            "Crew", "--members", "ann,bob", "--mode", "moderated", "--moderator", "Ann",
        ]);
        let parsed = parse_room_new_args(&mut moderated).unwrap();
        assert_eq!(parsed.mode, rooms::Mode::Moderated);
        assert_eq!(parsed.moderator.as_deref(), Some("ann"));

        let error = parse_room_new_args(&mut args(&["Crew", "--members", "ann,bob", "--mode", "debate"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("channel, round_robin or moderated"), "{error}");
        for members in ["", "ann,", ",bob", "ann, ,bob"] {
            let error = parse_room_new_args(&mut args(&["Crew", "--members", members])).unwrap_err().to_string();
            assert!(error.contains("empty member"), "{error}");
        }
        let error = parse_room_new_args(&mut args(&["Crew"])).unwrap_err().to_string();
        assert!(error.contains("--members"), "{error}");
        let error = parse_room_new_args(&mut args(&["--members", "ann,bob"])).unwrap_err().to_string();
        assert!(error.contains("needs a name"), "{error}");
    }

    #[test]
    fn creating_a_bot_and_a_room_writes_files_and_refuses_bad_input() {
        with_temp_home(|| {
            let bot = parse_new_args(&mut args(&["Helper Bot", "--class", "explore"])).unwrap();
            let (slug, dir) = create_bot(&bot, "Be terse.").unwrap();
            assert_eq!(slug, "helper-bot");
            assert_eq!(dir, home::root().join("helper-bot"));
            assert_eq!(std::fs::read_to_string(dir.join("BOT.md")).unwrap(), "Be terse.");
            let toml = std::fs::read_to_string(dir.join("bot.toml")).unwrap();
            assert!(toml.contains("class = \"explore\""), "{toml}");
            let json = serde_json::to_string(&CreatedBot { slug: slug.clone(), path: dir.display().to_string() }).unwrap();
            assert_eq!(json, format!("{{\"slug\":\"helper-bot\",\"path\":\"{}\"}}", dir.display()));

            // A duplicate is refused with the file to edit instead of a second bot.
            let error = create_bot(&bot, "").unwrap_err().to_string();
            assert!(error.contains("bot helper-bot already exists"), "{error}");
            assert!(error.contains("BOT.md"), "{error}");

            // A name that would take one of herdbot's own home folders is refused.
            let reserved = parse_new_args(&mut args(&["Rooms"])).unwrap();
            assert!(create_bot(&reserved, "").unwrap_err().to_string().contains("reserved"));

            let partial = home::root().join("partial");
            std::fs::create_dir_all(&partial).unwrap();
            std::fs::write(partial.join("BOT.md"), "Keep existing instructions").unwrap();
            let partial_args = parse_new_args(&mut args(&["Partial"])).unwrap();
            assert!(create_bot(&partial_args, "overwrite").is_err());
            assert!(!partial.join("bot.toml").exists());
            assert_eq!(std::fs::read_to_string(partial.join("BOT.md")).unwrap(), "Keep existing instructions");

            let bad_cwd = parse_new_args(&mut args(&["Other", "--cwd", "/nope/not/here"])).unwrap();
            assert!(create_bot(&bad_cwd, "").unwrap_err().to_string().contains("not a directory"));

            create_bot(&parse_new_args(&mut args(&["Beta"])).unwrap(), "").unwrap();
            let room = parse_room_new_args(&mut args(&[
                "Crew", "--members", "helper-bot,beta", "--mode", "moderated", "--moderator", "helper-bot", "--json",
            ]))
            .unwrap();
            let (room_slug, saved) = create_room(&room).unwrap();
            assert_eq!(room_slug, "crew");
            assert_eq!(saved.moderator.as_deref(), Some("helper-bot"));
            assert_eq!(serde_json::to_string(&CreatedRoom { slug: room_slug }).unwrap(), "{\"slug\":\"crew\"}");
            assert!(rooms::room_dir("crew").join("room.toml").exists());
            assert!(rooms::room_dir("crew").join("transcript.md").exists());

            let error = create_room(&room).unwrap_err().to_string();
            assert!(error.contains("room crew already exists"), "{error}");

            let unmoderated = parse_room_new_args(&mut args(&[
                "Debate", "--members", "helper-bot,beta", "--mode", "moderated",
            ]))
            .unwrap();
            assert!(create_room(&unmoderated).unwrap_err().to_string().contains("--moderator"));

            let stranger = parse_room_new_args(&mut args(&["Team", "--members", "helper-bot,zed"])).unwrap();
            let error = create_room(&stranger).unwrap_err().to_string();
            assert!(error.contains("no bot \"zed\""), "{error}");
            assert!(error.contains("helper-bot"), "{error}");

            let solo = parse_room_new_args(&mut args(&["Solo", "--members", "helper-bot"])).unwrap();
            assert!(create_room(&solo).unwrap_err().to_string().contains("at least two members"));
            // None of the rejected rooms left a folder behind.
            for slug in ["debate", "team", "solo"] {
                assert!(!rooms::room_dir(slug).join("room.toml").exists(), "{slug} was written");
            }
        });
    }
}

#[cfg(test)]
mod collaboration_tests {
    use super::*;

    #[test]
    fn targets_are_deduplicated_by_type_and_have_stable_ids() {
        let bot = CollaborationTarget { kind: "bot".into(), slug: "alpha".into() };
        let room = CollaborationTarget { kind: "room".into(), slug: "alpha".into() };
        assert_eq!(unique_collaboration_targets(vec![bot.clone(), bot.clone(), room.clone()]), [bot.clone(), room]);
        assert_eq!(collaboration_target_id("r-1", &bot), "collab:r-1:bot:alpha");
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    fn running(name: &str, socket_path: &str) -> serde_json::Value {
        serde_json::json!({"sessions":[{"name":name,"running":true,"socket_path":socket_path}]})
    }

    #[test]
    fn switching_socket_rejects_existing_bot_and_room_state_without_changes() {
        let root = std::env::temp_dir().join(format!("herdbot-session-guard-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".state/rooms")).unwrap();
        for state in [".state/helper.json", ".state/rooms/team.json", ".state/rooms/team.workspace"] {
            let path = root.join(state);
            std::fs::write(&path, "{\"workspace_id\":\"w1\"}").unwrap();
            assert!(bind_session_socket(&root, "/named.sock", "/default.sock").is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"workspace_id\":\"w1\"}");
            std::fs::remove_file(path).unwrap();
        }
        bind_session_socket(&root, "/named.sock", "/default.sock").unwrap();
        assert!(bind_session_socket(&root, "/other.sock", "/default.sock").is_err());
        std::fs::write(root.join(".state/helper.json"), "state").unwrap();
        bind_session_socket(&root, "/named.sock", "/default.sock").unwrap();
        assert!(bind_session_socket(&root, "/default.sock", "/default.sock").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn socket_commands_include_card_answers_but_exclude_lifecycle_and_config() {
        assert!(uses_herdr("card"));
        assert!(uses_herdr("daemon"));
        for command in ["config", "session", "service", "setup", "--version", "status", "watch"] {
            assert!(!uses_herdr(command));
        }
    }

    #[test]
    fn validates_named_sessions_without_allowing_default() {
        for name in [
            "",
            "default",
            "bad name",
            "../escape",
            "x".repeat(65).as_str(),
        ] {
            assert!(validate_session_name(name).is_err(), "accepted {name:?}");
        }
        assert!(validate_session_name("work_2").is_ok());
    }

    #[test]
    fn validates_session_create_arguments() {
        assert_eq!(
            parse_session_create_args(&["create".into(), "work".into()]).unwrap(),
            ("work".into(), false)
        );
        assert_eq!(
            parse_session_create_args(&["create".into(), "work".into(), "--json".into()]).unwrap(),
            ("work".into(), true)
        );
        for args in [
            vec![],
            vec!["list".into()],
            vec!["create".into()],
            vec!["create".into(), "work".into(), "--other".into()],
            vec![
                "create".into(),
                "work".into(),
                "--json".into(),
                "extra".into(),
            ],
        ] {
            assert!(
                parse_session_create_args(&args).is_err(),
                "accepted {args:?}"
            );
        }
    }

    #[test]
    fn running_session_is_idempotent_and_does_not_spawn() {
        let result = create_session_with(
            "work",
            || Ok(running("work", "/tmp/work.sock")),
            |_| panic!("running session must not be spawned"),
            |_| {},
        )
        .unwrap();
        assert_eq!(
            result,
            ReadySession {
                name: "work".into(),
                socket_path: "/tmp/work.sock".into()
            }
        );
    }

    #[test]
    fn launch_failure_is_reported() {
        let error = create_session_with(
            "work",
            || Ok(serde_json::json!({"sessions":[]})),
            |_| Err("executable missing".into()),
            |_| {},
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("could not start Herdr session"));
        assert!(error.contains("executable missing"));
    }

    #[test]
    fn success_waits_for_running_session_with_socket_path() {
        let mut polls = 0;
        let mut launches = Vec::new();
        let result = create_session_with(
            "work",
            || {
                polls += 1;
                if polls == 1 {
                    Ok(serde_json::json!({"sessions":[]}))
                } else {
                    Ok(running("work", "/tmp/work.sock"))
                }
            },
            |name| {
                launches.push(name.to_owned());
                Ok(())
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(launches, ["work"]);
        assert_eq!(polls, 2);
        assert_eq!(result.socket_path, "/tmp/work.sock");
    }

    #[test]
    fn running_without_socket_is_not_reported_as_ready() {
        let error = create_session_with(
            "work",
            || Ok(serde_json::json!({"sessions":[{"name":"work","running":true}]})),
            |_| Ok(()),
            |_| {},
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("did not report running with socket_path"));
    }
}
