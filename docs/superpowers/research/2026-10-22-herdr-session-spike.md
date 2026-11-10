# Herdr named-session lifecycle and socket-resolution spike

Historical run date: 2026-10-24
Scope: Task 1, from `docs/superpowers/plans/2026-10-22-settings-session-spike.md`
Herdr: 0.9.1, protocol 22

## Live recheck — 2026-11-09

Installed `herdr --version` reports 0.9.1. `herdr --help`, `session --help`, and the list/attach/stop/delete help retain the interfaces below. `herdr session list --json` returns `sessions` with `name`, `default`, `running`, `session_dir`, and `socket_path`, including stopped sessions. The rebuilt `herdbot config sessions --json` decodes this live output. `herdr --session default status` reports running, compatible protocol 22, and the default socket.

All lifecycle experiments below are salvaged historical evidence, not rerun proof. No sessions were created, attached, stopped, or deleted during this recheck. The protected `dr-pc-weekly-1109` session was not targeted by any operation. Listing session metadata was the only discovery operation.

The old conclusion needs a safety qualification: bot workspace IDs and room `.workspace` markers are local to a server, but existing startup code clears missing IDs and creates replacements. Resolving another session's socket alone is insufficient to meet the no-recreation requirement. The implementation now binds a home to its first effective socket in `.state/herdr-socket`, rejects a different binding, and rejects legacy socket changes when workspace state or a live daemon exists. Use a separate `HERDBOT_HOME` for another session. Explicit operator `HERDR_SOCKET` remains authoritative for socket selection. Legacy homes without a binding are conservatively treated as default; existing custom-socket homes require an operator-verified adoption before use or a separate home. A newly supplied override cannot prove ownership of existing IDs.

Named-session creation/cleanup, normal TUI attachment, and installed launchd propagation were not reverified. Shane owns the interactive checks. The existing supported discovery contract is sufficient to implement the setting with these guards; it does not prove interactive attachment or installed-service behavior.

## Historical result

An optional named session is viable for future Herdbot starts if the selected session is resolved to its socket and passed as `HERDR_SOCKET`. Herdr's supported `session list --json` provides that socket path, and the Herdbot `doctor` command successfully connected to a named session through the existing `herdr-client`. Keep `default` as the shipped value. Do not change or migrate existing bot workspaces: the observed `herdbot-dev` workspace remained on the default Herdr server while the named server ran.

The spike did not verify an interactive attach in the visible Herdr TUI. This task shell had no TTY; `session attach` started the named server, then exited with `Inappropriate ioctl for device`. No UI was launched by that noninteractive invocation. Treat visible TUI attachment as an outstanding manual check before designing UI behavior around named sessions.

## Supported CLI and JSON

`herdr 0.9.1` help reported:

```text
herdr session list [--json]
herdr session attach <NAME>
herdr session stop [--json] <NAME>
herdr session delete [--json] <NAME>
```

`list`, `stop`, and `delete` support JSON. `attach` has no JSON option in its help. Before creating the fixture, `herdr session list --json` returned:

```json
{"sessions":[{"default":true,"name":"default","running":true,"session_dir":"/Users/shane/.config/herdr","socket_path":"/Users/shane/.config/herdr/herdr.sock"}]}
```

The JSON fields provide supported discovery of session name, default/running status, session directory, and API socket path. The disposable session output later showed `session_dir` `/Users/shane/.config/herdr/sessions/spike-wkylr5` and `socket_path` `/Users/shane/.config/herdr/sessions/spike-wkylr5/herdr.sock`.

`herdr --session spike-wkylr5 status` before startup reported `server: status: not running` and the computed socket path. It did not create a session directory or make the session appear in the list. Therefore resolving a future socket path does not itself prove a server exists.

## Startup and attach behavior

The first `herdr session attach spike-wkylr5` attempt was rejected with:

```text
error: nested herdr is disabled by default.
```

The session inherited Herdr's nested-launch guard because this command ran in a Herdr-managed pane. To isolate the experiment, I retried with a temporary `/tmp` config setting `[experimental] allow_nested = true`; the user's `~/.config/herdr/config.toml` was not changed. The config was removed after each command.

`HERDR_CONFIG_PATH=<temporary-config> timeout 4 herdr session attach spike-wkylr5` then printed terminal control sequences and exited 1 with:

```text
herdr: Inappropriate ioctl for device (os error 25)
```

Despite the non-TTY attach error, `herdr session list --json` showed the named server running and both API and client socket files existed. Thus `session attach` starts the server before attempting terminal UI attachment. No Herdr GUI process appeared. A separate headless probe, `HERDR_CONFIG_PATH=<temporary-config> timeout 5 herdr --session spike-wkylr5 server`, printed `herdr server running` and named socket/log paths; timeout then stopped that process. Interactive TUI visibility and usability remain unverified.

## herdr-client feasibility

Source inspection in `crates/herdr-client/src/lib.rs` confirms `Client::from_env()` reads `HERDR_SOCKET`, falling back to `~/.config/herdr/herdr.sock`, and `Client::ping()` connects over Unix socket and checks the protocol. While the named session was running, this live check was run:

```sh
HERDR_SOCKET="$HOME/.config/herdr/sessions/spike-wkylr5/herdr.sock" cargo run -q -p herdbot -- doctor --json
```

It returned JSON with `{"id":"herdr","message":"herdr 0.9.1","status":"ok"}`, confirming the existing Rust client reached the named server and negotiated the compatible protocol. Doctor also reported its normal environment checks; those findings were unrelated to the socket probe.

## Default workspace observation

While `spike-wkylr5` was running, `herdr api snapshot` (default session) still reported Herdr 0.9.1 workspace `w3H`, label `🤖 herdbot-dev`, with two panes whose CWD was `/Users/shane/Code/herdbot`. The default session's socket remained `/Users/shane/.config/herdr/herdr.sock`. No default workspace, bot workspace, or production configuration was modified.

## Cleanup proof

Only `spike-wkylr5` was stopped and deleted:

```sh
herdr session stop spike-wkylr5 --json
```

returned `{"session":{"default":false,"name":"spike-wkylr5","running":false,...},"stopped":true}`. Then:

```sh
herdr session delete spike-wkylr5 --json
```

returned `{"deleted":true,"session":{"default":false,"name":"spike-wkylr5",...}}`. A final `herdr session list --json` contained only the running default session, and `test ! -e ~/.config/herdr/sessions/spike-wkylr5` passed (`fixture_directory_absent`). `herdr --session default status` confirmed the default server and compatibility were still running.

## Recommendation against the Settings spec

The Settings spec's future setting is technically supportable for new Herdbot starts: retain `default` as the default, use the supported session JSON to resolve `socket_path`, and supply it through `HERDR_SOCKET`. Do not silently migrate existing workspaces or imply that changing the setting reattaches their current panes. The separate UI design should define when a newly selected session takes effect and should verify normal interactive `herdr session attach` behavior. Keep `default` as the initial value; interactive attachment remains a manual check. The setting must enforce the ownership guard described in the live recheck.

No product code, production config, existing workspace, or default session was changed. No commit was made.
