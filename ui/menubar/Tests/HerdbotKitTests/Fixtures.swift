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
