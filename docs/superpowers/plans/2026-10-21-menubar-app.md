# Menu Bar App (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A SwiftUI menu bar app that shows herdbot's state (bots, running subagents, cooling harnesses, blocked agents), posts a notification when an agent becomes blocked, and focuses that agent's herdr pane when you click it.

**Architecture:** A SwiftPM package at `ui/menubar/` with a UI-free library `HerdbotKit` (JSON models, a parser, a `HerdbotCLI` protocol with a `Process`-based implementation, and an `@Observable` `MenuState`) and a thin executable `HerdbotMenu` (menu bar scene, notifier). The app only talks to the `herdbot` CLI from phase 0: it follows `herdbot watch --json` and runs `herdbot focus <agent>`. A script wraps the executable in an ad-hoc-signed `.app` bundle, because notifications need a bundle id.

**Tech Stack:** Swift 6.4 (Command Line Tools is enough; full Xcode is not installed), SwiftUI `MenuBarExtra`, Observation, UserNotifications, Swift Testing, macOS 14+.

Spec: `docs/superpowers/specs/2026-10-21-ui-layer-design.md` (phase 1). Phase 0 (the CLI contract) is done and on `feature/ui-layer`.

## Global Constraints

- The app never reads herdbot's files and never links Rust. Its only inputs are `herdbot watch --json` lines and its only action is `herdbot focus <agent>`.
- Every line carries `"v"`; the app supports `v == 1` (`supportedVersion`) and shows "update herdbot" for anything else.
- `swift build`, `./scripts/test.sh` and `./scripts/build-app.sh` pass at the end of every task that touches the package (run from `ui/menubar/`).
- Do not use `@State`, `@StateObject` or other SwiftUI macros that need a plugin missing from Command Line Tools: `@State` is a macro in this SDK and fails with "plugin for module 'SwiftUIMacros' not found". `@Observable` and `@MainActor` are fine.
- `swift test` needs the Testing macro plugin path on Command Line Tools installs; always run tests through `./scripts/test.sh`.
- Commits: conventional prefix, explicit paths (never `git add -A` / `.`), no AI attribution, no push.
- Do NOT launch the app in this plan (it triggers macOS's one-time notification permission prompt). Launching is the manual checklist in Task 5 for Shane.

## Deviations from the spec (decided while verifying the code, 2026-10-21)

- **Watch only.** The spec has the agent poll `status --json` every ~2 s and also tail `watch`. `watch --json` already prints a snapshot first and again on every change, so polling is redundant; the app follows `watch` only.
- **Login item deferred.** Registering a login item needs a properly signed bundle (`SMAppService`); that is untestable here. Add the app manually in System Settings → General → Login Items. Task 5 documents it.
- **Quick-entry (phase 2)** is a separate plan.

## File Structure

All new, under `ui/menubar/`:

- `Package.swift`, `.gitignore`, `Info.plist`
- `scripts/test.sh`, `scripts/build-app.sh`
- `Sources/HerdbotKit/Models.swift` — Codable mirrors of the phase 0 JSON
- `Sources/HerdbotKit/Stream.swift` — `CLIError`, `HerdbotCLI`, `StreamParser`
- `Sources/HerdbotKit/ProcessCLI.swift` — runs the real `herdbot`
- `Sources/HerdbotKit/MenuState.swift` — `IconState`, `NotificationTracker`, `MenuState`
- `Sources/HerdbotMenu/{App,MenuIcon,MenuContent,Notifier}.swift` — the app
- `Tests/HerdbotKitTests/{Fixtures,DecodingTests,ProcessCLITests,StateTests}.swift`
- Modified: `README.md`, the spec, `docs/superpowers/plans/README.md`; new `ui/menubar/README.md`

Work in a worktree: `git -C ~/Code/herdbot worktree add .worktrees/ui-menubar -b feature/ui-menubar feature/ui-layer`. Every command below runs from `<worktree>/ui/menubar` unless stated.

---

### Task 1: Package, models and the stream parser

**Files:**
- Create: `ui/menubar/Package.swift`, `ui/menubar/.gitignore`, `ui/menubar/scripts/test.sh`
- Create: `Sources/HerdbotKit/Models.swift`, `Sources/HerdbotKit/Stream.swift`
- Create: `Tests/HerdbotKitTests/Fixtures.swift`, `Tests/HerdbotKitTests/DecodingTests.swift`

**Interfaces:**
- Produces: `supportedVersion`, `Event`, `BotView`, `RunView`, `CooldownView`, `Snapshot`, `StreamLine`, `CLIError` (`.notFound`, `.unsupported(Int)`, `.failed(String)`), `protocol HerdbotCLI` (`watch()`, `focus(agent:)`), `StreamParser.parse(_:) throws -> StreamLine?`; test helpers `snapshotLine`, `eventLine`, `snapshot(daemon:runs:blocked:)`, `blockedEvent(_:at:)`, `line(_:)`, `fakeHerdbot(_:)`.

- [ ] **Step 1: Scaffold the package (library and test targets only)**

`ui/menubar/Package.swift`:

```swift
// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "HerdbotMenu",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "HerdbotKit"),
        .testTarget(name: "HerdbotKitTests", dependencies: ["HerdbotKit"]),
    ]
)
```

`ui/menubar/.gitignore`:

```
.build/
build/
```

`scripts/test.sh`:

```bash
#!/bin/bash
# swift test, plus the Testing macro plugin path that Command Line Tools installs don't pass on their own.
set -euo pipefail
cd "$(dirname "$0")/.."
plugins="$(xcode-select -p)/usr/lib/swift/host/plugins/testing"
if [ -d "$plugins" ]; then
  exec swift test -Xswiftc -plugin-path -Xswiftc "$plugins" "$@"
fi
exec swift test "$@"
```

Run `chmod +x scripts/test.sh`.

- [ ] **Step 2: Write the failing tests**

`Tests/HerdbotKitTests/Fixtures.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

let snapshotLine = """
{"v":1,"type":"snapshot","snapshot":{"v":1,"at":100,"daemon":"up",\
"bots":[{"slug":"alpha","title":"Alpha","up":true}],\
"runs":[{"id":"r1","bot":"alpha","agent":"alpha-r1","route":"claude:sonnet","description":"do it"}],\
"cooldowns":[{"harness":"claude","until":4102444800,"reason":"limit"}],\
"blocked":[{"at":10,"agent":"alpha-main","kind":"blocked","reason":"approval: Bash","harness":"claude"}],\
"warnings":["x"]}}
"""

let eventLine = """
{"v":1,"type":"event","event":{"at":20,"agent":"alpha-main","kind":"turn_end","text":"done","harness":"claude"}}
"""

func snapshot(daemon: String = "up", runs: Int = 0, blocked: [Event] = []) -> Snapshot {
    Snapshot(
        v: 1, at: 1, daemon: daemon, bots: [],
        runs: (0..<runs).map { RunView(id: "r\($0)", bot: "a", agent: "a-r\($0)", route: "x", description: "d") },
        cooldowns: [], blocked: blocked, warnings: [])
}

func blockedEvent(_ agent: String, at: Int = 1) -> Event {
    Event(at: at, agent: agent, kind: "blocked", text: nil, reason: "approval", harness: "claude")
}

func line(_ snapshot: Snapshot) -> StreamLine {
    StreamLine(v: 1, type: "snapshot", snapshot: snapshot, event: nil)
}

/// A fake `herdbot` in a temp dir: the script body decides what it prints.
func fakeHerdbot(_ body: String) throws -> URL {
    let dir = FileManager.default.temporaryDirectory.appendingPathComponent("hk-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let script = dir.appendingPathComponent("herdbot")
    try "#!/bin/sh\n\(body)\n".write(to: script, atomically: true, encoding: .utf8)
    try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: script.path)
    return script
}
```

`Tests/HerdbotKitTests/DecodingTests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@Suite struct Decoding {
    @Test func snapshotLineDecodes() throws {
        let parsed = try #require(try StreamParser.parse(snapshotLine))
        #expect(parsed.type == "snapshot")
        let snap = try #require(parsed.snapshot)
        #expect(snap.daemon == "up")
        #expect(snap.bots.map(\.slug) == ["alpha"])
        #expect(snap.runs.first?.agent == "alpha-r1")
        #expect(snap.cooldowns.first?.until == 4102444800)
        #expect(snap.blocked.first?.reason == "approval: Bash")
        #expect(snap.blocked.first?.text == nil)
    }

    @Test func eventLineDecodes() throws {
        let parsed = try #require(try StreamParser.parse(eventLine))
        #expect(parsed.event?.kind == "turn_end")
        #expect(parsed.event?.text == "done")
    }

    @Test func unknownVersionThrows() {
        #expect(throws: CLIError.unsupported(2)) { try StreamParser.parse(#"{"v":2,"type":"snapshot"}"#) }
    }

    @Test func garbageIsSkipped() throws {
        #expect(try StreamParser.parse("not json") == nil)
        #expect(try StreamParser.parse(#"{"hello":1}"#) == nil)
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `./scripts/test.sh 2>&1 | grep -E "error" | head -5`
Expected: compile errors (`Snapshot`, `StreamParser`, `Event`… not found).

- [ ] **Step 4: Implement**

`Sources/HerdbotKit/Models.swift`:

```swift
import Foundation

/// The JSON `herdbot status --json` and `herdbot watch --json` print. Mirrors
/// crates/herdbot-core/src/snapshot.rs; bump `supportedVersion` with `snapshot::VERSION`.
public let supportedVersion = 1

public struct Event: Codable, Hashable, Sendable {
    public let at: Int
    public let agent: String
    public let kind: String
    public let text: String?
    public let reason: String?
    public let harness: String
}

public struct BotView: Codable, Hashable, Sendable {
    public let slug: String
    public let title: String
    public let up: Bool
}

public struct RunView: Codable, Hashable, Sendable {
    public let id: String
    public let bot: String
    public let agent: String
    public let route: String
    public let description: String
}

public struct CooldownView: Codable, Hashable, Sendable {
    public let harness: String
    /// Unix seconds.
    public let until: Int
    public let reason: String
}

public struct Snapshot: Codable, Hashable, Sendable {
    public let v: Int
    public let at: Int
    /// "up" or "down".
    public let daemon: String
    public let bots: [BotView]
    public let runs: [RunView]
    public let cooldowns: [CooldownView]
    public let blocked: [Event]
    public let warnings: [String]
}

/// One line of `herdbot watch --json`: `type` is "snapshot" or "event".
public struct StreamLine: Codable, Hashable, Sendable {
    public let v: Int
    public let type: String
    public let snapshot: Snapshot?
    public let event: Event?
}
```

`Sources/HerdbotKit/Stream.swift`:

```swift
import Foundation

public enum CLIError: Error, Equatable, LocalizedError {
    case notFound
    case unsupported(Int)
    case failed(String)

    public var errorDescription: String? {
        switch self {
        case .notFound: "herdbot not found (set HERDBOT_BIN or install it to ~/.cargo/bin)"
        case .unsupported(let v): "herdbot speaks protocol v\(v); update herdbot or this app"
        case .failed(let message): message
        }
    }
}

public protocol HerdbotCLI: Sendable {
    /// `herdbot watch --json`: a snapshot line first, then changes. Ends with an error when the
    /// process exits.
    func watch() -> AsyncThrowingStream<StreamLine, Error>
    /// `herdbot focus <agent>`.
    func focus(agent: String) async throws
}

public enum StreamParser {
    /// nil for a line that isn't ours (malformed, or not a stream line). Throws `unsupported`
    /// for a protocol version this build doesn't know.
    public static func parse(_ line: String) throws -> StreamLine? {
        struct Head: Decodable { let v: Int }
        let data = Data(line.utf8)
        guard let head = try? JSONDecoder().decode(Head.self, from: data) else { return nil }
        guard head.v == supportedVersion else { throw CLIError.unsupported(head.v) }
        return try? JSONDecoder().decode(StreamLine.self, from: data)
    }
}
```

- [ ] **Step 5: Run to verify they pass**

Run: `./scripts/test.sh 2>&1 | grep -E "error|Test run"`
Expected: `Test run with 4 tests … passed`. (If `swift test` cannot find the `Testing` macros, you ran `swift test` directly; use the script.)

- [ ] **Step 6: Commit**

```bash
git add ui/menubar/Package.swift ui/menubar/.gitignore ui/menubar/scripts/test.sh ui/menubar/Sources ui/menubar/Tests
git commit -m "feat: add HerdbotKit models and stream parser for the menu bar app"
```

---

### Task 2: `ProcessCLI`

**Files:**
- Create: `Sources/HerdbotKit/ProcessCLI.swift`, `Tests/HerdbotKitTests/ProcessCLITests.swift`

**Interfaces:**
- Consumes: `HerdbotCLI`, `StreamParser`, `CLIError`, `fakeHerdbot(_:)`, `snapshotLine`, `eventLine`.
- Produces: `ProcessCLI(binary:environment:)`, `ProcessCLI.locate(environment:home:) -> URL?`; conforms to `HerdbotCLI`.

- [ ] **Step 1: Write the failing tests**

`Tests/HerdbotKitTests/ProcessCLITests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@Suite struct ProcessCLITests {
    @Test func watchStreamsLinesThenEnds() async throws {
        let script = try fakeHerdbot("echo '\(snapshotLine)'\necho 'junk'\necho '\(eventLine)'")
        var types: [String] = []
        do {
            for try await parsed in ProcessCLI(binary: script).watch() { types.append(parsed.type) }
            Issue.record("stream should end with an error")
        } catch {
            #expect(error as? CLIError == .failed("herdbot watch ended"))
        }
        #expect(types == ["snapshot", "event"])
    }

    @Test func watchReportsAnUnknownVersion() async throws {
        let script = try fakeHerdbot(#"echo '{"v":9,"type":"snapshot"}'"#)
        do {
            for try await _ in ProcessCLI(binary: script).watch() {}
            Issue.record("expected an error")
        } catch {
            #expect(error as? CLIError == .unsupported(9))
        }
    }

    @Test func focusPassesTheAgentAndReportsFailure() async throws {
        let ok = try fakeHerdbot(#"[ "$1 $2" = "focus a-1" ]"#)
        try await ProcessCLI(binary: ok).focus(agent: "a-1")
        let bad = try fakeHerdbot("echo 'herdr agent_not_found' >&2; exit 1")
        await #expect(throws: CLIError.failed("herdr agent_not_found")) {
            try await ProcessCLI(binary: bad).focus(agent: "nope")
        }
    }

    @Test func locatePrefersHerdbotBin() throws {
        let script = try fakeHerdbot("true")
        #expect(ProcessCLI.locate(environment: ["HERDBOT_BIN": script.path], home: "/nonexistent") == script)
        #expect(ProcessCLI.locate(environment: [:], home: "/nonexistent") == nil)
    }

    /// Only when HERDBOT_BIN points at a real, new-enough herdbot: the live contract check.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["HERDBOT_BIN"] != nil))
    func realBinaryStreamsASnapshotFirst() async throws {
        let binary = try #require(ProcessCLI.locate())
        var first: StreamLine?
        for try await parsed in ProcessCLI(binary: binary).watch() {
            first = parsed
            break
        }
        #expect(first?.type == "snapshot")
        #expect(first?.snapshot?.v == supportedVersion)
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `./scripts/test.sh 2>&1 | grep -E "error" | head -3`
Expected: `cannot find 'ProcessCLI' in scope`.

- [ ] **Step 3: Implement**

`Sources/HerdbotKit/ProcessCLI.swift`:

```swift
import Foundation

public struct ProcessCLI: HerdbotCLI {
    public let binary: URL
    public let environment: [String: String]

    public init(binary: URL, environment: [String: String] = ProcessInfo.processInfo.environment) {
        self.binary = binary
        self.environment = environment
    }

    /// GUI apps get a bare PATH, so look in the usual install spots. `HERDBOT_BIN` wins.
    public static func locate(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        home: String = NSHomeDirectory()
    ) -> URL? {
        let candidates = [environment["HERDBOT_BIN"], home + "/.cargo/bin/herdbot", "/opt/homebrew/bin/herdbot", "/usr/local/bin/herdbot"]
        return candidates
            .compactMap { $0 }
            .map { URL(fileURLWithPath: $0) }
            .first { FileManager.default.isExecutableFile(atPath: $0.path) }
    }

    public func watch() -> AsyncThrowingStream<StreamLine, Error> {
        AsyncThrowingStream { continuation in
            let process = Process()
            let pipe = Pipe()
            process.executableURL = binary
            process.arguments = ["watch", "--json"]
            process.environment = environment
            process.standardOutput = pipe
            process.standardError = FileHandle.nullDevice
            do {
                try process.run()
            } catch {
                continuation.finish(throwing: CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
                return
            }
            let reader = Task {
                do {
                    for try await line in pipe.fileHandleForReading.bytes.lines {
                        if let parsed = try StreamParser.parse(line) { continuation.yield(parsed) }
                    }
                    continuation.finish(throwing: CLIError.failed("herdbot watch ended"))
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in
                reader.cancel()
                if process.isRunning { process.terminate() }
            }
        }
    }

    public func focus(agent: String) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let process = Process()
            let errors = Pipe()
            process.executableURL = binary
            process.arguments = ["focus", agent]
            process.environment = environment
            process.standardOutput = FileHandle.nullDevice
            process.standardError = errors
            process.terminationHandler = { finished in
                if finished.terminationStatus == 0 {
                    continuation.resume()
                } else {
                    let text = String(decoding: errors.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                    continuation.resume(throwing: CLIError.failed(text.trimmingCharacters(in: .whitespacesAndNewlines)))
                }
            }
            do {
                try process.run()
            } catch {
                continuation.resume(throwing: CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify they pass, then run the live contract test**

Run: `./scripts/test.sh 2>&1 | grep -E "error|skipped|Test run"`
Expected: `Test run with 9 tests … passed` with `realBinaryStreamsASnapshotFirst() skipped`.

Then, with a built herdbot from this branch (`cargo build` at the repo root first):

Run: `HERDBOT_BIN=$(git rev-parse --show-toplevel)/target/debug/herdbot ./scripts/test.sh 2>&1 | grep -E "error|realBinary|Test run"`
Expected: `realBinaryStreamsASnapshotFirst() passed` — the Swift decoder reads the real Rust output. If it fails, the JSON contract drifted from `crates/herdbot-core/src/snapshot.rs`; fix `Models.swift`, not the Rust.

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/ProcessCLI.swift ui/menubar/Tests/HerdbotKitTests/ProcessCLITests.swift
git commit -m "feat: add ProcessCLI to drive herdbot watch and focus"
```

---

### Task 3: `MenuState` and notification tracking

**Files:**
- Create: `Sources/HerdbotKit/MenuState.swift`, `Tests/HerdbotKitTests/StateTests.swift`

**Interfaces:**
- Consumes: `StreamLine`, `Snapshot`, `Event`, `HerdbotCLI`; helpers `snapshot(...)`, `blockedEvent(_:at:)`, `line(_:)`, `eventLine`.
- Produces: `IconState` (`.idle`, `.running`, `.blocked(Int)`, `.daemonDown`, `.unavailable`), `NotificationTracker.fresh(_:)`, `MenuState` (`snapshot`, `problem`, `icon`, `onBlocked`, `apply(_:)`, `fail(_:)`, `follow(_:)`).

- [ ] **Step 1: Write the failing tests**

`Tests/HerdbotKitTests/StateTests.swift`:

```swift
import Foundation
import Testing
@testable import HerdbotKit

@Suite struct Tracking {
    @Test func firstCallOnlyRecords() {
        var tracker = NotificationTracker()
        #expect(tracker.fresh([blockedEvent("a")]).isEmpty)
        #expect(tracker.fresh([blockedEvent("a")]).isEmpty)
    }

    @Test func announcesNewBlocksOnce() {
        var tracker = NotificationTracker()
        _ = tracker.fresh([])
        #expect(tracker.fresh([blockedEvent("a")]).map(\.agent) == ["a"])
        #expect(tracker.fresh([blockedEvent("a")]).isEmpty)
        #expect(tracker.fresh([]).isEmpty)
        // The same agent blocking again later is a new event.
        #expect(tracker.fresh([blockedEvent("a", at: 9)]).map(\.agent) == ["a"])
    }
}

@MainActor
@Suite struct State {
    @Test func iconFollowsTheSnapshot() {
        let state = MenuState()
        #expect(state.icon == .unavailable)
        state.apply(line(snapshot()))
        #expect(state.icon == .idle)
        state.apply(line(snapshot(runs: 1)))
        #expect(state.icon == .running)
        state.apply(line(snapshot(daemon: "down")))
        #expect(state.icon == .daemonDown)
        state.apply(line(snapshot(daemon: "down", blocked: [blockedEvent("a"), blockedEvent("b")])))
        #expect(state.icon == .blocked(2))
        state.fail("boom")
        #expect(state.icon == .unavailable)
        #expect(state.problem == "boom")
    }

    @Test func onBlockedFiresForNewBlocksOnly() {
        let state = MenuState()
        var announced: [String] = []
        state.onBlocked = { announced.append($0.agent) }
        state.apply(line(snapshot(blocked: [blockedEvent("old")])))
        state.apply(line(snapshot(blocked: [blockedEvent("old"), blockedEvent("new")])))
        #expect(announced == ["new"])
    }

    @Test func eventLinesAreIgnored() throws {
        let state = MenuState()
        state.apply(try #require(try StreamParser.parse(eventLine)))
        #expect(state.snapshot == nil)
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `./scripts/test.sh 2>&1 | grep -E "error" | head -3`
Expected: `cannot find 'NotificationTracker' in scope`.

- [ ] **Step 3: Implement**

`Sources/HerdbotKit/MenuState.swift`:

```swift
import Foundation
import Observation

public enum IconState: Equatable, Sendable {
    case idle
    case running
    case blocked(Int)
    case daemonDown
    case unavailable
}

/// Blocked events already announced. The first call only records, so launching the app
/// doesn't re-announce agents that were already waiting.
public struct NotificationTracker: Sendable {
    private var seen: Set<String> = []
    private var primed = false

    public init() {}

    public mutating func fresh(_ blocked: [Event]) -> [Event] {
        let keys = blocked.map { "\($0.agent)#\($0.at)" }
        defer {
            seen = Set(keys)
            primed = true
        }
        guard primed else { return [] }
        return zip(blocked, keys).filter { !seen.contains($0.1) }.map { $0.0 }
    }
}

@MainActor @Observable
public final class MenuState {
    public private(set) var snapshot: Snapshot?
    /// Why there is no usable snapshot: herdbot missing, wrong version, or watch ended.
    public private(set) var problem: String?
    /// Called once per newly blocked agent.
    @ObservationIgnored public var onBlocked: (Event) -> Void = { _ in }
    @ObservationIgnored private var tracker = NotificationTracker()

    public init() {}

    public var icon: IconState {
        guard problem == nil, let snapshot else { return .unavailable }
        if !snapshot.blocked.isEmpty { return .blocked(snapshot.blocked.count) }
        if snapshot.daemon == "down" { return .daemonDown }
        return snapshot.runs.isEmpty ? .idle : .running
    }

    /// Event lines are ignored: a blocked event also changes the next snapshot.
    public func apply(_ line: StreamLine) {
        guard line.type == "snapshot", let snapshot = line.snapshot else { return }
        problem = nil
        self.snapshot = snapshot
        for event in tracker.fresh(snapshot.blocked) { onBlocked(event) }
    }

    public func fail(_ message: String) {
        problem = message
    }

    /// Follow `watch` until cancelled, reconnecting with a capped backoff.
    public func follow(_ cli: any HerdbotCLI) async {
        var delay = 1
        while !Task.isCancelled {
            do {
                for try await line in cli.watch() {
                    apply(line)
                    delay = 1
                }
            } catch {
                fail(error.localizedDescription)
            }
            try? await Task.sleep(for: .seconds(delay))
            delay = min(delay * 2, 10)
        }
    }
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `./scripts/test.sh 2>&1 | grep -E "error|Test run"`
Expected: `Test run with 14 tests … passed`.

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Sources/HerdbotKit/MenuState.swift ui/menubar/Tests/HerdbotKitTests/StateTests.swift
git commit -m "feat: add MenuState and blocked-agent notification tracking"
```

---

### Task 4: The app and the bundle script

**Files:**
- Modify: `ui/menubar/Package.swift` (add the executable target)
- Create: `Sources/HerdbotMenu/{App,MenuIcon,MenuContent,Notifier}.swift`, `Info.plist`, `scripts/build-app.sh`

**Interfaces:**
- Consumes: `HerdbotKit` (`MenuState`, `ProcessCLI`, `IconState`, `Event`, `CLIError`).
- Produces: `build/HerdbotMenu.app` (bundle id `dev.herdbot.menu`, `LSUIElement`, ad-hoc signed).

- [ ] **Step 1: Add the app target**

Replace `ui/menubar/Package.swift` with:

```swift
// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "HerdbotMenu",
    platforms: [.macOS(.v14)],
    targets: [
        .target(name: "HerdbotKit"),
        .executableTarget(name: "HerdbotMenu", dependencies: ["HerdbotKit"]),
        .testTarget(name: "HerdbotKitTests", dependencies: ["HerdbotKit"]),
    ]
)
```

- [ ] **Step 2: Write the app sources**

`Sources/HerdbotMenu/App.swift`:

```swift
import AppKit
import HerdbotKit
import SwiftUI

@main
struct HerdbotMenuApp: App {
    // A plain `let`: the App struct is built once, and @State is a macro whose plugin needs full Xcode.
    private let state: MenuState
    private let cli: ProcessCLI?
    private let notifier: Notifier

    init() {
        let state = MenuState()
        let cli = ProcessCLI.locate().map { ProcessCLI(binary: $0) }
        let notifier = Notifier(cli: cli)
        state.onBlocked = { notifier.post($0) }
        if let cli {
            Task { await state.follow(cli) }
        } else {
            state.fail(CLIError.notFound.localizedDescription)
        }
        self.state = state
        self.cli = cli
        self.notifier = notifier
    }

    var body: some Scene {
        MenuBarExtra {
            MenuContent(state: state, cli: cli)
        } label: {
            MenuIcon(icon: state.icon)
        }
    }
}
```

`Sources/HerdbotMenu/MenuIcon.swift`:

```swift
import HerdbotKit
import SwiftUI

struct MenuIcon: View {
    let icon: IconState

    var body: some View {
        switch icon {
        case .idle: Image(systemName: "circle")
        case .running: Image(systemName: "circle.fill")
        case .blocked(let count):
            HStack(spacing: 2) {
                Image(systemName: "exclamationmark.circle.fill")
                Text("\(count)")
            }
        case .daemonDown: Image(systemName: "circle.slash")
        case .unavailable: Image(systemName: "questionmark.circle")
        }
    }
}
```

`Sources/HerdbotMenu/MenuContent.swift`:

```swift
import AppKit
import HerdbotKit
import SwiftUI

struct MenuContent: View {
    let state: MenuState
    let cli: ProcessCLI?

    var body: some View {
        if let problem = state.problem {
            Text(problem)
        } else if let snapshot = state.snapshot {
            if snapshot.daemon == "down" {
                Text("daemon not running; messages won't deliver")
            }
            if !snapshot.blocked.isEmpty {
                Section("Waiting on you") {
                    ForEach(snapshot.blocked, id: \.self) { event in
                        Button("\(event.agent): \(event.reason ?? "waiting")") { focus(event.agent) }
                    }
                }
            }
            Section("Bots") {
                ForEach(snapshot.bots, id: \.slug) { bot in
                    Text("\(bot.up ? "●" : "○") \(bot.title.isEmpty ? bot.slug : bot.title)")
                }
            }
            if !snapshot.runs.isEmpty {
                Section("Running") {
                    ForEach(snapshot.runs, id: \.id) { run in
                        Button("\(run.bot): \(run.description) (\(run.route))") { focus(run.agent) }
                    }
                }
            }
            if !snapshot.cooldowns.isEmpty {
                Section("Cooling down") {
                    ForEach(snapshot.cooldowns, id: \.harness) { cooldown in
                        Text("\(cooldown.harness) until \(until(cooldown.until)): \(cooldown.reason)")
                    }
                }
            }
            if !snapshot.warnings.isEmpty {
                Text("\(snapshot.warnings.count) warning(s): \(snapshot.warnings[0])")
            }
        } else {
            Text("waiting for herdbot…")
        }
        Divider()
        Button("Quit") { NSApplication.shared.terminate(nil) }
    }

    private func focus(_ agent: String) {
        Task { try? await cli?.focus(agent: agent) }
    }

    private func until(_ seconds: Int) -> String {
        Date(timeIntervalSince1970: TimeInterval(seconds)).formatted(date: .omitted, time: .shortened)
    }
}
```

`Sources/HerdbotMenu/Notifier.swift`:

```swift
import HerdbotKit
import UserNotifications

/// Posts a notification per newly blocked agent; clicking one focuses that agent's pane.
/// Needs an app bundle: a bare `swift run` binary has no bundle id, so it stays silent.
@MainActor
final class Notifier: NSObject, UNUserNotificationCenterDelegate {
    private let cli: ProcessCLI?
    private let enabled = Bundle.main.bundleIdentifier != nil

    init(cli: ProcessCLI?) {
        self.cli = cli
        super.init()
        guard enabled else { return }
        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func post(_ event: Event) {
        guard enabled else { return }
        let content = UNMutableNotificationContent()
        content.title = "\(event.agent) is waiting"
        content.body = event.reason ?? "needs your input"
        content.sound = .default
        content.userInfo = ["agent": event.agent]
        let request = UNNotificationRequest(identifier: "blocked-\(event.agent)-\(event.at)", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        guard let agent = response.notification.request.content.userInfo["agent"] as? String else { return }
        await focus(agent)
    }

    private func focus(_ agent: String) async {
        try? await cli?.focus(agent: agent)
    }
}
```

- [ ] **Step 3: Build and re-run the tests**

Run: `swift build 2>&1 | grep -E "error|warning:|Build complete" | grep -v "ld: warning"; ./scripts/test.sh 2>&1 | grep -E "error|Test run"`
Expected: `Build complete!` with no Swift warnings (the `ld: warning: search path … not found` lines are harmless Command Line Tools noise), and 14 tests passing.

- [ ] **Step 4: Add the bundle files and build the app**

`Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>dev.herdbot.menu</string>
    <key>CFBundleName</key><string>HerdbotMenu</string>
    <key>CFBundleExecutable</key><string>HerdbotMenu</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
```

`scripts/build-app.sh`:

```bash
#!/bin/bash
# Build build/HerdbotMenu.app. Notifications need a bundle id; the signature is ad-hoc.
set -euo pipefail
cd "$(dirname "$0")/.."
swift build -c release
bin="$(swift build -c release --show-bin-path)/HerdbotMenu"
app="build/HerdbotMenu.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS"
cp "$bin" "$app/Contents/MacOS/HerdbotMenu"
cp Info.plist "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
echo "built $app"
```

Run `chmod +x scripts/build-app.sh`, then `./scripts/build-app.sh`.
Expected: `Info.plist: OK`, `built build/HerdbotMenu.app`. Then check:

Run: `codesign -dv build/HerdbotMenu.app 2>&1 | grep -E "Identifier|Signature"; plutil -extract LSUIElement raw build/HerdbotMenu.app/Contents/Info.plist`
Expected: `Identifier=dev.herdbot.menu`, `Signature=adhoc`, and `true`. Do not launch it (see Global Constraints).

- [ ] **Step 5: Commit**

```bash
git add ui/menubar/Package.swift ui/menubar/Sources/HerdbotMenu ui/menubar/Info.plist ui/menubar/scripts/build-app.sh
git commit -m "feat: add the SwiftUI menu bar app and app bundle script"
```

---

### Task 5: Docs and the manual checklist

**Files:**
- Create: `ui/menubar/README.md`
- Modify: `README.md`, `docs/superpowers/specs/2026-10-21-ui-layer-design.md`, `docs/superpowers/plans/README.md`

- [ ] **Step 1: Write `ui/menubar/README.md`**

````markdown
# herdbot menu bar

A SwiftUI menu bar app for herdbot. It follows `herdbot watch --json`, shows bots, running
subagents, cooling harnesses and blocked agents, and posts a notification when an agent becomes
blocked; clicking it runs `herdbot focus <agent>`. Design: `docs/superpowers/specs/2026-10-21-ui-layer-design.md`.

Needs macOS 14+, Swift 6 (Command Line Tools is enough), and a herdbot that has `watch --json`
and `focus` (install with `cargo install --path crates/herdbot`, then restart the daemon).

```bash
./scripts/test.sh          # unit tests (use this, not bare `swift test`)
./scripts/build-app.sh     # builds build/HerdbotMenu.app, ad-hoc signed
open build/HerdbotMenu.app
```

The app finds herdbot at `$HERDBOT_BIN`, `~/.cargo/bin/herdbot`, `/opt/homebrew/bin/herdbot` or
`/usr/local/bin/herdbot`. To start it at login, add it in System Settings → General → Login Items
(the app does not register itself).

Icon: `○` idle, `●` subagents running, `⚠ N` agents blocked, `⊘` daemon down, `?` herdbot missing.

## Manual check (needs a person: notification permission and a click)

1. `cargo install --path crates/herdbot` and restart the daemon, so `herdbot watch --json` exists.
2. Use a throwaway state dir so nothing real is touched:
   `export HB=$(mktemp -d); mkdir -p $HB/.state/events`
3. Start the app with it: `HERDBOT_HOME=$HB build/HerdbotMenu.app/Contents/MacOS/HerdbotMenu &`
   Allow notifications when macOS asks. Expect a menu bar icon; its menu shows "daemon not running"
   (nothing listens on the throwaway home's lock socket).
4. Block a real agent so the click has somewhere to go:
   `echo '{"at":1,"agent":"herdbot-dev","kind":"blocked","reason":"approval: demo","harness":"claude"}' >> $HB/.state/events/herdbot-dev.jsonl`
   Expect within ~2 s: the icon shows `⚠ 1`, the menu lists "herdbot-dev: approval: demo", and a
   notification "herdbot-dev is waiting" appears.
5. Click the notification: herdr focuses the herdbot-dev pane.
6. Clear it: append `{"at":2,"agent":"herdbot-dev","kind":"turn_end","harness":"claude"}` to the same file; the icon returns to idle.
7. Quit from the menu; `rm -r $HB`.
````

- [ ] **Step 2: Update the other docs**

- `README.md`: in "Status", change "Not built yet: … and the desktop UI" to say the menu bar app exists in `ui/menubar` (built separately with Swift; see its README) and the full desktop app is not built. In the roadmap's UI bullet, add one sentence pointing to `ui/menubar/README.md`.
- Spec: in Phase 1, replace "Polls `status --json` about every 2 s and tails `watch --json` for blocked events." with "Follows `watch --json` only (its first line is a snapshot and it repeats on every change), reconnecting with a capped backoff." In "Risks and assumptions", replace the Swift-layout assumption with "The Swift package lives in `ui/menubar/` and builds with Command Line Tools (`./scripts/build-app.sh`); login-item registration is deferred (add the app in System Settings → Login Items)."
- `docs/superpowers/plans/README.md`: add a row for this plan after the phase 0 row: `| [Desktop UI, phase 1: menu bar app](2026-10-21-menubar-app.md) | done (committed on feature/ui-menubar), 2026-10-21 | new \`ui/menubar\` Swift package, README | ` (use the same format as the other rows; write "done" only after the commit below exists, as in earlier plans).

- [ ] **Step 3: Final checks and commit**

Run from `ui/menubar`: `swift build 2>&1 | grep -E "error|Build complete"; ./scripts/test.sh 2>&1 | grep -E "error|Test run"; ./scripts/build-app.sh | tail -1`
Run from the repo root: `cargo build && cargo test --workspace 2>&1 | grep "test result" | head -3` (the Rust side is untouched; expect 57 / 14 / 1 / 2 passed).

```bash
git add ui/menubar/README.md README.md docs/superpowers/specs/2026-10-21-ui-layer-design.md docs/superpowers/plans/README.md
git commit -m "docs: document the menu bar app and its manual check"
```

---

## Self-review (spec coverage)

- Menu bar icon states (idle/running/blocked/daemon down/unavailable): `MenuState.icon`, `MenuIcon`, tested in `StateTests`.
- Lists bots, running subagents, cooling harnesses with time left, warnings: `MenuContent`.
- Notification on blocked, click focuses the pane: `NotificationTracker` + `MenuState.onBlocked` + `Notifier`; click path is the manual check (a click can't be automated).
- `"v"` mismatch shows "update herdbot": `StreamParser` throws `.unsupported`, `CLIError.errorDescription`, shown by `MenuContent` via `problem`.
- Daemon down dimmed icon and message: `IconState.daemonDown` and the "daemon not running" row.
- Mocked-CLI testing: `fakeHerdbot` scripts for `ProcessCLI`; `MenuState.apply` driven by `StreamLine` values; one live contract test against the real binary.
- Quick-entry and login item: out of scope (phase 2 / deferred, stated above).
- Names are consistent across tasks: `HerdbotCLI.watch/focus`, `StreamParser.parse`, `MenuState.apply/fail/follow`, `NotificationTracker.fresh`, `ProcessCLI.locate`.
