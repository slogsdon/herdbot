# Quick Entry Redesign (UI phase 2b) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the quick-entry panel like Spotlight and Alfred: a large full-width message field, a quiet row beneath it, no buttons, a result overlay, and the system's own material and accessibility settings.

**Architecture:** The panel becomes a borderless `NSPanel` whose content is `NSGlassEffectView` (Liquid Glass, macOS 26+) or an `NSVisualEffectView` (earlier), hosting a SwiftUI view. The material, not the app, honours Reduce Transparency; the app reads Increase Contrast and Reduce Motion itself. Behaviour (hotkey, send, focus, dictation) is unchanged.

**Tech Stack:** Swift 6, SwiftUI, AppKit, Swift Testing. macOS floor stays 14.

Spec: `docs/superpowers/specs/2026-10-21-ui-layer-design.md` (Phase 2). Builds on `docs/superpowers/plans/2026-10-22-quick-entry.md`.

## Global Constraints

- Branch `feature/ui-quickentry`, worktree `/Users/shane/Code/herdbot/.worktrees/ui-quickentry`. Work only there.
- Shane approved committing each task below: explicit paths only (never `git add -A` or `git add .`), the conventional message given, **no AI or tool attribution**, no push, no merge.
- Never run `cargo install`, never restart the daemon, never launch the menu bar app (a running instance of it belongs to Shane; do not kill it).
- Swift: 4-space indent, no `@State` (a macro whose plugin is missing under Command Line Tools), zero Swift compiler warnings (linker `search path ... not found` warnings are an environment quirk and fine).
- Test with `./scripts/test.sh` from `ui/menubar`, never bare `swift test`.
- Layout, from Shane: large message field (full width), a "To" dropdown below it aligned right in the same style, no Cancel/Send buttons, a success/error overlay covering the whole panel, 8 pt padding, a line-art gear icon for settings, a hairline border. Use only system semantic colours and materials.
- Baseline before this plan: Swift 25 tests pass. Rust untouched.
- Keep your ledger in `.superpowers/sdd/ui-quickentry/progress.md` (untracked; do not commit it).

All code below was compiled, its tests run, and the panel rendered and screenshotted in a scratch copy before this plan was written. Reproduce it exactly; if a step fails, report it, don't redesign.

---

### Task 1: state for the result overlay

**Files:**
- Modify: `ui/menubar/Sources/HerdbotKit/QuickEntry.swift`, `ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift`

**Interfaces:**
- Produces: `QuickEntryState.recipientLabel: String?` (the current recipient as shown, e.g. `Alpha` or `Crew (room)`) and `QuickEntryState.dismissFailure()` (clears a `.failed` status only; the text is kept).

- [ ] **Step 1: Write the failing tests**

Apply to `ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift` (the new tests go before `resetStartsAFreshMessage`):

```diff
--- a/ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift
+++ b/ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift
@@ -82,6 +82,29 @@
         #expect(await state.send(using: FakeCLI()))
     }
 
+    @Test func labelsTheCurrentRecipient() {
+        let (state, _, _) = make()
+        #expect(state.recipientLabel == "Alpha")
+        state.choose(.room("crew"))
+        #expect(state.recipientLabel == "Crew (room)")
+    }
+
+    @Test func dismissingAFailureKeepsTheText() async {
+        let (state, _, _) = make()
+        let cli = FakeCLI()
+        cli.failure = "boom"
+        state.text = "keep me"
+        _ = await state.send(using: cli)
+        state.dismissFailure()
+        #expect(state.status == .idle)
+        #expect(state.text == "keep me")
+        // Dismissing is only for failures: it must not clear a queued confirmation.
+        cli.failure = nil
+        _ = await state.send(using: cli)
+        state.dismissFailure()
+        #expect(state.status == .queued)
+    }
+
     @Test func resetStartsAFreshMessage() async {
         let (state, _, _) = make()
         state.text = "x"
```

- [ ] **Step 2: Run to see it fail**

Run (from `ui/menubar`): `./scripts/test.sh`
Expected: build FAIL (`recipientLabel`, `dismissFailure` do not exist).

