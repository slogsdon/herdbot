//! herdbot: Grok-Bot-style bots on herdr.

mod daemon;
mod grok;
mod home;
mod mcp;
mod route;
mod run;
mod tools;

use herdr_client::Client;

use home::Result;

const USAGE: &str = "usage:
  herdbot daemon                         deliver messages, watch subagents, fire schedules, sync Grok Bot
  herdbot import-grok                    one-way import of Grok Bot's roster (instructions)
  herdbot new <name> [--cwd DIR] [--class CLASS]
  herdbot list | runs
  herdbot up <bot> | down <bot>
  herdbot send <bot> <text…>             queue a prompt for the bot (needs the daemon)
  herdbot task <bot> <class> <prompt…>   start a subagent for the bot from the shell
  herdbot route <class> [--bot BOT]      show which harness:model a class resolves to
  herdbot mcp --bot BOT [--run ID]       stdio MCP server (launched by harnesses)
  herdbot tool <name> --bot BOT [--run ID] <json>   one tool call (pi extension)";

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
        "daemon" => daemon::run().await,
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
            let slug = home::slug(&name);
            if home::bot_dir(&slug).join("bot.toml").exists() {
                return Err(format!("bot {slug} already exists").into());
            }
            home::save_bot(
                &slug,
                &home::Bot {
                    name,
                    cwd,
                    class,
                    source: "local".into(),
                    ..Default::default()
                },
            )?;
            std::fs::write(home::bot_dir(&slug).join("BOT.md"), "")?;
            println!(
                "created {} (edit BOT.md for its instructions)",
                home::bot_dir(&slug).display()
            );
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
