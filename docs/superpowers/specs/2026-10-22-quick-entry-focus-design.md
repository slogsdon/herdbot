# Quick-entry focus lifecycle

## Intent

Make ⌥Space show only the quick-entry panel, and dismiss it when the user leaves it, while preserving dictation input when MacWhisper briefly takes focus.

## Goals

- Showing the panel must not raise an already-open chat window.
- The panel remains key and accepts keyboard and dictation input.
- Escape, send, and hotkey toggle retain their current behavior.
- A brief focus transfer to a dictation HUD does not dismiss the panel; sustained focus elsewhere does.

## Non-goals

- No change to quick-entry layout, send semantics, hotkey registration, or chat behavior.
- No hard-coded list of dictation app bundle identifiers.

## Approaches

- Non-activating panel: likely avoids raising the chat window but risks breaking the active-app dictation path.
- Keep activation and restore window ordering: can preserve dictation but depends on fragile window ordering.
- Keep the key panel, prevent other app windows from being raised where AppKit permits, and dismiss after a cancellable short focus-loss grace period: chosen as the narrowest approach that preserves the existing dictation requirement; validate on macOS before committing to AppKit details.

## Constraints and builder choices

- Existing `QuickEntryController.show()` activates the app and makes the borderless panel key. `hidesOnDeactivate` is intentionally false for dictation.
- A returning focus during the grace period cancels dismissal. Clicking another app, ⌘Tab, another app window, and Mission Control must eventually dismiss.
- If AppKit cannot separate panel activation from raising the chat window, document the observed behavior and choose the least fragile tested fallback.

## Verification

Add unit tests around any extracted, deterministic focus policy. Manual MacWhisper checks are required with chat open and closed, plus ⌘Tab, click-away, other app window, and Mission Control. Confirm dictated text still lands in the panel.
