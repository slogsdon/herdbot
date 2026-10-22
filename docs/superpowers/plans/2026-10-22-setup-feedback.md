# Setup window feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The setup window explains itself: "Set up everything" is enabled only when something can be fixed, shows progress while it runs and a result afterwards, and checks that have no automatic fix say what the user must do.

**Architecture:** All logic goes in `HerdbotKit` (`SetupState`, a pure `hint(for:)`), covered by Swift Testing; `SetupView` only renders it. Package: `ui/menubar`.

**Tech Stack:** Swift 6, SwiftUI, Swift Testing (`./scripts/test.sh`). No new dependencies.

## Evidence (reported by Shane 2026-10-22, fresh environment)

- Clicking "Set up everything" does nothing and gives no feedback; its text is black while the other buttons' text is white.
- Why (read from the code): `fixAll()` runs `herdbot setup --yes`, which only runs checks that carry a fix. With `harnesses` failing (no fix exists for it) and `cli-link` failing (fixable, but see the PATH bug in `docs/superpowers/plans/2026-10-22-gui-path.md`), `setup` has nothing to do, exits 0 with no output, `fixAll` discards the output and refreshes into the same state. `SetupView` never shows anything for success.
- The button's `.disabled(!setup.needsAttention || setup.problem != nil)` is true for `worst != ok`, so it is enabled whenever anything is red, including when nothing is fixable.

## Global Constraints

- The app only talks to the CLI. Do not read herdbot files.
- Keep `SetupCLI`'s signatures (`doctor()`, `setup(fix:)`); `setup(fix:)` already returns the CLI's stdout.
- Existing tests keep passing; the full `./scripts/test.sh` hangs only if the pre-existing `ProcessCLI` bug is back (it was fixed in `11f239f`), so run suites with `timeout 120` and report a hang as a hang.
- Do not launch the app or script clicks; never kill a process by name.
- Commits: conventional messages, no AI attribution, explicit `git add` paths, no push.

**Branch/worktree:** `git -C ~/Code/herdbot worktree add .worktrees/setup-feedback -b fix/setup-feedback main` (after `fix/gui-path` is merged, so the app and CLI agree). Work from `ui/menubar`.

## File map

- Modify `Sources/HerdbotKit/SetupState.swift`, `Sources/HerdbotKit/Doctor.swift`.
- Modify `Sources/HerdbotMenu/SetupView.swift`.
- Tests: `Tests/HerdbotKitTests/SetupTests.swift`.

---

### Task 1: State and hints

**Files:** Modify `Sources/HerdbotKit/Doctor.swift`, `Sources/HerdbotKit/SetupState.swift`, `Tests/HerdbotKitTests/SetupTests.swift`.

**Interfaces:**
- Produces: `func hint(for check: DoctorCheck) -> String?`; on `SetupState`: `public private(set) var fixingAll: Bool`, `public private(set) var lastResult: String?`, `public var hasFixable: Bool`.

- [ ] **Step 1: Failing tests** (append to the `Setup` suite; reuse `FakeSetupCLI`, `report`, `okCheck`, `failService`, `warnLink` from `Fixtures.swift`; add `failHarnesses = DoctorCheck(id: "harnesses", status: .fail, message: "none of claude, codex, cursor-agent, pi is on PATH")` to `Fixtures.swift`):

```swift
    @Test func theAllButtonNeedsSomethingFixable() async {
        let onlyUnfixable = SetupState(cli: FakeSetupCLI([report(okCheck, failHarnesses)]))
        await onlyUnfixable.refresh()
        #expect(onlyUnfixable.needsAttention)
        #expect(!onlyUnfixable.hasFixable)
        let fixable = SetupState(cli: FakeSetupCLI([report(failHarnesses, failService)]))
        await fixable.refresh()
        #expect(fixable.hasFixable)
        let fine = SetupState(cli: FakeSetupCLI([report(okCheck)]))
        await fine.refresh()
        #expect(!fine.hasFixable)
    }

    @Test func fixAllReportsWhatSetupDidAndClearsWhenDone() async {
        let cli = FakeSetupCLI([report(failService), report(okCheck)])
        cli.output = "service: installed\ncli-link: linked /opt/homebrew/bin/herdbot"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(state.lastResult == "service: installed\ncli-link: linked /opt/homebrew/bin/herdbot")
        #expect(!state.fixingAll)
    }

    @Test func fixAllWithNothingToDoSaysSo() async {
        let cli = FakeSetupCLI([report(failHarnesses)])
        cli.output = ""
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(state.lastResult == "Nothing to fix automatically.")
    }

    @Test func fixAllKeepsTheErrorAndNoResult() async {
        let cli = FakeSetupCLI([report(failService)])
        cli.failure = "service: build output"
        let state = SetupState(cli: cli)
        await state.refresh()
        await state.fixAll()
        #expect(state.fixError == "setup: service: build output")
        #expect(state.lastResult == nil)
    }

    @Test func hintsExplainChecksWithoutAFix() {
        #expect(hint(for: failHarnesses)?.contains("PATH") == true)
        #expect(hint(for: DoctorCheck(id: "herdr", status: .fail, message: "x"))?.contains("herdr") == true)
        #expect(hint(for: failService) == nil) // it has a fix button instead
        #expect(hint(for: okCheck) == nil)
    }
```

