# Herdbot settings and lifecycle controls

## Intent

Give users one Settings destination in the chat app for supported herdbot and per-bot configuration, setup checks, and daemon lifecycle actions, while keeping TOML as the source of truth and using the CLI as the app boundary.

## Goals

- Add Settings entry points from the chat sidebar and status menu.
- Read and edit supported global `herdbot.toml` settings and per-bot `bot.toml` settings through `herdbot config`; expose descriptions, types, defaults, and apply timing.
- Preserve comments, key ordering, and unrelated content when editing TOML.
- Reopen the existing setup checklist from Settings.
- Offer explicit daemon disable and uninstall actions with distinct effects and confirmations. Uninstall preserves bot data by default; purge retains the existing typed-path confirmation.
- Add a herdr session setting whose initial/default value is `default`; changing it must not automatically move existing bot workspaces.

## Non-goals

- Editing `routing.toml` in the first release.
- Implementing session selection before the herdr lifecycle and existing-workspace behavior spike is complete.
- Replacing TOML, introducing a database, or exposing arbitrary file editing.

## Approaches

- Parse/edit TOML in Swift: rejected because it duplicates CLI config knowledge and creates a second mutation boundary.
- Add CLI-backed typed config operations and metadata, then build the UI over JSON: chosen to keep validation and TOML preservation in Rust.
- Expose raw TOML in a text editor: smaller UI, but unsafe for preserving comments and validating values.

## Phases

1. Config registry and CLI operations, including comment-preserving edits and per-bot keys.
2. Settings navigation and forms using the CLI JSON contract.
3. Setup rerun and daemon disable/uninstall actions with status refresh and clear confirmation.
4. Herdr session setting, gated on a spike that confirms creation, socket resolution, and behavior for existing workspaces. It defaults to `default`; no automatic workspace migration.

## Constraints and builder choices

- Use the existing Rust CLI as the only settings I/O boundary; use a format-preserving TOML editor such as `toml_edit` if confirmed compatible with the repository.
- Validate paths, enums, and numeric values at the CLI boundary. Reject unknown keys and malformed values without modifying files.
- Describe whether a setting applies immediately, on the next bot start, or after daemon restart. Offer a restart action only where required.
- Per-bot settings are in; routing table is out. Herdr session defaults to `default`; changing sessions never silently migrates existing workspaces.
- Keep disable reversible and separate from uninstall. Preserve existing uninstall and purge safeguards.

## Verification

Rust tests exercise metadata, parsing, validation, preservation of comments/order/unrelated keys, atomic failure behavior, and per-bot scoping. Swift tests cover JSON decoding, forms, load/save errors, setup rerun, and lifecycle confirmation. Manual checks verify setting apply timing and visible destructive-action copy.
