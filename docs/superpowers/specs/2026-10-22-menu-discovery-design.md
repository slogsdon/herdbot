# Menu discovery and setup visibility

## Intent

Make the status menu a fast route to the five most recently active bots and keep setup visible only when the install needs attention. Every bot remains reachable from the chat window.

## Goals

- Show at most five bots, ordered by conversation `last_at` descending; bots without activity sort after active bots, then by display name and slug.
- Clicking a bot opens the chat window with that bot selected, using the existing open-chat path.
- Show Setup when a report needs attention or setup has failed; hide it before the initial report and when all checks pass.

## Non-goals

- No “All bots…” menu item; the chat window is the route to bots outside the top five.
- No changes to the chat sidebar, status protocol, or setup workflow.

## Approaches

- Add menu-local sorting and visibility conditions: smallest change, but can be harder to unit test.
- Put the two derived rules in existing HerdbotKit state: preferred for deterministic tests and a thin SwiftUI menu.

Use existing state helpers if they fit without adding a new abstraction; test sorting and setup visibility as pure derived behavior.

## Constraints and builder choices

- **Assumption:** “Most recently active” means conversation `BotView.lastAt`, not last time a chat was opened. If wrong, recency requires a separate persisted app-open signal.
- Use `BotView.lastAt` already supplied by the snapshot; do not infer recency from app window history.
- Ties and bots without activity use display name, then slug, for stable order.
- Preserve current ordering of non-bot menu sections.
- A report error (`setup.problem`) must leave Setup reachable so the user can diagnose it.

## Verification

Swift unit tests cover empty/under-five/over-five rosters, missing timestamps, timestamp ties, chat selection, and setup states (no report, healthy report, attention, error). Manually verify a menu click opens the selected bot.
