# Herdbot — microsite

Static microsite for [Herdbot](https://github.com/slogsdon/herdbot), served at
`https://projects.logsdon.io/herdbot/`.

Source revision: `bba0563ae3ac6d891b9bb26984f4c74444f3d005` (branch `feature/website`
of `slogsdon/herdbot`, HEAD at the time of writing; commit date 2026-10-22, which is
the `lastmod` in `site.json`). All product content on the site is derived from that
revision: `README.md`, `routing.toml`, `packaging/herdbot.rb.tmpl`,
`crates/herdbot/src/main.rs` (the CLI usage text), and
`crates/herdbot/src/{doctor,setup}.rs`.

## Build

Node 26.12.0, PHP 8.3+, and Composer are required.

```sh
cd website
composer install --no-interaction --prefer-dist
npm install                          # only needed for the packaging checks below
node scripts/build-site.ts           # SITE_NOINDEX=1 by default: preview, noindex
SITE_NOINDEX=0 node scripts/build-site.ts   # indexable production artifact
```

Output is a physical subtree at `website/dist/herdbot/`, plus `dist/_headers` in
preview builds. Nothing else is written at the `dist/` root: the hub owns
`/robots.txt`, `/sitemap.xml`, and the slashless `/herdbot` redirect.

Packaging (local only; neither command deploys):

```sh
npm ci
npx --no-install cf build --mode microsite
npx --no-install cf build --mode microsite-production
```

## One file drives the site

`site.json` holds the four values the build and the pages need — `slug`, `name`,
`origin`, `lastmod` — plus the `source` URL. `scripts/build-site.ts` validates
them, exports them to the PHP layout as `SITE_SLUG`, `SITE_NAME`, `SITE_ORIGIN`
and `SITE_NOINDEX`, and mounts every emitted URL under `/<slug>/`. The layout
throws if they are missing, so nothing falls back to a hard-coded project name.

`cloudflare.config.ts` keeps the Worker name (`herdbot-microsite`, and
`herdbot-microsite-preview` off production) and the route pattern
(`projects.logsdon.io/herdbot/*`) as literals on purpose: they are the deployment
identity, not site content, and the file is loaded by the Cloudflare CLI outside
the build script.

## What the build asserts

`scripts/build-site.ts` fails the build unless all of the following hold:

- `index.html`, `404.html`, `sitemap.xml`, `assets/site.css`, `assets/tokens.css`,
  `assets/fonts/*` exist and are non-empty.
- No `_redirects`, `robots.txt`, `sitemap.xml`, `index.html`, or `404.html` at the
  `dist/` root; the root holds only `<slug>/` (and `_headers` when `SITE_NOINDEX=1`).
- Every HTML page has exactly one `<h1>`, a unique `<title>` containing the project
  name, a meta description, the skip link, the `main` landmark, and the primary
  `nav`.
- Canonical and `og:url` equal `https://projects.logsdon.io/herdbot/<path>`; the 404
  carries neither and is always noindex.
- The `noindex` meta matches `SITE_NOINDEX`, and the footer links both
  `https://projects.logsdon.io/` and `https://shane.logsdon.io/`.
- No `href`/`src` is root-relative outside `/<slug>/`, and every local link and
  asset target exists in the artifact.
- The sitemap lists exactly the canonical pages, with `lastmod` set to
  `site.json`'s `lastmod`.

The Google Fonts `@import` is stripped from the copied `tokens.css` (fonts are
served locally) and `/assets/` inside the copied CSS is rewritten to
`/herdbot/assets/`.

## Seeded skeleton

`website/` is the verified pilot skeleton from
`deferred-skill-catalog-website/website` (`scripts/build-site.ts`, the layout, the
CSS, tokens, fonts, `composer.*`, `package.json`, `vite.config.ts`), unmodified
except for this project's content and identity. One inherited string was left
alone because the skeleton files were out of scope for this change:
`composer.json`'s `description` still reads "Microsite for Deferred Skill
Catalog". It is package metadata, not site output; the lock is otherwise the
hub's `slogsdon/flat-file` pin.

## Content and design provenance

- Pages: `pages/index.php` is F · Frontispiece (what herdbot is, the
  bot/subagent/route/failover vocabulary, status, where to go; no install guide).
  `pages/getting-started.php` is D · Assembly Order (seven numbered steps:
  requirements → install → doctor/setup → new + up → routing → first task → known
  edges). `pages/404.html.php` is G · Recovery.
- The install route is the one the source README documents: the Homebrew cask
  `slogsdon/tap/herdbot`, with the Releases zip as the alternative. The
  `slogsdon/homebrew-tap` repository is linked as distribution infrastructure, not
  as a second site.
- Design is the collection's: `public/assets/tokens.css`, the fonts,
  `assets/fonts/LICENSES.txt`, and the hub's `site.css` are copied from the
  collection hub (`projects.logsdon.io`) via the pilot, along with
  `design/provenance.json`. No CSS was appended for this site; every class used on
  these pages already exists in the copied file.
- Every status qualifier the source states is kept on the page: early
  development, macOS only (launchd), `cursor-agent` not seen working end to end,
  claude's quota detection unverified, the untested resend path, the unverified
  `ask` approval flow and remote hosts, and the modal that can swallow a queued
  message. No release date, version, measurement or capability was invented.

## Verified here

- `composer install`, `node scripts/build-site.ts` (preview and
  `SITE_NOINDEX=0`), and the artifact assertions above.
- `npm install`/`npm ci` and both `npx --no-install cf build` modes (packaging
  only).
- The absence of out-of-namespace root-relative URLs, and the preview/production
  indexing difference.
- That `site.json`'s `lastmod` (2026-10-22) is the content revision's commit date.

## Not verified here (Unverified)

No product command on the site was executed while building it — no `brew`, no
`herdbot`, no `cargo`, no harness CLI. The site quotes them from the source
revision, and the getting-started page says so.

- `brew install --cask slogsdon/tap/herdbot` — from `README.md` (Install) and
  `packaging/herdbot.rb.tmpl`. Not run; installing the product was out of scope
  for this change.
- The Releases zip path ("drag `Herdbot.app` to Applications, open it, and press
  **Set up everything**") — from `README.md` (Install). Not run.
- `herdbot doctor`, `herdbot doctor --json`, `herdbot setup`,
  `herdbot setup --only restart-daemon`, `herdbot service status|install|uninstall`,
  and `HERDBOT_HOME` — from `README.md` (Install, "The service", "Other commands"),
  the CLI usage in `crates/herdbot/src/main.rs`, and the fix order in
  `crates/herdbot/src/setup.rs`. Not run. The list of checks doctor performs is read
  from `crates/herdbot/src/doctor.rs` and not exercised against a live install.
- `herdbot new <name> [--cwd DIR] [--class CLASS]`, the `created … (edit BOT.md for
  its instructions)` output, `herdbot up <bot>` / `down <bot>`, and `herdbot list`
  — from `README.md` (Bot layout, Other commands) and `main.rs`. Not run.
- `herdbot send <bot> "…"`, `herdbot task <bot> <class> "…"`, `herdbot runs`,
  `herdbot status --json`, `herdbot history …`, `herdbot focus <agent>`, and the
  `card list` / `card answer` pair — from `README.md` (Quick start, Other commands,
  Harness launch) and `main.rs`. Not run; the prompts on the page are examples, not
  transcripts.
- `herdbot route <class>` / `herdbot route <class> --bot <bot>` and the routing
  order, class list, effort fallback, cooldown and failover behaviour — from
  `README.md` (Routing) and `routing.toml`. Read, not exercised; the model IDs in
  the table were listed by the CLIs on 2026-10-19, which this build did not repeat.
- herdr "tested with 0.9.1, socket protocol 22", the harness version table
  (claude 2.1.304, codex 0.163.0, cursor-agent 2026.10.16), every "verified live on
  2026-10-20" claim, and the current limits and roadmap items — from `README.md`.
  Reproduced from the document, not re-measured here.
- `cargo install --path crates/herdbot` and the source quick start — from
  `README.md` (Quick start → From source). Not run.
- The cask's `depends_on macos: :sequoia` and its uninstall/zap stanzas — from
  `packaging/herdbot.rb.tmpl`. Read only; no cask was installed or removed.

## Deployment

Not performed from this repository: no `cf deploy`, no `cf previews deploy`, no
Cloudflare API call. Publication is the collection's T08 step and runs through the
hub's own workflow; this repository only produces the artifact and the Worker
configuration. The hub owns the hostname (`projects.logsdon.io`), the sitemap
index, and the slashless redirect; this Worker claims only
`projects.logsdon.io/herdbot/*`.
