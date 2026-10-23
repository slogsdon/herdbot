# Settings app progress

Plan: `docs/superpowers/plans/2026-10-22-settings-app.md` (Tasks 1–2)
Spec: `docs/superpowers/specs/2026-10-22-settings-design.md`
Contract: `docs/superpowers/plans/2026-10-22-settings-config.md` (read from the main checkout
`/Users/shane/Code/herdbot/docs/…`; not copied into this worktree, nothing here needs it at build time).
The `set`/`unset` half and the `map` value shape (`FROM=TO`) were read from the sibling
`.worktrees/settings-config-write/crates/herdbot/src/config.rs`, which is the only place either is
implemented; this worktree has no Rust changes.
Worktree: `/Users/shane/Code/herdbot/.worktrees/settings-app-ui`, branch `feature/settings-app-ui`, base `3e8199c`
Toolchain: Swift 6.4 (swiftlang-6.4.0.34.1), macOS 27.0 host, package platform macOS 15

## Task 1: Settings models and CLI seam — done

- [x] Baseline `cd ui/menubar && ./scripts/test.sh` green: **116 tests in 15 suites**, 30.5 s.
- [x] Failing tests first: fixtures + decode tests + process argument tests.
- [x] Red run `./scripts/test.sh`: build failed — `cannot find 'SettingsList' in scope`,
      `cannot find 'SettingsEntry' in scope` (Fixtures.swift:130/135). Recorded, not committed.
- [x] Implemented `SettingDescriptor`, `SettingValue`, `SettingType`, `SettingApply`,
      `SettingsList`/`SettingsEntry` envelopes, `SettingsCLI`, and the `ProcessCLI: SettingsCLI`
      extension.
- [x] Green run `./scripts/test.sh`: **128 tests in 15 suites**, no warnings, 0.3 s test time.
      (116 + 12 new: 6 decode, 6 process.)
- [x] App target built, since `test.sh` only runs `swift test`:
      `./scripts/build-app.sh` -> `built build/Herdbot.app (arm64, version 0.1.2)`, and
      `test -x build/Herdbot.app/Contents/MacOS/HerdbotMenu` passed.

## Contract as implemented

- `herdbot config list [--bot SLUG] --json` -> `{"version":1,"scope":"global|bot","settings":[Setting,…]}`
  decoded into `[SettingDescriptor]`.
- `herdbot config get|set|unset … [--bot SLUG] --json` -> `{"version":1,"scope":…,"setting":Setting}`
  decoded into one `SettingDescriptor`.
- Argument vectors (tests assert them exactly; `--json` before `--bot`, both flags position
  independent in the CLI):
  - `config list --json` / `config list --json --bot helper`
  - `config get <key> --json [--bot helper]`
  - `config set <key> <value> --json [--bot helper]` (a value with spaces stays one argv entry)
  - `config unset <key> --json [--bot helper]`
- `SettingDescriptor` fields: `key`, `type` (`enum|string|integer|boolean|map`), `description`,
  `default` -> `defaultValue`, `value`, `choices` (absent -> `[]`), `apply`
  (`immediate|next-bot-start|daemon-restart`, with a `label` for apply-timing copy).
- Every field but `choices` is required, so a payload this build cannot read fails instead of
  producing a silently empty form.
- `SettingValue` covers the four JSON shapes plus `null`/unset: `.text`, `.integer`, `.flag`,
  `.table([String: String])`, `.unset`, with `.text` rendering (table = sorted `KEY=VALUE` lines,
  unset = empty) for form fields.
- Errors: non-zero exit keeps the CLI stderr message verbatim (`CLIError.failed`); a contract
  version other than 1 throws `CLIError.unsupported` ("… update herdbot or this app"); any other
  decode failure throws `CLIError.failed("herdbot config <cmd> printed settings this app cannot
  read; update herdbot or this app")`.
- No TOML parsing, no filesystem writes in Swift; the CLI stays the only I/O boundary.

## Files touched (this task)

- `ui/menubar/Sources/HerdbotKit/Stream.swift` (+164): settings models, envelopes, `SettingsCLI`,
  `supportedSettingsVersion`.
- `ui/menubar/Sources/HerdbotKit/ProcessCLI.swift` (+40): `ProcessCLI: SettingsCLI`.
- `ui/menubar/Tests/HerdbotKitTests/Fixtures.swift` (+34): `globalSettingsJSON`, `botSettingsJSON`,
  `botMemorySettingJSON`, `settings(from:)`, `setting(from:)`.
- `ui/menubar/Tests/HerdbotKitTests/DecodingTests.swift` (+81): 6 tests.
- `ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift` (+58): 6 tests.

## Task 2: Settings navigation and forms — done except manual UI check

- [x] Failing tests first: new `Tests/HerdbotKitTests/SettingsTests.swift` (16 tests) with
      `FakeSettingsCLI` + a `Gate` actor for mid-flight saves.