- [ ] **Step 3: Implement**

```diff
--- a/ui/menubar/Sources/HerdbotKit/QuickEntry.swift
+++ b/ui/menubar/Sources/HerdbotKit/QuickEntry.swift
@@ -68,6 +68,11 @@
             ?? options.first?.recipient
     }
 
+    /// What the person sees for the current recipient, e.g. "Alpha" or "Crew (room)".
+    public var recipientLabel: String? {
+        options.first { $0.recipient == recipient }?.label
+    }
+
     public var daemonDown: Bool { menu.snapshot?.daemon == "down" }
 
     public var canSend: Bool {
@@ -86,6 +91,11 @@
         chosen = nil
     }
 
+    /// Clear a failure so the person can edit and try again; the text was kept.
+    public func dismissFailure() {
+        if case .failed = status { status = .idle }
+    }
+
     /// True when the message was queued. Dictated text is never sent on its own: this runs only
     /// when the person submits.
     @discardableResult
```

- [ ] **Step 4: Run to see it pass**

Run: `./scripts/test.sh`
Expected: PASS, 27 tests.

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/QuickEntry.swift ui/menubar/Tests/HerdbotKitTests/QuickEntryTests.swift
git commit -m "feat(menubar): label the recipient and dismiss a failed send"
```

---

### Task 2: the Spotlight-style panel

No unit tests: this is AppKit/SwiftUI presentation. The gate is a warning-free build; looks and accessibility behaviour are checked by a person (Task 3's checklist).

**Files:**
- Modify: `ui/menubar/Sources/HerdbotMenu/QuickEntryView.swift`, `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`, `ui/menubar/Sources/HerdbotMenu/App.swift`

**Interfaces:**
- Consumes: `QuickEntryState.recipientLabel`, `dismissFailure()` from Task 1.
- Produces: `QuickEntryController(state:cli:hotkeyLabel:)` (new `hotkeyLabel`, shown in the gear menu).

Design notes the code encodes (do not change without asking):
- The window is borderless, transparent, with a system shadow. Its content is `NSGlassEffectView` on macOS 26+ and an `NSVisualEffectView` (`.hudWindow`) before, so Reduce Transparency is honoured by the material. SwiftUI draws the hairline (0.5 pt, `Color.primary` at 0.18; 1 pt at 0.6 with Increase Contrast) and the rounded clip.
- The "To" control is a `Menu` containing an inline `Picker`, not a bare `Picker` (a `Picker` draws its own bordered pop-up button, which breaks the borderless look).
- Reduce Motion removes the fade on show/hide and the overlay transition.
- The panel keeps the focus approach from the previous fix: `onAppear`, the `didBecomeKey` notification, and an 80 ms `.task`. Do not remove any of them.
- Failure overlay: dismissed by a click or by typing (`onChange` of the text); there is no auto-dismiss. Success overlay: 0.6 s, then the panel closes.
- The panel top edge stays fixed as the field grows (`resize(toHeight:)`), placed in the upper third of the screen with the pointer.

- [ ] **Step 1: Replace `ui/menubar/Sources/HerdbotMenu/QuickEntryView.swift`**

```swift
import AppKit
import HerdbotKit
import SwiftUI

/// Spotlight-style: a large message field, a quiet row beneath it (settings on the left, the
/// recipient on the right), and no buttons. Return sends; Esc closes. Nothing is sent until you
/// press Return, so dictated text can be read first. Colours are system semantic colours and the
/// material comes from the window (see QuickEntryController), so Reduce Transparency, Increase
/// Contrast, Reduce Motion, light/dark and the accent colour all follow the system.
struct QuickEntryView: View {
    static let width: CGFloat = 680
    static let cornerRadius: CGFloat = 22

