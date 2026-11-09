# Terminal focus and attach spike

**Status: static spike complete; live checks open.** Static inspection (dictionaries, herdr CLI output, local app signatures, herdbot build scripts, Apple docs) was performed and then re-verified from scratch on 2026-11-09 against macOS 27.2 (26B123) in the salvage pass. Live terminal automation, Apple Events permission prompts, and signed-build checks were **not** executed (they would raise permission prompts; the spike directive forbids that). Do not start product implementation until the live checks below are done by Shane.

Claims here are tagged: plain text = verified from local evidence or fetched vendor docs on 2026-11-09; `Likely:` = inference with stated basis; `Assumption:` = chosen to proceed.

## Installed applications (re-verified 2026-11-09)

Host OS: macOS 27.2 (build 26B123), from `sw_vers`. The original capture ran on 2026-10-26 under macOS 27.0.1 (26A434); since then this machine updated macOS and Ghostty 1.3.1 → 1.4.0. Terminal.app and iTerm2 are unchanged.

| App | Bundle ID | Version | Scripting dictionary |
|---|---|---:|---|
| Terminal.app | `com.apple.Terminal` | 2.15 | `/System/Applications/Utilities/Terminal.app/Contents/Resources/Terminal.sdef` |
| iTerm2 | `com.googlecode.iterm2` | 3.7.6 | `/Applications/iTerm.app/Contents/Resources/iTerm2.sdef` |
| Ghostty | `com.mitchellh.ghostty` | 1.4.0 | `/Applications/Ghostty.app/Contents/Resources/Ghostty.sdef` |

Versions and bundle IDs read from each app's `Info.plist` via PlistBuddy. The `sdef` utility cannot run on this machine: Command Line Tools are active, not full Xcode (`xcode-select: error: tool 'sdef' requires Xcode`); the bundled XML dictionaries were parsed directly instead.

## Per-app interfaces (dictionary-verified, not live-tested)

These are interface declarations read from the shipped `.sdef` files — evidence that the commands exist and their argument shapes, not proof they succeed or that the app activates as expected.

- **Terminal.app:** commands include `make`, `do script` (direct parameter: command text; optional `in` parameter accepting `tab`, `window`, or null; result: `tab`), and `do script in new tab` (optional `next to` tab/window target, defaults to frontmost window or a new window). Tab elements expose `tty`, `processes`, `busy`, `selected`, `custom title`, and content properties. That is enough to build "activate + run `herdr session attach default` in a new tab" as one command. Whether `do script` with a null/absent `in` target opens a new window versus reusing the front tab, and which surface ends up frontmost: live check needed.
- **iTerm2:** `create tab with default profile` takes a window specifier as direct parameter plus an optional `command` text parameter ("Shell command to run") and returns a `tab`; `create window with default profile` likewise returns a `window`. Also present: `create window with profile`, `split horizontally/vertically …`, `select`, `write`, and a session class exposing `id`, `unique ID`, `tty`, `name`, `is processing`, `is at shell prompt`, `contents`. The `command` parameter means the attach command can be passed at tab creation — no post-creation text injection. `unique ID` is the candidate handle for tracking a Herdbot-created client (its stability across iTerm restarts is `Likely:` but untested).
- **Ghostty 1.4.0:** commands include `new window`, `new tab` (optional `in` window, optional `with configuration` surface configuration; result: `tab`), `activate window`, `select tab`, `focus`, `input text`, `send key`, `split`, `close tab`/`close window`. Classes expose `id`, `selected tab`, `focused terminal`, and per-terminal `working directory`, `pid`, `tty`. Correction to the earlier capture: the `surface configuration` element **does** declare `initial working directory`, `command`, `initial input`, and `wait after command` properties, and `new tab`/`new window` accept a surface configuration — so an initial command may be set without injecting text after creation. Whether AppleScript can set those configuration properties and whether `command` runs as the shell's initial command (vs. a Ghostty config key) is untested; the text-injection path (`input text` + `send key`) remains the documented fallback.

Sources: the three bundled `.sdef` files listed above, parsed on 2026-11-09. Ghostty's hosted AppleScript doc page could not be fetched (404 on both tried paths); the shipped dictionary is the local source of record.

## Duplicate detection (herdr 0.9.1 — re-verified)

- `herdr status client --json` from a non-attached shell returns client/version/endpoint fields with `"session": null`. `herdr status server --json` reports the server but no client list.
- `herdr session list --json` exposes per session only: `name`, `default`, `running`, `session_dir`, `socket_path`. No client identifiers, no client inventory.
- `herdr api snapshot` (read-only socket call) returns server-side session state only — workspaces, tabs, panes, agents, focus ids, and per-pane `terminal_id`/`terminal_title`. `terminal_id` (e.g. `term_65f659705a6381`) identifies the pane's internal PTY, not the hosting terminal app. There is no attached-client inventory anywhere in the output.

