# Jump to pane terminal focus

## Intent

Make “Jump to pane” visibly useful by focusing the herdr agent and bringing the terminal that displays its session forward; open an attached client only when no client herdbot opened for that session is available.

## Goals

- Keep `herdbot focus <agent>` as the single entry point used by chat, cards, and menu actions.
- Support Terminal.app, iTerm2, and Ghostty.
- Use the LaunchServices `public.unix-executable` handler as the initial default terminal choice; allow a setting override only after Settings support exists.
- Avoid duplicate clients/windows where herdbot can reliably identify a client it opened.
- Surface a clear failure if terminal automation is unavailable or denied; herdr focus errors remain visible.

## Non-goals

- Guarantee discovery or focus of remote/mosh clients.
- Guess which terminal hosts an arbitrary existing client by process ancestry.
- Add Kitty, WezTerm, or other terminals.
- Change which herdr session is used; it remains `default`.

## Approaches

- Only activate the configured terminal: small and low-risk, but fails when no herdr client is attached.
- Activate or open a tab with per-terminal scripting and track clients herdbot creates: meets the desired flow, chosen after a spike confirms command syntax and notarization requirements.
- Infer the hosting terminal by walking process ancestry: unreliable across remote and multiplexed clients, so rejected.

## Constraints and builder choices

- First implementation must be preceded by the documented spike for all three supported terminals, command syntax, app activation, duplicate tracking, and signing/Automation prompts.
- Terminal automation is best-effort; inability to detect a remote client may create a duplicate and must be stated in UI/help copy.
- Reuse one focus path so every current “Jump to pane” call site receives the same behavior.

## Verification

Automated tests cover terminal selection, command construction, focus errors, and duplicate tracking without invoking Apple Events. On a real Mac, exercise each supported terminal, no-client launch, existing herdbot-opened client, denied Automation permission, and a remote client. Release-signing and notarized permission prompts require manual verification.