    let state: QuickEntryState
    let cli: ProcessCLI?
    let hotkeyLabel: String
    let close: () -> Void
    /// Reports the content height so the panel can follow it as the field grows.
    let heightChanged: (CGFloat) -> Void
    @FocusState private var focused: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var increaseContrast: Bool { NSWorkspace.shared.accessibilityDisplayShouldIncreaseContrast }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextField("Message, Return to send", text: Binding(get: { state.text }, set: { state.text = $0 }), axis: .vertical)
                .textFieldStyle(.plain)
                .font(.system(size: 22))
                .lineLimit(1...6)
                .focused($focused)
                .disabled(state.status == .sending)
                .onSubmit(submit)
                .onChange(of: state.text) { state.dismissFailure() }
            HStack(spacing: 10) {
                gear
                note
                Spacer(minLength: 0)
                recipientMenu
            }
        }
        .padding(8)
        .frame(width: Self.width)
        .overlay { result }
        .clipShape(RoundedRectangle(cornerRadius: Self.cornerRadius, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: Self.cornerRadius, style: .continuous)
                .strokeBorder(Color.primary.opacity(increaseContrast ? 0.6 : 0.18), lineWidth: increaseContrast ? 1 : 0.5)
        }
        .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { heightChanged($0) }
        .onAppear { focused = true }
        // onAppear runs before the panel is key, and SwiftUI drops a focus request made then. The first
        // show becomes key after activation (the notification); later shows become key at once, before
        // this view subscribes, so ask again once the panel has settled.
        .onReceive(NotificationCenter.default.publisher(for: NSWindow.didBecomeKeyNotification)) { _ in focused = true }
        .task {
            try? await Task.sleep(for: .milliseconds(80))
            focused = true
        }
    }

    private var gear: some View {
        Menu {
            Text("Hotkey \(hotkeyLabel)")
            Divider()
            Button("Quit herdbot menu") { NSApplication.shared.terminate(nil) }
        } label: {
            Image(systemName: "gearshape")
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .foregroundStyle(.secondary)
    }

    /// Why a send can't work, ahead of time.
    @ViewBuilder private var note: some View {
        if cli == nil {
            Text("herdbot not found").foregroundStyle(.red)
        } else if state.daemonDown {
            Text("daemon not running; messages wait until it is").foregroundStyle(.orange)
        }
    }

    /// A Menu rather than a bare Picker: a Picker draws its own bordered pop-up button.
    private var recipientMenu: some View {
        HStack(spacing: 4) {
            Text("To")
            Menu {
                Picker("To", selection: Binding(get: { state.recipient }, set: { if let r = $0 { state.choose(r) } })) {
                    ForEach(state.options) { option in
                        Text(option.label).tag(Optional(option.recipient))
                    }
                }
                .pickerStyle(.inline)
                .labelsHidden()
            } label: {
                HStack(spacing: 3) {
                    Text(state.recipientLabel ?? "no recipients")
                    Image(systemName: "chevron.up.chevron.down").font(.system(size: 9))
                }
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .font(.system(size: 13))
        .foregroundStyle(.secondary)
    }

    /// Covers the whole panel: queued, then the panel closes; failed, until you click or type.
    @ViewBuilder private var result: some View {
        switch state.status {
        case .queued:
            overlay(symbol: "checkmark.circle.fill", tint: .green, title: "Queued to \(state.recipientLabel ?? "herdbot")", detail: nil)
        case .failed(let message):
            overlay(symbol: "exclamationmark.triangle.fill", tint: .orange, title: message, detail: "Click or start typing to edit")
                .onTapGesture { state.dismissFailure() }
        case .idle, .sending:
            EmptyView()
        }
    }

    private func overlay(symbol: String, tint: Color, title: String, detail: String?) -> some View {
        VStack(spacing: 6) {
            Image(systemName: symbol).font(.system(size: 30)).foregroundStyle(tint)
            Text(title).font(.system(size: 15, weight: .medium)).multilineTextAlignment(.center).lineLimit(3)
            if let detail { Text(detail).font(.system(size: 12)).foregroundStyle(.secondary) }
        }
        .padding(16)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.regularMaterial)
        .transition(reduceMotion ? .identity : .opacity)
    }

    private func submit() {
        guard let cli else { return }
        Task {
            if await state.send(using: cli) {
                try? await Task.sleep(for: .milliseconds(600))
                close()
            }
        }
    }
}
```

- [ ] **Step 2: Replace `ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift`**

```swift
import AppKit
import HerdbotKit
import SwiftUI

/// Borderless panels refuse key status by default; this one must take it so dictation lands in its
/// field. Escape closes it.
private final class QuickEntryPanel: NSPanel {
    var onCancel: () -> Void = {}
    override var canBecomeKey: Bool { true }
    override func cancelOperation(_ sender: Any?) { onCancel() }
}

@MainActor
final class QuickEntryController {
    private let state: QuickEntryState
    private let cli: ProcessCLI?
    private let hotkeyLabel: String
    private let panel: QuickEntryPanel
    /// Set while a fade-out runs, so a quick re-show isn't ordered out by the old fade.
    private var hiding = false

    private var reduceMotion: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    init(state: QuickEntryState, cli: ProcessCLI?, hotkeyLabel: String) {
        self.state = state
        self.cli = cli
        self.hotkeyLabel = hotkeyLabel
        panel = QuickEntryPanel(
            contentRect: NSRect(x: 0, y: 0, width: QuickEntryView.width, height: 80),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false)
        panel.isOpaque = false
        panel.backgroundColor = .clear
        panel.hasShadow = true
        panel.isFloatingPanel = true
        panel.level = .floating
        panel.isReleasedWhenClosed = false
        // Stay open if a dictation HUD takes focus for a moment: only Escape or a send closes it.
        panel.hidesOnDeactivate = false
        panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        panel.onCancel = { [weak self] in self?.hide() }
    }

    func toggle() {
        panel.isVisible && !hiding ? hide() : show()
    }

    func show() {
        hiding = false
        state.reset()
        // A fresh view each time, so the text field takes focus on every open.
        let host = NSHostingView(rootView: QuickEntryView(
            state: state, cli: cli, hotkeyLabel: hotkeyLabel,
            close: { [weak self] in self?.hide() },
            heightChanged: { [weak self] in self?.resize(toHeight: $0) }))
        panel.contentView = material(around: host)
        let height = host.fittingSize.height
        panel.setFrame(NSRect(origin: origin(forHeight: height), size: NSSize(width: QuickEntryView.width, height: height)), display: true)
        // The app must be active for the panel to become key: dictation tools type into the active app.
        NSApp.activate(ignoringOtherApps: true)
        panel.alphaValue = reduceMotion ? 1 : 0
        panel.makeKeyAndOrderFront(nil)
        if !reduceMotion {
            NSAnimationContext.runAnimationGroup { $0.duration = 0.12; panel.animator().alphaValue = 1 }
        }
    }

    func hide() {
        guard panel.isVisible, !hiding else { return }
        if reduceMotion {
            panel.orderOut(nil)
            return
        }
        hiding = true
        Task {
            await NSAnimationContext.runAnimationGroup { context in
                context.duration = 0.1
                panel.animator().alphaValue = 0
            }
            // A re-show during the fade clears `hiding`; leave the panel up then.
            guard hiding else { return }
            hiding = false
            panel.orderOut(nil)
        }
    }

    /// The system's own material, which follows Reduce Transparency and Increase Contrast by itself:
    /// Liquid Glass where it exists (macOS 26+), the HUD blur before that.
    private func material(around host: NSView) -> NSView {
        if #available(macOS 26, *) {
            let glass = NSGlassEffectView()
            glass.cornerRadius = QuickEntryView.cornerRadius
            glass.contentView = host
            return glass
        }
        let blur = NSVisualEffectView()
        blur.material = .hudWindow
        blur.blendingMode = .behindWindow
        blur.state = .active
        blur.wantsLayer = true
        blur.layer?.cornerRadius = QuickEntryView.cornerRadius
        blur.layer?.cornerCurve = .continuous
        blur.layer?.masksToBounds = true
        host.translatesAutoresizingMaskIntoConstraints = false
        blur.addSubview(host)
        NSLayoutConstraint.activate([
            host.leadingAnchor.constraint(equalTo: blur.leadingAnchor),
            host.trailingAnchor.constraint(equalTo: blur.trailingAnchor),
            host.topAnchor.constraint(equalTo: blur.topAnchor),
            host.bottomAnchor.constraint(equalTo: blur.bottomAnchor),
        ])
        return blur
    }

    /// Upper third of the screen the pointer is on, centred, like Spotlight.
    private func origin(forHeight height: CGFloat) -> NSPoint {
        let screen = NSScreen.screens.first { NSMouseInRect(NSEvent.mouseLocation, $0.frame, false) } ?? NSScreen.main
        let area = screen?.visibleFrame ?? NSRect(x: 0, y: 0, width: 1440, height: 900)
        return NSPoint(x: area.midX - QuickEntryView.width / 2, y: area.maxY - area.height * 0.22 - height)
    }

    /// Keep the top edge still as the field grows or shrinks.
    private func resize(toHeight height: CGFloat) {
        guard panel.isVisible, abs(panel.frame.height - height) > 0.5 else { return }
        var frame = panel.frame
        frame.origin.y += frame.height - height
        frame.size.height = height
        panel.setFrame(frame, display: true)
    }
}
```

- [ ] **Step 3: Pass the hotkey label in `ui/menubar/Sources/HerdbotMenu/App.swift`**

```diff
--- a/ui/menubar/Sources/HerdbotMenu/App.swift
+++ b/ui/menubar/Sources/HerdbotMenu/App.swift
@@ -22,7 +22,7 @@
         } else {
             state.fail(CLIError.notFound.localizedDescription)
         }
