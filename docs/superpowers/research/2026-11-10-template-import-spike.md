# Bot template import spike (usability item U2b)

Run date: 2026-11-10
Scope: the four Unknowns in [U2](../../usability-notes.md#u2-create-bots-and-rooms-from-the-chat-window--button-and-import-from-a-template-url) — fetchability, payload format and contents, other providers' equivalents, and a recommended import design for U2(c).
Method: plain `curl` from this shell, no cookies, no login, no account, no private API. Pages were fetched with the default `curl/8.x` User-Agent. Nothing was installed and no Grok Bot, Cursor, Claude or ChatGPT session was opened.

Tags: **[verified]** means I fetched or grepped it in this run and the command and result are quoted below. **[unverified]** means it is inference from what is quoted and has not been exercised.

## Answers

| Unknown | Answer |
|---|---|
| Is a Grok Bot share/template link fetchable without a login? | **Yes, as HTML.** `https://x.ai/bot/<id>` returns 200 with a small HTML preview, and marketplace bot pages return 200 with more. No cookie, token or session was needed. **[verified]** |
| What format is it? | Server-rendered HTML (Next.js). No public JSON: no documented API, no `application/json` payload, and the only structured data is schema.org `SoftwareApplication` metadata. Installing is an app-only deep link, `grokbot://app/v1/bot-template?id=<id>`. **[verified]** |
| Are skills, memories, plugins and schedules in the payload? | **Partly, and not from the web page.** Memories text is rendered in the marketplace page's HTML. Skills, routines/schedules and integrations appear only as tab labels, in the page and in the RSC payload. Official docs say the *link* exposes identity, description, skills and routines; that full payload reaches the app through the deep link, so it is app-side and not fetchable. **[verified for the HTML and deep link, unverified for the in-app payload]** |
| Do other providers offer a fetchable equivalent? | **Yes, but as files and JSON, not as a bot template link.** Claude Code publishes `marketplace.json` plugin catalogues over HTTPS (fetched: 200, 6,829 bytes). Cursor distributes plugins as git repositories with `plugin.json` manifests and keeps rules in the repo. ChatGPT GPTs share by link but the store is login-walled and GPTs are being retired in favour of plugins; plugin sharing is workspace-permission based. **[verified]** |
| Recommendation for U2(c) | Fetch the URL for identity, description and memories, show a preview, and let the user paste what the page withholds. Write the result as a **normal `source = "local"` bot** with provenance in `bot.toml`, skills as one file each in `skills/`, memories as notes in `memory/`, routines as **disabled** `[[schedules]]` only when the user supplies a cron. Do not scrape the marketplace, do not call a private API, do not require a login. See [the design](#recommended-design-for-u2c). |

Two findings change what U2's "what is known" section assumed:

1. The preview page does **not** carry skills or memories, so a URL import cannot be the route to skills that `import-grok` misses. Only the marketplace page's memories tab is public; skills and routines stay behind the app handoff. **[verified]**
2. Grok Bot's own documentation is now hosted under `cursor.com/docs/grok-bot`, and the app download on the template page points at `api2.cursor.sh`, so the canonical source for template behaviour is the Cursor docs, not the third-party posts in U2's note. **[verified: URLs and redirect targets; unverified: any statement about the companies]**

## 1. Fetchability

| URL | Result |
|---|---|
| `https://x.ai/bot/guides/templates-for-grok-bot` | 200, `text/html`, 144,804-byte body, `x-nextjs-prerender: 1`, `cache-control: s-maxage=31536000` |
| `https://x.ai/bot/marketplace` | 200, 370,436-byte body, listing rendered server-side |
| `https://x.ai/bot/marketplace/bots/alfred` | 200, 106,210-byte body |
| `https://x.ai/bot/marketplace/bots/tinkabot` | 200, 93,609-byte body |
| `https://x.ai/bot/p7Gh6HIrfv4AGzIow6-9X` (the template link for `alfred`) | 200, 69,726-byte body |
| `https://x.ai/bot` (product page) | 403 to `curl` — the only 403 in the set |
| `https://x.ai/bot/guides`, `https://grok.com/bot/marketplace` | 200 |
| `https://x.ai/bot/marketplace/` (trailing slash) | 308 to `/bot/marketplace` |

No request carried a cookie jar, an `Authorization` header or a token; each `curl` was a fresh process. The pages are prerendered and served from cache, so this is not an accident of a warm session. **[verified]**

The marketplace listing is complete enough to enumerate bots without JavaScript: `/bot/marketplace` contains 35 distinct `href="/bot/marketplace/bots/<slug>"` links plus nine category links. `robots.txt` allows everything except `/tools/` and declares `Content-Signal: ai-train=yes, search=yes, ai-input=yes`. **[verified]**

The template page is deliberately unindexed — `<meta name="robots" content="noindex, nofollow">` — even though it is publicly fetchable. Reading one URL a user pastes is not the same act as crawling the catalogue, and the design below keeps that distinction. **[verified for the meta tag; the last sentence is a judgement]**

I did not invent or guess any share link: the one id used above is the `Import Bot` target that the public marketplace page itself links to. **[verified]**

## 2. Format and payload contents

### The template page is an HTML preview plus a deep link

`https://x.ai/bot/p7Gh6HIrfv4AGzIow6-9X` renders, in full, this much:

- the bot's name (`Alfred`) and its description;
- a third-party disclaimer that ends "By clicking 'Add to Grok Bot', you accept the terms";
- one anchor, `href="grokbot://app/v1/bot-template?id=p7Gh6HIrfv4AGzIow6-9X"`, labelled `Add to Grok Bot`;
- a "Don't have Grok Bot?" download link.

That is the whole payload: no instructions, no skills, no memories, no JSON. The custom scheme is the handoff, so the actual template is fetched by the desktop app after the user installs it. **[verified]**

### The marketplace page is richer, but only for memories

`/bot/marketplace/bots/alfred` adds a details panel with three tabs — `memories · skills · routines` — and renders the memories tab server-side as a `<ul>`; the `alfred` page has one `<li>` whose text begins `OPERATING INSTRUCTIONS` and then carries the bot's whole prompt. `tinkabot` renders the same shape with six memory/fact paragraphs and a different tab set: `memories · skills · integrations`. So the page varies its tabs with the components the template actually contains, which tells the importer *what kinds* of component exist without exposing most of them. **[verified]**

What is not there:

- The `skills`, `routines` and `integrations` tabs are `<button>`s with no server-rendered panel. `skills` appears once on each page, as the label plus the subtitle `Playbooks it can run`; no skill body, name list or count is in the HTML.
- The React Server Components payload (`self.__next_f.push`) contains no `"skills"`, `"routines"`, `"memories"`, `"instructions"`, `"plugins"` or `"slug"` keys — 0 hits each. It carries the same rendered text and a chunk manifest.
- The only `application/ld+json` block is schema.org metadata: `SoftwareApplication` with `name`, `description`, `url`, `author`, `offers.price = 0`. Useful for identity, useless for content.
- There is no documented public endpoint. `docs.x.ai` is linked from the page footer and the marketplace has no export or download control; the 10 `export` matches in the page are unrelated marketing copy. **[verified within the fetched HTML; absence of a documented API is bounded by what I searched]**

### Official documentation on what a template carries

`https://cursor.com/docs/grok-bot/work.md`, section "Share a Bot" **[verified, quoted]**:

> Share a public link when someone else should start from the same Bot. The link exposes the shared configuration (identity, description, skills, and routines), so remove API keys, internal URLs, and customer data first. The recipient adds a copy to their own account; they never get your computer, logins, or conversation history […] On the Teams plan and the Enterprise plan, admins control whether members can publish Bot templates publicly.

`https://x.ai/bot/guides/templates-for-grok-bot` **[verified, quoted]**:

> When you create a template, you create a copy of the bot's instructions, relevant memories and skills, and specific plugins. […] The bot kept memories and plugins, but left out personal or internal memories. […] If there are plugins that were included in the original bot, your new bot will have to reinstall those plugins, but it comes with them. […] If there are non-standard plugins like MCP servers or other scripts or code, those also aren't included in the template bot. […] Routines are just the triggers that you might have for a bot, like how often it runs on its own. […] First-party plugins and skills will be included in your template. […] Templates will not share any personal or private skills, or private information.

Put together **[verified against those two pages]**: the link's payload is identity, description, skills, routines and first-party plugin/skill references; it excludes secrets, custom code, private or personal memories and private skills. Combined with the HTML evidence, the split is:

| Component | Public HTML | After the `grokbot://` handoff |
|---|---|---|
| Name, description, creator | yes (template page and marketplace page) | yes |
| Instructions / memories | yes on the **marketplace** page, no on the template page | yes |
| Skills | label only | yes |
| Routines / schedules | label only | yes |
| Plugins / integrations | label only (where the tab appears) | references only; the recipient reinstalls them |
| Secrets, custom code, scripts, MCP servers | never | never |

Two smaller official facts matter for the design: routines "are just the triggers" (a schedule plus an instruction in prose, not cron), and the Teams/Enterprise plan lets an admin forbid public template publishing, so a URL that works today can legitimately stop working. **[verified from the quoted docs]**

## 3. Other providers

| Provider | Shareable agent template? | Fetchable format | Official source |
|---|---|---|---|
| Claude Code / Claude | Yes. A marketplace is a directory or git repo with `.claude-plugin/marketplace.json`; users register it and install plugins from it. Plugins carry `skills/`, `commands/`, `agents/`, `hooks/` and `.mcp.json`. | **Yes, JSON over HTTPS.** `https://raw.githubusercontent.com/anthropics/claude-code/main/.claude-plugin/marketplace.json` returned 200 in 6,829 bytes and starts `{"$schema": "https://json.schemastore.org/claude-code-marketplace.json", "name": "claude-code-plugins", … "plugins": [{"name": "agent-sdk-dev", "source": {"source": "git-subdir", "url": "…", "path": "plugins/agent-sdk-dev"}}]}`. `anthropics/skills` serves the same shape in 2,213 bytes. | [plugin marketplaces](https://code.claude.com/docs/en/plugin-marketplaces), [plugin manifest reference](https://code.claude.com/docs/en/plugins-reference), [Agent Skills](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview) |
| Claude skills | Yes. A skill is a directory with `SKILL.md` (YAML `name`/`description`) plus optional scripts; in Claude Code it lives in `~/.claude/skills` or `.claude/skills`, and custom skills are shared through plugins; in claude.ai they are uploaded as zips. | Files and zips, not a template URL. | same Agent Skills page |
| ChatGPT / GPTs | Sharing exists (specific people, groups, the whole workspace) but GPTs are being retired in favour of plugins. | **No public template URL or JSON.** `https://chatgpt.com/gpts` returns 200 with a 700 KB app shell whose only auth text is `Sign in` — the store needs a session. Plugin sharing is workspace permissions plus the workspace directory. | [GPTs and sharing](https://learn.chatgpt.com/docs/enterprise/gpts-and-sharing), [Build plugins](https://learn.chatgpt.com/docs/build-plugins), [Migrating custom GPTs](https://learn.chatgpt.com/docs/migrate-custom-gpts) |
| Codex | Skills and `AGENTS.md` are files in the repository or in `$CWD/.agents/skills`, `~/.codex/skills`, `/etc/codex/skills`. | Files, fetchable only where the repo or file is public. | [Build skills](https://learn.chatgpt.com/docs/build-skills), [AGENTS.md](https://learn.chatgpt.com/docs/agent-configuration/agents-md) |
| Cursor | Plugins are distributed as git repositories with a `plugin.json` manifest — `.cursor-plugin/plugin.json` for Cursor Plugins, a root `plugin.json` for the Agent Plugins standard — and listed in the Cursor Marketplace. Rules are repository files (`.cursor/rules`, `AGENTS.md`). | **Yes, via git**: clone the repo and read the manifest and files. No template link. | [Plugins](https://cursor.com/docs/plugins), [Rules](https://cursor.com/docs/rules) |
| Grok Bot | Yes: one public link per template. | HTML preview plus an app-only deep link (this spike). | [Share a Bot](https://cursor.com/docs/grok-bot/work.md#share-a-bot), [Templates guide](https://x.ai/bot/guides/templates-for-grok-bot) |

The useful conclusion is that the providers that publish an actual *format* publish **files and JSON, in a git repo or over HTTPS**, and the provider that publishes a *link* publishes an app handoff. Grok Bot is the only one in this set whose template can be previewed in a browser; everybody else's fetchable artefacts are things herdbot can already read from disk. **[verified against the table's sources; the conclusion is inference]**

## Recommended design for U2(c)

### Source of input: URL for the preview, paste for the rest, files where they exist

Three inputs, in priority order. **(1)** A Grok Bot URL (`https://x.ai/bot/<id>` or `https://x.ai/bot/marketplace/bots/<slug>`), fetched once, server-side allowlisted to those two path shapes on `x.ai`, with `herdbot/<version>` as the User-Agent, no redirects off-host, and no caching. **(2)** A paste field in the same sheet for skills, memories or routines the user copies out of the Grok Bot app — because the page does not expose them, and because the only supported way to get them is the app the user is already signed in to. **(3)** A folder or file drop for formats that genuinely are fetchable and open: a Claude Code `marketplace.json` or plugin directory, a `SKILL.md` directory, an `AGENTS.md`.

Rejected, with reasons: scraping `/bot/marketplace` to build a browsable catalogue (the pages are `noindex`, one admin toggle removes public publishing, and the catalogue is large and fast-moving); calling any private Grok Bot API (the README already says herdbot does not, and it would need credentials); requiring a login in herdbot; and inventing an "export file" format, because no official export exists in anything I read. **[the rejection of an export file is verified to the extent that no export control or doc appeared in the fetched pages]**

### Preview before write

The sheet should show exactly what was fetched and what was not, then reuse the U2(a) bot form fields (name, working directory, task class, instructions). The "not imported" list is not a guess: it is the set of tab labels on the page minus the tab that was rendered. For `alfred` that reads "skills, routines — open the template in Grok Bot and paste them here". A user who pastes nothing still gets a usable bot built from the description and memories, and knows why it is thin.

### Mapping onto the herdbot bot layout

| Template part | herdbot target | Notes |
|---|---|---|
| Name, description | `bot.toml` `name`/`title`; slug via the existing `home::slug` | Same rules as `herdbot new`: lowercase, ≤20 chars |
| Instructions (template page: absent; marketplace memories tab) | `BOT.md` | Verbatim first, then a short herdbot provenance header. `[paths]` rewrites still apply at launch, so `/workspace/...` strings stay as written |
| Memories (facts) | `memory/imported-<YYYY-MM-DD>.md`, one note per fact | Plain notes the agent reads with its file tools. Do **not** write `.state/memory/<slug>.json`: that is OptChat's cache, and `memory = true` stays off unless the user asks |
| Skills | `skills/<skill-slug>.md`, one file per skill, YAML frontmatter `name`/`description` | Uses the same shape as Anthropic's `SKILL.md` so a skill can move either way. herdbot has **no skills loader** — `home.rs` creates the directory and the preamble tells the agent to read `{dir}/skills` — so `BOT.md` must gain a one-line index (`Skills: skills/x.md, skills/y.md`) or the agent may never look |
| Routines / schedules | `[[schedules]]` with `enabled = false`, plus the prose instruction in `BOT.md` | Only when the user supplies a cron. Grok routines are prose ("Every weekday at 8:00 AM") and `Schedule.cron` is a real cron expression, so converting automatically would be guesswork. If no cron is given, leave a `## Routines` section in `BOT.md` and no schedule entry |
| Plugins / integrations | `bot.toml` note and a `BOT.md` line naming them | Nothing to install: the template never carries the plugin, the recipient reinstalls it, and credentials are excluded by design |
| Secrets, code, scripts | nothing, ever | Excluded by the format itself |

`bot.toml` should record provenance rather than pretend the bot was hand-made: `template_url`, `template_source = "grok"`, `template_fetched_at`, and a `template_components` list (what the page said it had). Those are new optional, defaulted fields, so old files keep parsing. **[verified that `Bot` uses `#[serde(default)]` throughout, so added fields are backward compatible]**

### The open question: `source = "local"` or `source = "grok"`?

**Recommendation: `source = "local"`, with `template_source = "grok"` as provenance.**

The code decides this rather than taste. `grok::import()` finds an existing bot by `grok_id`, creates a bot with `source = "grok"` only when no local bot of that slug exists, and overwrites `BOT.md` only when `source == "grok"`. A template copy has no roster row and therefore no `grok_id`, so:

- `source = "grok"` would mean a bot the sync can never match, refresh or own — a permanent lie in `bot.toml`;
- worse, that bot's slug now occupies the name, and a later roster row with the same name hits the `skipped {name:?}: local bot {slug} already exists` branch, so a real Grok bot would silently not import;
- `source = "local"` means herdbot owns `BOT.md`, the daemon's re-import leaves it alone, and the user's edits (and `BOT.local.md`) are never at risk. **[verified in `crates/herdbot/src/grok.rs` and `daemon.rs`]**

The cost is that an imported template cannot be refreshed from its URL. That is the right default: Grok Bot's own docs say the recipient gets "a copy" and that "adding a shared Bot accepts the third-party bot terms", and there is no version/etag in the page to refresh against. A `herdbot import-template --refresh` that re-fetched and diffed against `BOT.local.md` is a possible follow-up, not part of (c). If Shane wants refreshability, it should be a separate decision because it needs a merge policy.

The `template_url` field keeps the cheap part of refresh: the UI can show where the bot came from, and re-paste is a manual action.

### Seam and sizing

Keep the CLI as the only writer, as U2(a) already does. One new subcommand is enough:

```sh
herdbot import-template <url> [--name NAME] [--cwd DIR] [--class CLASS] [--paste FILE] [--dry-run] [--json]
```

- `--dry-run` returns the parsed preview (`{"name":…,"description":…,"memories":[…],"components":["skills","routines"]}`) and writes nothing, so the sheet can show it before Create.
- `--json` on success returns the same `{"slug","path"}` shape as `herdbot new`, which the app already decodes.
- `--paste FILE` reads the user's pasted skills/routines/memories as text and splits it on explicit `## ` headings; without it, the instruction is description plus memories only.

Network access belongs in the CLI, not the app, so the app keeps no URL-fetch code and the allowlist lives in one place. Parsing the pages is regular HTML text extraction, not a headless browser: the data is prerendered, and a regex over `<li>` and the tab labels is enough — but it is also fragile by nature, so it must fail loudly (`exit 1` with "the template page has changed; paste the template instead") rather than half-import.

Estimated size: medium. One module (fetch, parse, map), one CLI arm, one preview sheet, tests from recorded fixtures. No new dependency if the HTML scan is hand-written; a small HTML parser crate is the alternative and needs a dependency decision.

### Tests that would have caught the mistakes in this spike

Fixture-based, no network: (1) the `alfred` template page yields name and description and *no* skills; (2) the `alfred` marketplace page yields the memories text and component labels `["skills","routines"]`; (3) a page with the tabs removed exits 1 instead of importing an empty bot; (4) an imported bot is `source = "local"`, has `template_url` set, and a subsequent `grok::import()` neither overwrites `BOT.md` nor skips a real roster row because of a slug clash; (5) a pasted routine without a cron produces no `[[schedules]]`; with a cron produces one with `enabled = false`.

## Left unverified

- **The in-app payload.** I did not install Grok Bot, follow the `grokbot://` link or sign in, so "skills and routines arrive in the app" rests on the two official docs, not on observation. If Shane has a template he can add, the app's "View Details" dialog is the place to confirm the skill and routine shape before (c) fixes the paste format.
- **Whether a routine can be exported as a cron from the app.** Nothing in the docs or pages shows one; the docs describe routines in prose only.
- **Whether every marketplace bot's memories are complete.** I read two pages. A bot whose memories are all private may render an empty tab, which the parser must treat as "nothing public", not as a failure.
- **Non-`x.ai` template hosts.** Some share links may be mailed or mirrored elsewhere; only `x.ai` paths were tested.
- **Rate limits, terms of use for automated fetching.** `robots.txt` allows the paths, but the bot-sharing terms page (`/legal/bot-sharing-terms`, linked from the template page) was not read. One fetch per user action is the safe reading; a catalogue crawl is not.
- **Cursor's marketplace JSON.** I confirmed plugins are git-hosted with `plugin.json` manifests and that a publish flow exists, but did not fetch a listing endpoint, because Cursor documents git distribution rather than a catalogue API.

## Sources

Fetched in this run (all with plain `curl`, no session):

- <https://x.ai/bot/guides/templates-for-grok-bot> — template guide: what is packaged, install flow, exclusions.
- <https://x.ai/bot/marketplace> — public listing, 35 bot links and nine categories.
- <https://x.ai/bot/marketplace/bots/alfred> — marketplace detail page; memories tab rendered, `skills`/`routines` label only.
- <https://x.ai/bot/marketplace/bots/tinkabot> — second detail page; memories rendered, `skills`/`integrations` label only.
- <https://x.ai/bot/p7Gh6HIrfv4AGzIow6-9X> — the `alfred` template link itself: preview, `noindex`, `grokbot://app/v1/bot-template?id=…`.
- <https://x.ai/robots.txt> — `Allow: /`, `Disallow: /tools/`, Content-Signal line.

Official documentation:

- <https://cursor.com/docs/grok-bot/work.md#share-a-bot> — "Share a Bot": the link exposes identity, description, skills and routines; the recipient gets a copy; admins can restrict public publishing.
- <https://cursor.com/docs/grok-bot> and <https://cursor.com/docs/llms.txt> — Grok Bot's current canonical docs location.
- <https://code.claude.com/docs/en/plugin-marketplaces> — `.claude-plugin/marketplace.json`, register and install.
- <https://code.claude.com/docs/en/plugins-reference> — `plugin.json`, `skills/`, `commands/`, `agents/`, `hooks/`, `.mcp.json`.
- <https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview> — `SKILL.md`, zip upload, sharing through plugins.
- <https://learn.chatgpt.com/docs/enterprise/gpts-and-sharing> — GPT sharing scope.
- <https://learn.chatgpt.com/docs/build-plugins> — plugin sharing permissions; custom GPTs retiring.
- <https://learn.chatgpt.com/docs/migrate-custom-gpts> — migration guidance.
- <https://learn.chatgpt.com/docs/build-skills> — `SKILL.md` locations in Codex.
- <https://learn.chatgpt.com/docs/agent-configuration/agents-md> — `AGENTS.md`.
- <https://cursor.com/docs/plugins> — plugins as git repos, `plugin.json`, Agent Plugins standard, marketplace and publish flow.
- <https://cursor.com/docs/rules> — rules as repository and dashboard files.

Fetched artefacts (evidence):

- `https://raw.githubusercontent.com/anthropics/claude-code/main/.claude-plugin/marketplace.json` — 200, 6,829 bytes, `$schema`, `name`, `owner`, `plugins[]` with `git-subdir` sources.
- `https://raw.githubusercontent.com/anthropics/skills/main/.claude-plugin/marketplace.json` — 200, 2,213 bytes.
- `https://chatgpt.com/gpts` — 200, 700,755-byte app shell, `Sign in` only, no public catalogue content.

Repository code read for the mapping and the `source` decision:

- `crates/herdbot/src/grok.rs` — roster cache path, `grok_id` matching, `BOT.md` overwrite only when `source == "grok"`, skip when a local bot of the slug exists.
- `crates/herdbot/src/daemon.rs` — `grok::roster_mtime()` / `grok::import()` on roster change; `fire_schedules` only fires `enabled` schedules.
- `crates/herdbot-core/src/home.rs` — `Bot` fields and `#[serde(default)]`, `Schedule { name, cron, prompt, enabled }` with "imported schedules start disabled", `memory`/`skills` directories created per bot.
- `crates/herdbot/src/tools.rs` — the preamble line telling each agent that `memory/` and `skills/` are its own files.
- `crates/herdbot/src/main.rs` — `herdbot new … --instructions TEXT [--json]`, the seam an importer should reuse.
- `README.md` — "Schedules, skills and memory aren't on disk … they'll have to come from an exported bot template", and the roadmap item "Schedule and skill import from Grok Bot templates".