Extend `FakeSetupCLI` in `Fixtures.swift` with `nonisolated(unsafe) var output = "ok"` and make `setup(fix:)` return `output`. Run `./scripts/test.sh --filter Setup` → FAIL.

- [ ] **Step 2: Implement.** In `Doctor.swift`:

```swift
/// What the user must do for a failing check that has no automatic fix.
public func hint(for check: DoctorCheck) -> String? {
    guard check.status != .ok, check.fix == nil else { return nil }
    switch check.id {
    case "herdr": return "Start herdr, then this check clears by itself."
    case "harnesses": return "Install claude, codex, cursor-agent or pi, and make sure it is on your PATH."
    default: return nil
    }
}
```

In `SetupState`: add `public private(set) var fixingAll = false`, `public private(set) var lastResult: String?`, and

```swift
    /// Something the "Set up everything" button can actually do.
    public var hasFixable: Bool { checks.contains { $0.status != .ok && $0.fix != nil } }
```

Rewrite `fixAll()` (keep the login-item registration the current version does after a successful `setup`):

```swift
    public func fixAll() async {
        guard !fixingAll else { return }
        fixingAll = true
        fixError = nil
        lastResult = nil
        var messages: [String] = []
        do {
            let output = try await cli?.setup(fix: nil) ?? ""
            messages = output.split(separator: "\n").map(String.init)
        } catch {
            fixError = "setup: \(error.localizedDescription)"
            await refresh()
            fixingAll = false
            return
        }
        if case .notRegistered = loginItemStatus {
            do {
                try loginItem?.register()
                messages.append("login-item: registered")
            } catch {
                fixError = "login-item: \(error.localizedDescription)"
            }
        }
        await refresh()
        lastResult = messages.isEmpty ? "Nothing to fix automatically." : messages.joined(separator: "\n")
        fixingAll = false
    }
```

Check the current `fixAll` in the file first and keep any behaviour it has that this sketch drops. Run `./scripts/test.sh --filter Setup` → PASS; then the other suites.

- [ ] **Step 3: Commit.**

```bash
git add ui/menubar/Sources/HerdbotKit/Doctor.swift ui/menubar/Sources/HerdbotKit/SetupState.swift ui/menubar/Tests/HerdbotKitTests/Fixtures.swift ui/menubar/Tests/HerdbotKitTests/SetupTests.swift
git commit -m "feat(menubar): setup result, fixable state and hints"
```

---

### Task 2: Render it

**Files:** Modify `Sources/HerdbotMenu/SetupView.swift`.

- [ ] **Step 1:** In the button row:
  - `Button { Task { await setup.fixAll() } } label: { if setup.fixingAll { ProgressView().controlSize(.small) } else { Text("Set up everything") } }`, `.buttonStyle(.borderedProminent)`, `.disabled(!setup.hasFixable || setup.fixingAll || setup.problem != nil)`, and `.help(setup.hasFixable ? "Run every automatic fix" : "Nothing here can be fixed automatically")`.
  - Under the check list, before the error: `if let result = setup.lastResult { Text(result).font(.callout).foregroundStyle(.secondary).textSelection(.enabled) }`.
  - In `row(_:)`, under the message: `if let hint = hint(for: check) { Text(hint).font(.callout).foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true) }`.
- [ ] **Step 2:** Do not hard-code a label colour. A disabled `.borderedProminent` renders dimmed by design, and the new `.disabled` condition removes the case of an enabled-looking button that does nothing. If the label is still unreadable with a light accent colour, leave the system default and say so in the report.
- [ ] **Step 3: Verify.** `./scripts/test.sh` (per suite with `timeout 120` if it hangs), `./scripts/build-app.sh` builds with no new warnings. The window was compiled but not looked at; say so.
- [ ] **Step 4: Commit.**

```bash
git add ui/menubar/Sources/HerdbotMenu/SetupView.swift
git commit -m "fix(menubar): show progress, result and hints in the setup window"
```

---

## Self-review (done)

- Covers: no feedback on click (T1 `lastResult`, T2 text), button enabled with nothing to do (T1 `hasFixable`), no progress (T1 `fixingAll`, T2 spinner), red checks without a fix leave the user stuck (T1 `hint`, T2 row).
- Names consistent: `fixingAll`, `lastResult`, `hasFixable`, `hint(for:)`, `failHarnesses`, `FakeSetupCLI.output`.
- Unverified: the black label text. If it is the dimmed disabled style, T2's `.disabled` condition explains it (it was enabled-looking only when something is red); if it is accent-colour contrast, nothing here changes it.