-        let quickEntry = QuickEntryController(state: QuickEntryState(menu: state), cli: cli)
+        let quickEntry = QuickEntryController(state: QuickEntryState(menu: state), cli: cli, hotkeyLabel: "⌥Space")
         // ⌥Space. Not a modifier alone, so it can't clash with MacWhisper's Right ⌘ dictation key.
         hotkeyRegistered = GlobalHotkey().register(keyCode: kVK_Space, modifiers: optionKey) { quickEntry.toggle() }
         self.state = state
```

- [ ] **Step 4: Verify**

Run (from `ui/menubar`): `swift build 2>&1 | grep -E "error|warning: [^s]|Build"`, `./scripts/test.sh`, `./scripts/build-app.sh`
Expected: `Build complete!` with no Swift errors or warnings, 27 tests pass, `built build/HerdbotMenu.app`. Do not launch it.

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotMenu/QuickEntryView.swift ui/menubar/Sources/HerdbotMenu/QuickEntryController.swift ui/menubar/Sources/HerdbotMenu/App.swift
git commit -m "feat(menubar): restyle quick entry like Spotlight, on the system material"
```

---

### Task 3: docs and the manual check

**Files:**
- Modify: `ui/menubar/README.md`, `docs/superpowers/specs/2026-10-21-ui-layer-design.md`

