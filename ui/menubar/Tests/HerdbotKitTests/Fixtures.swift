import Foundation
import Testing
@testable import HerdbotKit

let snapshotLine = """
{"v":1,"type":"snapshot","snapshot":{"v":1,"at":100,"daemon":"up",\
"bots":[{"slug":"alpha","title":"Alpha","up":true}],\
"rooms":[{"slug":"crew","name":"Crew","mode":"round_robin","paused":false}],\
"runs":[{"id":"r1","bot":"alpha","agent":"alpha-r1","route":"claude:sonnet","description":"do it"}],\
"cooldowns":[{"harness":"claude","until":4102444800,"reason":"limit"}],\
"blocked":[{"at":10,"agent":"alpha-main","kind":"blocked","reason":"approval: Bash","harness":"claude"}],\
"warnings":["x"]}}
"""

let eventLine = """
{"v":1,"type":"event","event":{"at":20,"agent":"alpha-main","kind":"turn_end","text":"done","harness":"claude"}}
"""

func snapshot(daemon: String = "up", runs: Int = 0, blocked: [Event] = [], bots: [BotView] = [], rooms: [RoomView] = []) -> Snapshot {
    Snapshot(
        v: 1, at: 1, daemon: daemon, bots: bots, rooms: rooms,
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

/// Answers `doctor` from a queue of reports (the last one repeats) and records the fixes asked for.
final class FakeSetupCLI: SetupCLI, @unchecked Sendable {
    nonisolated(unsafe) var reports: [DoctorReport]
    nonisolated(unsafe) var fixes: [String?] = []
    nonisolated(unsafe) var failure: String?
    nonisolated(unsafe) var doctorFailure: String?
    init(_ reports: [DoctorReport]) { self.reports = reports }
    func doctor() async throws -> DoctorReport {
        if let doctorFailure { throw CLIError.failed(doctorFailure) }
        return reports.count > 1 ? reports.removeFirst() : reports[0]
    }
    func setup(fix: String?) async throws -> String {
        fixes.append(fix)
        if let failure { throw CLIError.failed(failure) }
        return "ok"
    }
}

func report(_ checks: DoctorCheck...) -> DoctorReport { DoctorReport(version: "0.1.0", checks: checks) }
let okCheck = DoctorCheck(id: "herdr", status: .ok, message: "herdr 0.9.1")
let failService = DoctorCheck(id: "service", status: .fail, message: "not installed", fix: "service")
let warnLink = DoctorCheck(id: "cli-link", status: .warn, message: "no herdbot on PATH", fix: "cli-link")

/// Records what quick entry sends; `failure` makes the next send throw.
final class FakeCLI: HerdbotCLI, @unchecked Sendable {
    nonisolated(unsafe) var sent: [(Recipient, String)] = []
    nonisolated(unsafe) var failure: String?

    func watch() -> AsyncThrowingStream<StreamLine, Error> { AsyncThrowingStream { $0.finish() } }
    func focus(agent: String) async throws {}
    func send(to recipient: Recipient, text: String) async throws {
        if let failure { throw CLIError.failed(failure) }
        sent.append((recipient, text))
    }
    /// What `history` answers, keyed by target; an absent target is an empty conversation.
    nonisolated(unsafe) var pages: [Recipient: [HistoryItem]] = [:]
    nonisolated(unsafe) var pending: [Recipient: [HistoryItem]] = [:]
    nonisolated(unsafe) var historyCalls: [(Recipient, Int?, Int)] = []
    nonisolated(unsafe) var historyFailure: String?
    func history(of target: Recipient, before: Int?, limit: Int) async throws -> HistoryPage {
        historyCalls.append((target, before, limit))
        if let historyFailure { throw CLIError.failed(historyFailure) }
        let all = pages[target] ?? []
        let end = min(before ?? all.count, all.count)
        let slice = Array(all[max(0, end - limit)..<end])
        return HistoryPage(target: target.historyTarget, total: all.count, items: slice + (before == nil ? pending[target] ?? [] : []))
    }
}
