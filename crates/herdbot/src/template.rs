//! Public Grok template preview and explicit, locally owned copies. No private API or scripts.

use crate::{
    home::{self, Result, Schedule},
    setup,
};
use serde::Serialize;
use std::{fs, path::PathBuf, process::Command};

const CHANGED: &str = "the template page has changed; paste the template instead: use New Bot → Instructions, or herdbot new NAME --instructions TEXT, for a manual copy";
const LIMIT: usize = 5 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Preview {
    pub name: String,
    pub description: String,
    pub memories: Vec<String>,
    pub components: Vec<String>,
    pub not_imported: Vec<String>,
    pub template_url: String,
    pub template_fetched_at: String,
    pub skills: Vec<Skill>,
    pub routines: Vec<Routine>,
    pub instructions: Vec<String>,
    pub integrations: Vec<String>,
    #[serde(skip)]
    labels: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub body: String,
}
#[derive(Debug, Serialize)]
pub struct Routine {
    pub name: String,
    pub cron: Option<String>,
    pub body: String,
}

fn marketplace(url: &str) -> Result<bool> {
    let path = url.strip_prefix("https://x.ai/bot/").ok_or("use https://x.ai/bot/<id> or https://x.ai/bot/marketplace/bots/<slug>; other hosts and schemes are not supported")?;
    let (marketplace, identifier) = path
        .strip_prefix("marketplace/bots/")
        .map_or((false, path), |id| (true, id));
    if identifier.is_empty()
        || !identifier
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        || (!marketplace && ["marketplace", "guides"].contains(&identifier))
    {
        return Err("use a single template URL: https://x.ai/bot/<id> or https://x.ai/bot/marketplace/bots/<slug> (no query, fragment, port or extra path)".into());
    }
    Ok(marketplace)
}

