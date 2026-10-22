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
        home: String = NSHomeDirectory(),
        bundlePath: String? = Bundle.main.bundlePath
    ) -> URL? {
        let helper = bundlePath.map { $0 + "/Contents/Helpers/herdbot" }
        let candidates = [environment["HERDBOT_BIN"], helper, home + "/.cargo/bin/herdbot", "/opt/homebrew/bin/herdbot", "/usr/local/bin/herdbot"]
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
        try await run(["focus", agent])
    }

    /// The text is one argv entry, so spaces, quotes and newlines reach herdbot untouched.
    public func send(to recipient: Recipient, text: String) async throws {
        switch recipient {
        case .bot(let slug): try await run(["send", slug, text])
        case .room(let slug): try await run(["room", "send", slug, text])
        }
    }

    public func history(of target: Recipient, before: Int?, limit: Int) async throws -> HistoryPage {
        var arguments = ["history", target.historyTarget, "--json", "--limit", String(limit)]
        if let before { arguments += ["--before", String(before)] }
        let data = try await output(arguments)
        let page = try JSONDecoder().decode(HistoryPage.self, from: data)
        guard page.v == supportedVersion else { throw CLIError.unsupported(page.v) }
        return page
    }

    private func run(_ arguments: [String]) async throws {
        _ = try await output(arguments)
    }

    /// Run `herdbot <arguments>` to completion and return its stdout; a non-zero exit throws its
    /// stderr. Stdout is read to its end while the process runs, so a large page can't stall it; stderr
    /// (herdbot's errors are one line) is read after it.
    private func output(_ arguments: [String], acceptingExit accepted: Set<Int32> = [0]) async throws -> Data {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Data, Error>) in
            let process = Process()
            let outputs = Pipe()
            let errors = Pipe()
            process.executableURL = binary
            process.arguments = arguments
            process.environment = environment
            process.standardOutput = outputs
            process.standardError = errors
            do {
                try process.run()
            } catch {
                continuation.resume(throwing: CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
                return
            }
            DispatchQueue.global().async {
                let data = outputs.fileHandleForReading.readDataToEndOfFile()
                let errorText = errors.fileHandleForReading.readDataToEndOfFile()
                process.waitUntilExit()
                if accepted.contains(process.terminationStatus) && (process.terminationStatus == 0 || !data.isEmpty) {
                    continuation.resume(returning: data)
                } else {
                    let text = String(decoding: errorText, as: UTF8.self)
                    continuation.resume(throwing: CLIError.failed(text.trimmingCharacters(in: .whitespacesAndNewlines)))
                }
            }
        }
    }
}

extension ProcessCLI: SetupCLI {
    public func doctor() async throws -> DoctorReport {
        let data = try await output(["doctor", "--json"], acceptingExit: [0, 1])
        let report = try JSONDecoder().decode(DoctorReport.self, from: data)
        guard report.v == supportedDoctorVersion else { throw CLIError.unsupported(report.v) }
        return report
    }

    public func setup(fix: String?) async throws -> String {
        var arguments = ["setup", "--yes"]
        if let fix { arguments += ["--only", fix] }
        return String(decoding: try await output(arguments), as: UTF8.self)
    }
}