- [x] Red run `./scripts/test.sh`: build failed — `cannot find 'SettingsState' in scope`
      (SettingsTests.swift:341, plus follow-on key-path/member errors). Recorded, not committed.
- [x] Implemented `SettingsState` (`Sources/HerdbotKit/SettingsState.swift`) and `SettingsView`
      (`Sources/HerdbotMenu/SettingsView.swift`), wired the sidebar row and status-menu item.
- [x] Mid-task red→green for reload semantics: `reloadPicksUpAValueChangedElsewhereButKeepsUnsavedEdits`
      failed against the first implementation (drafts were only seeded once), then passed after
      adding per-scope baselines. 144 tests in 16 suites, no warnings.
- [x] `./scripts/build-app.sh` -> `built build/Herdbot.app (arm64, version 0.1.2)`; the binary exists
      and is executable. No warnings from either build.
- [ ] Manual UI inspection (for Shane; cannot be done by the agent): see the Settings manual check
      in `ui/menubar/README.md`.

### State behaviour

- Scopes: `.global` and `.bot(slug)`; `present(_:)` shows the screen and loads, `dismiss()` returns to
  the chat without discarding the loaded form. `select(_:)` only commits `scope` after a successful
  `config list`, so a failed scope switch leaves the previous scope, descriptors and drafts on screen
  with `problem` set (load error).
- Drafts are kept per scope, with a baseline (the value the draft started from) per key, so:
  a failed save/unset keeps the typed text; switching global ↔ bot ↔ bot keeps typed text; `reload()`
  refreshes fields the user has not edited and keeps the ones they have.
- `save`/`unset` capture the scope they started in and drop their answer if the scope changed while
  the CLI ran, so a bot's returned descriptor can never land in another bot's or the global form.
  The per-key `status` is `.idle/.saving/.saved/.failed(message)`; failures carry the CLI's stderr
  verbatim. Validation lives only in the Rust CLI — Swift adds one guard: an empty `FROM=TO` entry is
  rejected locally with a hint and no CLI call.
- Map control: `mapEntries(key)` lists current entries sorted by `FROM`; `setMapEntry(key, text)` runs
  `config set <key> FROM=TO` (add or replace one entry) and clears the one-entry field on success;
  `resetMap(key)` runs `config unset <key>`. No per-entry delete: the CLI contract has no way to remove
  one entry, so none was invented.
- Types → controls: `enum` picker (an unknown current value still shows), `boolean` switch,
  `integer`/`string` text field, `map` entry field plus the entry list. Every row shows the
  description, `Default: …`, the apply timing (`SettingApply.label`) and its own status; a saved row
  repeats the apply timing ("Saved; takes effect when the bot next starts").

### Navigation

- Sidebar: a Settings row below the bot/room sections opens global settings and is highlighted while
  shown; selecting a conversation dismisses Settings first. The List's selection binding is empty
  while Settings is presented, so no conversation looks selected.
- Status menu: `Settings…` opens the chat window at Settings (`openSettings(nil)` in `App.swift`, which
  also takes an optional bot slug — the navigation path the plan asks for).
- `ChatWindow` shows `SettingsView` in the detail pane instead of `ChatView` when
  `SettingsState.isPresented`, and passes the existing split-view/sidebar frame and window behaviour
  through unchanged.

## Files touched in Task 2

- `ui/menubar/Sources/HerdbotKit/SettingsState.swift` (new, 244 lines): scopes, drafts/baselines,
  per-key status, save/unset, map entries, presentation flag.
- `ui/menubar/Sources/HerdbotMenu/SettingsView.swift` (new, 183 lines): metadata-driven form, scope
  picker, load-error banner, per-row status. New file rather than inlining into `ChatView.swift`,
  matching the existing `SetupView.swift` convention; the plan's file list did not name it.
- `ui/menubar/Tests/HerdbotKitTests/SettingsTests.swift` (new, 375 lines): fake settings CLI, `Gate`,
  16 tests (load, no CLI, select bot, load error, load-error recovery, per-bot save scoping, rejected
  save, unset, failed unset, stale save after scope switch, drafts per scope, reload refresh, map
  entries/set/reset, empty map entry, present/dismiss, edit clears status).
- `ui/menubar/Sources/HerdbotMenu/ChatView.swift`, `SidebarView.swift`, `MenuContent.swift`,
  `App.swift`: navigation wiring and the `SettingsState` instance.
- `ui/menubar/README.md`: Settings section plus a manual check list for Shane.

## Not done, deliberately

- Manual UI inspection: no automated UI test target exists for the app; the forms were verified by
  unit tests plus a building app target, not by clicking through them.
- Herdr session setting and its `default` value: gated on the named-session spike and the
  session-setting CLI contract; nothing session-shaped was added.
- Routing table: excluded from the first release.
- Per-entry deletion of `paths` entries: deferred per the plan; the CLI has no such operation.
- Rust CLI changes: none in this worktree (the CLI lives in the `settings-config*` worktrees).
- No commit, push, merge, install, or launch was performed.