fn curl_args(url: &str) -> Vec<String> {
    [
        "--disable",
        "--proto",
        "=https",
        "--max-redirs",
        "0",
        "--max-time",
        "20",
        "--max-filesize",
        "5242880",
        "-A",
        concat!("herdbot/", env!("CARGO_PKG_VERSION")),
        "-f",
        "-sS",
        "--write-out",
        "\n%{http_code}",
        "--url",
        url,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn fetch(url: &str) -> Result<String> {
    marketplace(url)?;
    let output = Command::new("/usr/bin/curl")
        .args(curl_args(url))
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "template fetch failed: {}; open the template and paste its contents instead",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let text = String::from_utf8(output.stdout)?;
    let (body, status) = text.rsplit_once('\n').ok_or(CHANGED)?;
    // curl does not reject 3xx without -L; require a successful page explicitly.
    if status != "200" {
        return Err(format!("template fetch returned HTTP {status}; redirects are not followed; use the original x.ai template URL").into());
    }
    if body.len() > LIMIT {
        return Err("template page exceeds 5 MiB".into());
    }
    Ok(body.to_owned())
}

fn elements<'a>(html: &'a str, tag: &str) -> Result<Vec<(&'a str, &'a str)>> {
    let mut rest = html;
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    while let Some(index) = rest.find(&open) {
        rest = &rest[index + open.len()..];
        if !rest.starts_with([' ', '>']) {
            continue;
        }
        let (attributes, body) = rest.split_once('>').ok_or(CHANGED)?;
        let (body, tail) = body.split_once(&close).ok_or(CHANGED)?;
        out.push((attributes, body));
        rest = tail;
    }
    Ok(out)
}

fn text(html: &str) -> Result<String> {
    let mut output = String::new();
    let mut rest = html;
    while !rest.is_empty() {
        if rest.starts_with('<') {
            let (tag, tail) = rest.split_once('>').ok_or(CHANGED)?;
            if tag.starts_with("<br") || tag == "</p" {
                output.push('\n');
            }
            rest = tail;
        } else if rest.starts_with('&') {
            let (entity, tail) = rest.split_once(';').ok_or(CHANGED)?;
            let decoded = match entity {
                "&amp" => '&',
                "&lt" => '<',
                "&gt" => '>',
                "&quot" => '"',
                "&apos" | "&#x27" | "&#39" => '\'',
                "&nbsp" => ' ',
                _ => {
                    let number = entity
                        .strip_prefix("&#x")
                        .and_then(|s| u32::from_str_radix(s, 16).ok())
                        .or_else(|| entity.strip_prefix("&#").and_then(|s| s.parse().ok()));
                    number.and_then(char::from_u32).ok_or(CHANGED)?
                }
            };
            output.push(decoded);
            rest = tail;
        } else {
            let character = rest.chars().next().ok_or(CHANGED)?;
            output.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    Ok(output.trim().to_owned())
}

pub(crate) fn parse(url: &str, html: &str) -> Result<Preview> {
    let is_marketplace = marketplace(url)?;
    // shortcut: only the recorded prerendered layout is supported; fail closed when it changes.
    let start = html.find("<h1").ok_or(CHANGED)?;
    let rendered = html[start..].split("<script").next().ok_or(CHANGED)?;
    let body = rendered
        .split_once("</h1>")
        .map(|(_, tail)| tail)
        .ok_or(CHANGED)?;
    let headings = elements(rendered, "h1")?;
    if headings.len() != 1 {
        return Err(CHANGED.into());
    }
    let name = text(headings[0].1)?;
    let description = elements(body, "p")?
        .into_iter()
        .find(|(attributes, _)| {
            if is_marketplace {
                attributes.contains("text-[15px] leading-6")
            } else {
                attributes.contains("title=")
            }
        })
        .map(|(_, value)| text(value))
        .transpose()?
        .ok_or(CHANGED)?;
    if name.is_empty() || description.is_empty() {
        return Err(CHANGED.into());
    }
    let mut labels = Vec::new();
    let mut memories = Vec::new();
    if is_marketplace {
        let panel = body.split_once("aria-current=\"true\"").ok_or(CHANGED)?.1;
        let (tabs, content) = panel.split_once("</nav>").ok_or(CHANGED)?;
        for (_, span) in elements(tabs, "span")? {
            let label = text(span)?;
            if ["memories", "skills", "routines", "integrations"].contains(&label.as_str()) {
                labels.push(label);
            }
        }
        if labels.first().map(String::as_str) != Some("memories") {
            return Err(CHANGED.into());
        }
        let (list, _) = content.split_once("</ul>").ok_or(CHANGED)?;
        if !list.contains("<ul") {
            return Err(CHANGED.into());
        }
        for (_, item) in elements(list, "li")? {
            let value = text(item)?;
            if !value.is_empty() {
                memories.push(value);
            }
        }
    } else if !body.contains("grokbot://app/v1/bot-template?id=") {
        return Err(CHANGED.into());
    }
    let components: Vec<_> = labels
        .iter()
        .filter(|label| label.as_str() != "memories")
        .cloned()
        .collect();
    Ok(Preview {
        name,
        description,
        memories,
        not_imported: components.clone(),
        components,
        template_url: url.into(),
        template_fetched_at: chrono::Utc::now().to_rfc3339(),
        skills: vec![],
        routines: vec![],
        instructions: vec![],
        integrations: vec![],
        labels,
    })
}

fn field(body: &str, prefix: &str) -> (Option<String>, String) {
    match body.split_once('\n') {
        Some((line, tail)) if line.starts_with(prefix) => {
            (Some(line[prefix.len()..].trim().into()), tail.trim().into())
        }
        _ if body.starts_with(prefix) => (Some(body[prefix.len()..].trim().into()), String::new()),
        _ => (None, body.into()),
    }
}

fn paste(preview: &mut Preview, input: &str) -> Result<()> {
    if input.trim().is_empty() {
        return Ok(());
    }
    if input.contains("```")
        || input.contains("~~~")
        || input.contains("#!")
        || input.contains("-----BEGIN ")
    {
        return Err("paste plain instructions, memories, skills or routines, never secrets, code or scripts".into());
    }
    let mut sections: Vec<(String, String)> = Vec::new();
    for line in input.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            sections.push((heading.trim().into(), String::new()));
        } else if let Some((_, body)) = sections.last_mut() {
            body.push_str(line);
            body.push('\n');
        } else if !line.trim().is_empty() {
            return Err("paste must start with an explicit ## Memories, ## Instructions, ## Skill: Name or ## Routine: Name heading".into());
        }
    }
    for (heading, body) in sections {
        let body = body.trim().to_owned();
        if body.is_empty() {
            return Err(format!("pasted section {heading:?} is empty").into());
        }
        if let Some(name) = heading.strip_prefix("Skill: ") {
            if name.trim().is_empty() {
                return Err("a skill needs a name".into());
            }
            let (description, body) = field(&body, "Description:");
            if body.is_empty() {
                return Err("a skill needs its prose instruction".into());
            }
            let slug = home::slug(name);
            if preview
                .skills
                .iter()
                .any(|skill| home::slug(&skill.name) == slug)
            {
                return Err(format!("duplicate skill slug {slug}").into());
            }
            preview.skills.push(Skill {
                name: name.into(),
                description: description.unwrap_or_else(|| name.into()),
                body,
            });
        } else if let Some(name) = heading.strip_prefix("Routine: ") {
            if name.trim().is_empty() {
                return Err("a routine needs a name".into());
            }
            let (cron, body) = field(&body, "Cron:");
            if let Some(cron) = &cron {
                cron.parse::<croner::Cron>()
                    .map_err(|error| format!("invalid pasted cron {cron:?}: {error}"))?;
            }
            if body.is_empty() {
                return Err("a routine needs its prose instruction".into());
            }
            preview.routines.push(Routine {
                name: name.into(),
                cron,
                body,
            });
        } else {
            match heading.as_str() {
            "Memories" => preview.memories.push(body), "Instructions" => preview.instructions.push(body), "Integrations" => preview.integrations.push(body),
            _ => return Err(format!("unsupported paste heading {heading:?}; use ## Memories, ## Instructions, ## Skill: Name, ## Routine: Name or ## Integrations").into()),
        }
        }
    }
    preview.not_imported.retain(|label| match label.as_str() {
        "skills" => preview.skills.is_empty(),
        "routines" => preview.routines.is_empty(),
        "integrations" => preview.integrations.is_empty(),
        _ => true,
    });
    Ok(())
}

pub fn preview(url: &str, paste_file: Option<&str>) -> Result<Preview> {
    let mut preview = parse(url, &fetch(url)?)?;
    if let Some(path) = paste_file {
        if fs::metadata(path)?.len() > LIMIT as u64 {
            return Err("pasted template exceeds 5 MiB".into());
        }
        paste(&mut preview, &fs::read_to_string(path)?)?;
    }
    Ok(preview)
}

pub fn create(
    url: &str,
    name: Option<String>,
    cwd: Option<String>,
    class: Option<String>,
    paste_file: Option<&str>,
) -> Result<(String, PathBuf)> {
    write(preview(url, paste_file)?, name, cwd, class)
}

pub(crate) fn write(
    preview: Preview,
    name: Option<String>,
    cwd: Option<String>,
    class: Option<String>,
) -> Result<(String, PathBuf)> {
    let name = name.unwrap_or_else(|| preview.name.clone());
    let slug = setup::bot_slug(&name)?;
    let mut instructions = preview.description.clone();
    for value in preview.memories.iter().chain(&preview.instructions) {
        instructions.push_str("\n\n");
        instructions.push_str(value);
    }
    instructions.push_str(&format!(
        "\n\n## Template provenance\nImported from {} (Grok), {}. Local copy.\n",
        preview.template_url, preview.template_fetched_at
    ));
    if !preview.skills.is_empty() {
        instructions.push_str(&format!(
            "\nSkills: {}\n",
            preview
                .skills
                .iter()
                .map(|skill| format!("skills/{}.md", home::slug(&skill.name)))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !preview.routines.is_empty() {
        instructions.push_str("\n## Routines\n");
        for routine in &preview.routines {
            instructions.push_str(&format!("\n### {}\n{}\n", routine.name, routine.body));
        }
    }
    if !preview.integrations.is_empty() {
        instructions.push_str(&format!(
            "\nIntegrations (reinstall separately): {}\n",
            preview.integrations.join("; ")
        ));
    } else if preview.labels.iter().any(|label| label == "integrations") {
        instructions.push_str("\nIntegrations: the template lists integrations but does not expose their names; open it in Grok Bot and reinstall them separately.\n");
    }
    let dir = home::bot_dir(&slug);
    fs::create_dir_all(home::root())?;
    fs::create_dir(&dir).map_err(|error| {
        format!(
            "cannot reserve {}: {error}; pick another template name with --name",
            dir.display()
        )
    })?;
    let result = (|| -> Result<()> {
        setup::create_bot(&name, cwd, class, &instructions)?;
        let mut bot = home::load_bot(&slug)?;
        bot.title = preview.description;
        bot.template_url = Some(preview.template_url);
        bot.template_source = Some("grok".into());
        bot.template_fetched_at = Some(preview.template_fetched_at);
        bot.template_components = preview.labels;
        bot.schedules = preview
            .routines
            .iter()
            .filter_map(|routine| {
                routine.cron.as_ref().map(|cron| Schedule {
                    name: routine.name.clone(),
                    cron: cron.clone(),
                    prompt: routine.body.clone(),
                    enabled: false,
                })
            })
            .collect();
        home::save_bot(&slug, &bot)?;
        if !preview.memories.is_empty() {
            fs::write(
                dir.join(format!(
                    "memory/imported-{}.md",
                    chrono::Utc::now().format("%Y-%m-%d")
                )),
                preview.memories.join("\n\n"),
            )?;
        }
        for skill in preview.skills {
            // JSON strings are valid YAML quoted scalars, including embedded newlines and colons.
            let contents = format!(
                "---\nname: {}\ndescription: {}\n---\n\n{}\n",
                serde_json::to_string(&skill.name)?,
                serde_json::to_string(&skill.description)?,
                skill.body
            );
            fs::write(
                dir.join(format!("skills/{}.md", home::slug(&skill.name))),
                contents,
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(cleanup) = fs::remove_dir_all(&dir) {
            return Err(format!(
                "template import failed: {error}; could not remove incomplete import {}: {cleanup}",
                dir.display()
            )
            .into());
        }
        return Err(error);
    }
    Ok((slug, dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    const SHARE: &str = "https://x.ai/bot/p7Gh6HIrfv4AGzIow6-9X";
    const MARKET: &str = "https://x.ai/bot/marketplace/bots/alfred";
    const SHARE_HTML: &str = include_str!("../fixtures/templates/alfred-template.html");
    const MARKET_HTML: &str = include_str!("../fixtures/templates/alfred-marketplace.html");

    #[test]
    fn recorded_share_contains_identity_without_invented_skills() {
        let preview = parse(SHARE, SHARE_HTML).unwrap();
        assert_eq!(preview.name, "Alfred");
        assert!(preview.description.starts_with("Designs, audits"));
        assert!(preview.skills.is_empty());
        assert!(preview.memories.is_empty());
        assert!(preview.components.is_empty());
    }

    #[test]
    fn recorded_marketplace_contains_memories_and_unrendered_tabs() {
        let preview = parse(MARKET, MARKET_HTML).unwrap();
        assert_eq!(preview.name, "Alfred");
        assert!(preview.memories[0].starts_with("OPERATING INSTRUCTIONS\n\nYou are Alfred"));
        assert!(preview.memories[0].contains("user's Grok Bot"));
        assert_eq!(preview.components, ["skills", "routines"]);
        assert_eq!(preview.not_imported, ["skills", "routines"]);
    }

    #[test]
    fn missing_tabs_or_identity_fail_closed() {
        assert!(
            parse(MARKET, &MARKET_HTML.replace("aria-current=\"true\"", ""))
                .unwrap_err()
                .to_string()
                .contains(CHANGED)
        );
        assert!(parse(SHARE, "<h1>Alfred</h1>").is_err());
        assert!(parse(MARKET, &MARKET_HTML.replace("</ul>", "")).is_err());
    }

    #[test]
    fn an_empty_public_memories_panel_is_valid() {
        let start = MARKET_HTML
            .find("<li><p class=\"text-primary text-sm leading-6 whitespace-pre-wrap\">")
            .unwrap();
        let end = MARKET_HTML[start..].find("</ul>").unwrap() + start;
        let mut html = MARKET_HTML.to_owned();
        html.replace_range(start..end, "");
        let preview = parse(MARKET, &html).unwrap();
        assert!(preview.memories.is_empty());
        assert_eq!(preview.not_imported, ["skills", "routines"]);
    }

    #[test]
    fn legacy_bot_config_keeps_provenance_optional() {
        let bot: home::Bot = toml::from_str("name = \"Legacy\"\n").unwrap();
        assert!(bot.template_url.is_none());
        assert!(bot.template_components.is_empty());
        assert!(!toml::to_string(&bot).unwrap().contains("template_"));
    }

    #[test]
    fn allowlist_rejects_other_hosts_and_path_tricks() {
        for url in [
            "http://x.ai/bot/id",
            "https://x.ai.evil/bot/id",
            "https://x.ai@evil/bot/id",
            "https://x.ai:443/bot/id",
            "https://x.ai/bot/id?x=y",
            "https://x.ai/bot/id#x",
            "https://x.ai/bot/../id",
            "https://x.ai/bot/id/extra",
            "https://x.ai/bot/%69d",
            "https://x.ai/bot/marketplace",
            "https://x.ai/bot/",
        ] {
            assert!(marketplace(url).is_err(), "{url}");
        }
        assert!(!marketplace(SHARE).unwrap());
        assert!(marketplace(MARKET).unwrap());
    }

    #[test]
    fn curl_is_bounded_cookie_free_and_never_follows_redirects() {
        let args = curl_args(SHARE);
        assert_eq!(args[0], "--disable");
        for pair in [
            ["--proto", "=https"],
            ["--max-redirs", "0"],
            ["--max-time", "20"],
            ["--max-filesize", "5242880"],
        ] {
            assert!(args.windows(2).any(|window| window == pair));
        }
        assert!(args.contains(&"-f".into()));
        assert!(!args.iter().any(|arg| {
            ["-L", "--location", "-b", "--cookie", "--cookie-jar"].contains(&arg.as_str())
        }));
        assert_eq!(args.last().unwrap(), SHARE);
    }

    #[test]
    fn pasted_routines_keep_prose_and_only_explicit_crons_create_disabled_schedules() {
        crate::with_temp_home(|| {
            let mut preview = parse(MARKET, MARKET_HTML).unwrap();
            paste(&mut preview, "## Routine: Morning\nEvery morning, review the queue.\n## Routine: Weekly\nCron: 0 9 * * 1\nReview the weekly queue.\n## Skill: Review\nDescription: Review a queue.\nRead the queue and summarize.\n## Memories\nThe owner is Sam.").unwrap();
            let (slug, dir) = write(preview, Some("Alfred copy".into()), None, None).unwrap();
            let bot = home::load_bot(&slug).unwrap();
            assert_eq!(bot.source, "local");
            assert_eq!(bot.template_url.as_deref(), Some(MARKET));
            assert_eq!(bot.template_source.as_deref(), Some("grok"));
            assert!(bot.grok_id.is_none());
            assert!(!bot.memory);
            assert_eq!(bot.schedules.len(), 1);
            assert!(!bot.schedules[0].enabled);
            let text = fs::read_to_string(dir.join("BOT.md")).unwrap();
            assert!(text.starts_with("Designs, audits"));
            assert!(text.contains("## Routines\n"));
            assert!(text.contains("Skills: skills/review.md"));
            assert!(dir.join("skills/review.md").is_file());
            let notes = fs::read_dir(dir.join("memory"))
                .unwrap()
                .collect::<std::io::Result<Vec<_>>>()
                .unwrap();
            assert_eq!(notes.len(), 1);
            assert!(
                fs::read_to_string(notes[0].path())
                    .unwrap()
                    .contains("The owner is Sam.")
            );
            assert!(!home::root().join(".state/memory").exists());
            assert!(
                write(
                    parse(SHARE, SHARE_HTML).unwrap(),
                    Some("Alfred copy".into()),
                    None,
                    None
                )
                .is_err()
            );
        });
    }

    #[test]
    fn existing_non_bot_content_is_preserved_and_failed_creation_can_retry() {
        crate::with_temp_home(|| {
            let existing = home::bot_dir("existing");
            fs::create_dir_all(existing.join("skills")).unwrap();
            fs::write(
                existing.join("skills/review.md"),
                "Keep my original content.",
            )
            .unwrap();
            let mut preview = parse(SHARE, SHARE_HTML).unwrap();
            paste(&mut preview, "## Skill: Review\nReplacement content.").unwrap();
            let error = write(preview, Some("Existing".into()), None, None).unwrap_err();
            assert!(error.to_string().contains("--name"));
            assert_eq!(
                fs::read_to_string(existing.join("skills/review.md")).unwrap(),
                "Keep my original content."
            );
            assert!(!existing.join("bot.toml").exists());
            let missing = home::root()
                .join("missing-directory")
                .to_string_lossy()
                .into_owned();
            assert!(
                write(
                    parse(SHARE, SHARE_HTML).unwrap(),
                    Some("Retry".into()),
                    Some(missing),
                    None
                )
                .is_err()
            );
            assert!(!home::bot_dir("retry").exists());
            write(
                parse(SHARE, SHARE_HTML).unwrap(),
                Some("Retry".into()),
                None,
                None,
            )
            .unwrap();
            assert!(home::bot_dir("retry").join("bot.toml").is_file());
        });
    }

    #[test]
    fn paste_rejects_ambiguous_or_executable_input_and_slug_collisions() {
        for input in [
            "A skill without a heading",
            "## Scripts\nrun this",
            "## Skill: A\n```sh\nhello\n```",
            "## Routine: Invalid\nCron: tomorrow\nReview",
            "## Skill: Review!\nFirst\n## Skill: Review\nSecond",
        ] {
            assert!(
                paste(&mut parse(SHARE, SHARE_HTML).unwrap(), input).is_err(),
                "{input}"
            );
        }
    }
}