**Conclusion (verified against herdr 0.9.1 CLI output):** Herdbot cannot ask herdr which local clients are attached, so it cannot identify a Herdbot-opened client after relaunch from herdr data alone. Remote/mosh clients are not discoverable, consistent with the design's non-goal. Duplicate avoidance must rely on Herdbot recording the client identity itself at creation time in terminal-app terms (iTerm2 `unique ID`, Ghostty tab `id`/window, Terminal tab — see per-app notes; Terminal.app's dictionary shows no stable tab `id` property, so cross-relaunch tracking there is `Likely:` weaker than iTerm2). Whether those app-side ids persist across app restarts was not validated.

## Apple Events, signing, and Herdbot's current release configuration

Verified against Apple's live documentation on 2026-11-09:

- `NSAppleEventsUsageDescription`: "This key is required if your app uses APIs that send Apple events." ([Apple docs](https://developer.apple.com/documentation/bundleresources/information-property-list/nsappleeventsusagedescription) — the underscore-path URL used in the original capture 302-redirects here and still resolves).
- `com.apple.security.automation.apple-events`: "A Boolean value that indicates whether the app may prompt the user for permission to send Apple events to other apps." ([Apple docs](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.security.automation.apple-events))
- Hardened Runtime / notarization: "To upload a macOS app to be notarized, you must enable the Hardened Runtime capability." ([Apple docs](https://developer.apple.com/documentation/Security/hardened-runtime)). `Likely:` applies to Developer-ID apps distributed outside the App Store, which is Herdbot's release path.

Local app evidence (2026-11-09, Ghostty 1.4.0): `Info.plist` contains `NSAppleEventsUsageDescription` ("A program running within Ghostty would like to use AppleScript."); `codesign` reports `flags=0x10000(runtime)` (hardened) and an entitlement list containing `com.apple.security.automation.apple-events` with no app-sandbox entitlement — a working local precedent that a non-sandboxed terminal app ships the usage string plus the automation entitlement under the hardened runtime. iTerm2 also reports the hardened runtime flag.

Herdbot current state (paths corrected after the `ui/menubar` → `ui/macos` rename; verified at HEAD `ff33f06`):

- `ui/macos/Info.plist` (bundle `dev.herdbot.menu`) has **no** `NSAppleEventsUsageDescription`.
- `ui/macos/scripts/build-app.sh`: signs with `codesign --force --sign "$SIGN_IDENTITY"`; for any non-ad-hoc identity it adds `--options runtime --timestamp`. No `--entitlements` flag anywhere; no `.entitlements` file exists under `ui/macos/`.
- `scripts/release.sh`: requires a `Developer ID Application` identity and a notarytool profile, submits `notarize.zip`, staples; `--dry-run`/`--no-notarize` fall back to ad-hoc.

Therefore the release configuration does not yet establish Apple Events readiness: the usage string and (for the hardened runtime path) the automation entitlement are both missing. The permission prompt, the denied-permission fallback, and the notarized-build prompt text were **not** exercised — `Assumption:` they behave like any other TCC-gated Apple Events sender; a scratch signed build must confirm.

## Open gates: manual checks Shane must run (and what each must show)

Not runnable in this spike: any Apple Events send from an unsigned/ad-hoc context would raise a TCC prompt, and the plan's live verification requires executing automation against real terminals. Live checks still required, in a disposable window/tab only, for each of Terminal.app, iTerm2, Ghostty:

1. **Activate + new tab running the attach.** Record OS version (now 27.2) and app version; from a scratch Herdbot/`osascript` context, activate the app and create a new tab/window running `herdr session attach default`. Must show: the command actually executes in the new surface, the new surface becomes frontmost/key, and the herdr TUI renders attached. Expected syntax per app is in "Per-app interfaces" above.
2. **No unrelated-tab interference.** Must show: focus/selection touches only the Herdbot-created surface; for Terminal.app specifically, that `do script`'s target selection never types into a pre-existing tab.
3. **Duplicate tracking.** Must show: the id returned/recordable at creation (iTerm2 session `unique ID`, Ghostty tab `id`, Terminal tab — index/name only?) can be re-queried later to confirm the client still exists, both after the tab closes and after the terminal app restarts. Any id that does not persist ⇒ that app falls back to "may open a duplicate" copy.
4. **Permission matrix.** In a signed (Developer ID, hardened, entitlements per above) scratch build: first-run prompt shown with the usage string text; on **Deny**, Herdbot surfaces the error and the manual `herdr session attach default` action instead of silently failing; on **Allow**, activation works from the notarized/stapled build, not just a locally signed one. Also test the "app not installed" and remote/mosh cases (duplicate expected, copy must say so).

Historical note (unverified by this pass): the 2026-10-26 capture records that the computer-use interface refused to drive Ghostty, Terminal.app, and iTerm2 with a safety rejection, and that delegated runs were blocked; no terminal was launched and no Apple Events were sent in either spike pass.

## Review against the design gates

- **Three supported terminals:** dictionaries confirm each declares tab/window creation and command/input operations, with iTerm2 and Terminal.app providing an initial-command path and Ghostty having a `command`/`initial input` surface-configuration property plus a text-injection fallback. Activation and actual attach remain gates 1–2 above. The design is feasible on paper for all three.
- **Duplicate avoidance:** herdr exposes no client inventory (verified), so tracking must be Herdbot-side and per-app. Do not claim reliable duplicate avoidance across relaunch or for remote/mosh clients in any copy.
- **Safe fallback:** denial/unavailable behavior unexercised (gate 4). The scoped fallback below is proposed so a denied path is still a designed outcome, not a bug.
- **Signing:** gap identified and concrete (usage string + entitlement missing from `ui/macos`); fixable before the first implementation release, verifiable only via gate 4.
- **No product change:** confirmed — research-only; no product files, no commits.

**Proposed scoped fallback (needs approval before implementation):** when no verifiable Herdbot-created client identity is available, do not select an arbitrary existing tab or silently open a duplicate. Keep the existing Herdr pane-focus result (`herdr agent focus` via the socket API, already implemented), explain that terminal automation could not identify a safe client, and show `herdr session attach default` as a manual action. If a separate explicit "Open new client" action is later offered, disclose that it may create a duplicate. On denied or unavailable Apple Events, show the reason and the manual action rather than sending input to an unknown terminal surface.

## Follow-up implementation recommendation

`Assumption:` Shane accepts gate 1–4 results as specced below; if any gate fails, apply the scoped fallback for that app only.

1. **Single focus path.** Keep `herdbot focus <agent>` as the only entry point (chat, cards, menu). Extend it to: call `herdr agent.focus` first (existing behavior, errors still visible); on success, best-effort terminal fronting behind one trait `TerminalDriver { activate(); open_attached_client(cmd); find_tracked_client() }` so per-app quirks never leak into callers.
2. **Terminal selection.** Resolve the target app via LaunchServices `public.unix-executable` default handler at call time (spec's initial choice); map bundle id → driver (`com.apple.Terminal`, `com.googlecode.iterm2`, `com.mitchellh.ghostty`); any other handler falls back to "focus succeeded, terminal not supported" copy. Defer the Settings override until Settings support exists.
3. **Per-app command construction (unit-testable without Apple Events):**
   - Terminal.app: `tell app "Terminal" to activate` then `do script "herdr session attach default" in new tab` variant; track by tab `tty`/`processes` heuristics only for the tab Herdbot just created, never for user tabs.
   - iTerm2: `tell app "iTerm" to activate`, then `create tab with default profile command "herdr session attach default"` (dictionary-verified `command` parameter); persist the returned session's `unique ID`.
   - Ghostty: `new tab with configuration` after creating a `surface configuration` and setting `command`/`initial working directory`; if live checks show those properties are not settable, fall back to `new tab` + `input text` + `send key` on the returned tab. Persist the tab `id`.
4. **Duplicate tracking.** Herdbot persists created-client records (app bundle id + app-side id + session name) in its own state under the herdbot home. On focus: if a record exists and the app still reports that id alive, `activate` + select that surface only; otherwise create a new tab and say in help/UI copy that a duplicate client may be created (remote/mosh clients can never be found — verified herdr limitation).
5. **Failure copy.** Non-zero `osascript` exit, error `-1743` (not authorized), or app-not-installed must each render a distinct message with the manual `herdr session attach default` line. Never retry into a different app.
6. **Release-blocking plist/entitlement work (do in the first implementation PR):** add `NSAppleEventsUsageDescription` to `ui/macos/Info.plist`; add `com.apple.security.automation.apple-events` via a new `ui/macos/herdbot.entitlements` wired into `build-app.sh`'s `codesign` (`--entitlements`), keeping the existing `--options runtime --timestamp` behavior; then run gate 4 against a notarized scratch build. Ghostty 1.4.0's shipped combination is the local reference config.
7. **Automated test surface (per the design's Verification section):** terminal resolution from bundle id, command-string construction per driver, record persistence/find-alive logic, and error→message mapping — all pure-Rust, no Apple Events invoked. Everything else is the manual matrix above.

Suggested plan split: (a) drivers + tracking + focus path with unit tests after gates 1–3 pass; (b) signing/entitlements + permission matrix (gate 4) before any release containing (a).