- [ ] **Step 1: `ui/menubar/README.md`**

Replace everything from the `## Quick entry` heading to the end of the file with:

````markdown
## Quick entry

⌥Space (or the menu's "Quick entry") opens a Spotlight-style panel: a large message field, and beneath
it a gear menu on the left and a "To" menu (bots, then rooms) on the right. Return sends it with
`herdbot send <bot> <text>` or `herdbot room send <room> <text>`; a "Queued to ..." overlay covers the
panel and it closes. A failure shows its error over the panel; click it or start typing to edit
(your text is kept). Esc closes the panel. The last recipient is remembered. If the daemon is down a
note says messages wait until it is up. Nothing is ever sent without a keypress, so you can read a
dictated message before it goes. The gear menu shows the hotkey and has Quit.

The panel uses the system material (Liquid Glass on macOS 26 and later, the HUD blur before that), so
Reduce Transparency, Increase Contrast, Reduce Motion, light/dark and your accent colour apply to it.

Dictation (MacWhisper, hold Right ⌘): the hotkey is not a bare modifier, so it can't collide with the
dictation key; the panel activates the app and takes key focus, so dictated text lands in its field; and the
panel stays open if a dictation HUD takes focus for a moment.

### Manual check (needs a person; MacWhisper for steps 7-8)

1. `./scripts/build-app.sh`, and quit any running HerdbotMenu first. Use a herdbot with rooms in
   `status --json` (`cargo install --path crates/herdbot`, restart the daemon).
2. Throwaway state with two bots and a room:
   `export HB=$(mktemp -d); for b in scratch other; do HERDBOT_HOME=$HB herdbot new $b; done; HERDBOT_HOME=$HB herdbot room new crew --members scratch,other`
3. `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`. Press ⌥Space. Expect a
   translucent rounded panel near the top of the screen with a hairline border, the cursor in a large
   field, a gear at the lower left, a "daemon not running" note, and `To ⌃ scratch` (borderless) at the
   lower right. If the menu says "⌥Space is taken by another app", free the key first.
4. Type `hello "quoted" world`, press Return. Expect a "Queued to scratch" overlay over the whole panel,
   then the panel closes. `cat $HB/.queue/scratch.jsonl` shows the text.
5. Reopen several times (send, then Esc, then send again): the cursor is in the field every time. Open the
   "To" menu, pick `Crew (room)`, send `to the room`; `tail -1 $HB/rooms/crew/transcript.jsonl` shows it
   from `you`. Reopen: the picker still shows `Crew (room)`.
6. Make a send fail: `chmod 000 $HB/.queue`, send, and expect an error overlay that stays; click it (or type) and the
   text is still there. `chmod 755 $HB/.queue` afterwards. Open the gear: it lists the hotkey and Quit.
   Type enough text (or paste several lines) to see the field grow up to 6 lines while the panel's top
   edge stays put.
7. MacWhisper: open the panel, hold Right ⌘ and speak. The text appears in the field and nothing sends;
   press Return to send. Repeat with another app in front before pressing ⌥Space.
8. System Settings → Accessibility → Display: turn on Reduce Transparency, then Increase Contrast, then
   Reduce Motion, one at a time, reopening the panel each time. Expect an opaque panel, a stronger border,
   and no fade, respectively. Switch to light mode: the panel stays readable. Switch them back.
9. Quit from the gear menu; `rm -r $HB`.
````

- [ ] **Step 2: spec**

In `docs/superpowers/specs/2026-10-21-ui-layer-design.md`, replace the first two bullets of the Phase 2 block (the ⌥Space bullet and the Return bullet) with:

````markdown
- ⌥Space (Carbon `RegisterEventHotKey`, no Accessibility permission) opens a Spotlight-style floating panel (redesigned 2026-10-22): a large full-width message field, then a row with a gear menu (hotkey, Quit) on the left and a borderless "To" bot/room menu on the right, preselected to the last recipient. There are no buttons, 8 pt padding and a hairline border.
- Return submits through `herdbot send` / `herdbot room send`. A success or error overlay covers the whole panel: success closes it after a moment; an error stays until a click or a keystroke, and the text is kept. Esc closes. Nothing is sent without a keypress, so dictated text can be reviewed first.
- The panel uses the system material (`NSGlassEffectView` on macOS 26+, `NSVisualEffectView` before) and semantic colours, so Reduce Transparency, Increase Contrast, Reduce Motion, light/dark and the accent colour follow the system.
````

- [ ] **Step 3: Verify and commit**

Run: `git diff --stat` (only these two files), `cd ui/menubar && ./scripts/test.sh`.

```bash
git add ui/menubar/README.md docs/superpowers/specs/2026-10-21-ui-layer-design.md
git commit -m "docs: document the quick-entry redesign and its manual check"
```

## Self-review

- Coverage of Shane's notes: large full-width field, "To" dropdown below and right-aligned in the same style, no buttons, whole-panel result overlay, 8 pt padding, line-art gear (`gearshape`), hairline border (Task 2); OS material and accessibility (Task 2 notes, Task 3 step 8).
- Changed from the approved design: the gear menu has the hotkey and Quit but no "Open menu bar app" (the panel is part of that app); and there is no ⌘Return shortcut, since Return already submits.
- Newline in the multi-line field is the system's own (⌥Return or ⇧Return, whichever macOS gives a vertical-axis field); it was not tested. Task 3 step 6 only checks that growth works.
- Not verifiable without a person: Reduce Transparency, Increase Contrast and Reduce Motion actually changing the panel; the failure overlay path; the focus fix surviving the borderless window.
