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

func snapshot(daemon: String = "up", runs: Int = 0, blocked: [Event] = [], bots: [BotView] = [], rooms: [RoomView] = [], cards: [Card] = []) -> Snapshot {
    Snapshot(
        v: 1, at: 1, daemon: daemon, bots: bots, rooms: rooms,
        runs: (0..<runs).map { RunView(id: "r\($0)", bot: "a", agent: "a-r\($0)", route: "x", description: "d") },
        cooldowns: [], blocked: blocked, warnings: [], cards: cards)
}

final class FakeCardCLI: CardCLI, @unchecked Sendable {
    nonisolated(unsafe) var answers: [(String, String)] = []
    nonisolated(unsafe) var failure: String?
    func answerCard(id: String, option: String) async throws {
        if let failure { throw CLIError.failed(failure) }
        answers.append((id, option))
    }
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
    nonisolated(unsafe) var output = "ok"
    init(_ reports: [DoctorReport]) { self.reports = reports }
    func doctor() async throws -> DoctorReport {
        if let doctorFailure { throw CLIError.failed(doctorFailure) }
        return reports.count > 1 ? reports.removeFirst() : reports[0]
    }
    func setup(fix: String?) async throws -> String {
        fixes.append(fix)
        if let failure { throw CLIError.failed(failure) }
        return output
    }
}

func report(_ checks: DoctorCheck...) -> DoctorReport { DoctorReport(version: "0.1.0", checks: checks) }
let okCheck = DoctorCheck(id: "herdr", status: .ok, message: "herdr 0.9.1")
let failService = DoctorCheck(id: "service", status: .fail, message: "not installed", fix: "service")
let warnLink = DoctorCheck(id: "cli-link", status: .warn, message: "no herdbot on PATH", fix: "cli-link")
let failHarnesses = DoctorCheck(id: "harnesses", status: .fail, message: "none of claude, codex, cursor-agent, pi is on PATH")

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

/// `herdbot config list --json` for the global scope. Values cover every shape the contract
/// sends: string, integer, table and an unset (`null`) setting.
let globalSettingsJSON = """
{"version":1,"scope":"global","settings":[\
{"key":"approvals.mode","type":"enum","description":"Whether agents ask before each tool action (ask) or use automatic review (auto).","default":"ask","value":"auto","choices":["ask","auto"],"apply":"next-bot-start"},\
{"key":"memory.max_compactors","type":"integer","description":"Maximum memory compactor runs across all bots at once. 0 stops new runs.","default":2,"value":4,"apply":"immediate"},\
{"key":"paths","type":"map","description":"Path rewrites applied to bot instructions when a bot launches.","default":{},"value":{"/workspace/vault":"~/Documents/vault"},"apply":"next-bot-start"},\
{"key":"cua.host","type":"string","description":"SSH host that runs cua-driver for computer_use agents. Unset means this Mac.","default":null,"value":null,"apply":"next-bot-start"}]}
"""

/// `herdbot config list --json` for one bot: a string that applies immediately, an enum that
/// inherits the global mode and a boolean.
let botSettingsJSON = """
{"version":1,"scope":"bot","settings":[\
{"key":"title","type":"string","description":"Display title shown in bot listings. The slug remains its identifier.","default":"","value":"Helper","apply":"immediate"},\
{"key":"approvals","type":"enum","description":"Approval mode for the main agent and subagents. Unset inherits the global approvals.mode.","default":"ask","value":"auto","choices":["ask","auto"],"apply":"next-bot-start"},\
{"key":"memory","type":"boolean","description":"Keep an OptChat memory log and add a view of it to the bot instructions.","default":false,"value":true,"apply":"next-bot-start"}]}
"""

/// `herdbot config get|set|unset memory --bot helper --json`: one setting in the same envelope.
let botMemorySettingJSON = """
{"version":1,"scope":"bot","setting":{"key":"memory","type":"boolean","description":"Keep an OptChat memory log and add a view of it to the bot instructions.","default":false,"value":true,"apply":"next-bot-start"}}
"""

/// The settings from a `config list --json` payload.
func settings(from json: String) throws -> [SettingDescriptor] {
    try JSONDecoder().decode(SettingsList.self, from: Data(json.utf8)).settings
}

/// The setting from a `config get|set|unset --json` payload.
func setting(from json: String) throws -> SettingDescriptor {
    try JSONDecoder().decode(SettingsEntry.self, from: Data(json.utf8)).setting
}
