import Foundation

private final class ProcessOutput: @unchecked Sendable {
    private let continuation: CheckedContinuation<Data, Error>
    private let accepted: Set<Int32>
    private let lock = NSLock()
    private var process: Process?
    private var output = Data()
    private var errors = Data()
    private var status: Int32?
    private var outputEnded = false
    private var errorsEnded = false
    private var completed = false

    init(process: Process, continuation: CheckedContinuation<Data, Error>, accepted: Set<Int32>) {
        self.process = process
        self.continuation = continuation
        self.accepted = accepted
    }

    func appendOutput(_ data: Data) { append(data, toErrors: false) }
    func appendErrors(_ data: Data) { append(data, toErrors: true) }
    func finishOutput() { finish(errors: false) }
    func finishErrors() { finish(errors: true) }

    func terminate(_ status: Int32) {
        lock.lock()
        self.status = status
        lock.unlock()
        resumeIfReady()
    }

    func fail(_ error: Error) {
        lock.lock()
        guard !completed else { lock.unlock(); return }
        completed = true
        let process = process
        self.process = nil
        lock.unlock()
        process?.terminationHandler = nil
        continuation.resume(throwing: error)
    }

    private func append(_ data: Data, toErrors: Bool) {
        lock.lock()
        if toErrors { errors.append(data) } else { output.append(data) }
        lock.unlock()
    }

    private func finish(errors: Bool) {
        lock.lock()
        if errors { errorsEnded = true } else { outputEnded = true }
        lock.unlock()
        resumeIfReady()
    }

    private func resumeIfReady() {
        lock.lock()
        guard !completed, let status, outputEnded, errorsEnded else { lock.unlock(); return }
        completed = true
        let output = output
        let errors = errors
        let process = process
        self.process = nil
        lock.unlock()
        process?.terminationHandler = nil

        if accepted.contains(status) && (status == 0 || !output.isEmpty) {
            continuation.resume(returning: output)
        } else {
            let text = String(decoding: errors, as: UTF8.self)
            continuation.resume(throwing: CLIError.failed(text.trimmingCharacters(in: .whitespacesAndNewlines)))
        }
    }
}

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
        bundlePath: String? = Bundle.main.bundlePath,
        systemPaths: [String] = ["/opt/homebrew/bin/herdbot", "/usr/local/bin/herdbot"]
    ) -> URL? {
        let helper = bundlePath.map { $0 + "/Contents/Helpers/herdbot" }
        let candidates = [environment["HERDBOT_BIN"], helper, home + "/.cargo/bin/herdbot"] + systemPaths
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
    /// stderr. Both streams are drained while the process runs, so a large page can't stall it.
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
            let result = ProcessOutput(process: process, continuation: continuation, accepted: accepted)
            outputs.fileHandleForReading.readabilityHandler = { handle in
                let data = handle.availableData
                if data.isEmpty {
                    handle.readabilityHandler = nil
                    result.finishOutput()
                } else {
                    result.appendOutput(data)
                }
            }
            errors.fileHandleForReading.readabilityHandler = { handle in
                let data = handle.availableData
                if data.isEmpty {
                    handle.readabilityHandler = nil
                    result.finishErrors()
                } else {
                    result.appendErrors(data)
                }
            }
            process.terminationHandler = { terminatedProcess in
                result.terminate(terminatedProcess.terminationStatus)
            }
            do {
                try process.run()
            } catch {
                outputs.fileHandleForReading.readabilityHandler = nil
                errors.fileHandleForReading.readabilityHandler = nil
                try? outputs.fileHandleForReading.close()
                try? errors.fileHandleForReading.close()
                result.fail(CLIError.failed("could not start herdbot: \(error.localizedDescription)"))
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

extension ProcessCLI: CardCLI {
    public func answerCard(id: String, option: String) async throws {
        _ = try await output(["card", "answer", id, option])
    }
}

extension ProcessCLI: SettingsCLI {
    public func listSettings(bot: String?) async throws -> [SettingDescriptor] {
        let data = try await output(settingsArguments(["config", "list", "--json"], bot: bot))
        return try settings(SettingsList.self, from: data, command: "config list").settings
    }

    public func getSetting(key: String, bot: String?) async throws -> SettingDescriptor {
        let data = try await output(settingsArguments(["config", "get", key, "--json"], bot: bot))
        return try settings(SettingsEntry.self, from: data, command: "config get").setting
    }

    /// The value is one argv entry, so spaces reach herdbot untouched and it validates them.
    public func setSetting(key: String, value: String, bot: String?) async throws -> SettingDescriptor {
        let data = try await output(settingsArguments(["config", "set", key, value, "--json"], bot: bot))
        return try settings(SettingsEntry.self, from: data, command: "config set").setting
    }

    public func unsetSetting(key: String, bot: String?) async throws -> SettingDescriptor {
        let data = try await output(settingsArguments(["config", "unset", key, "--json"], bot: bot))
        return try settings(SettingsEntry.self, from: data, command: "config unset").setting
    }

    /// `--bot` selects the bot scope, so a nil bot must not be passed at all.
    private func settingsArguments(_ arguments: [String], bot: String?) -> [String] {
        guard let bot else { return arguments }
        return arguments + ["--bot", bot]
    }

    /// An unreadable payload means an older or newer herdbot, not a setting to write blindly.
    private func settings<T: Decodable>(_ type: T.Type, from data: Data, command: String) throws -> T {
        do {
            return try JSONDecoder().decode(type, from: data)
        } catch let error as CLIError {
            throw error
        } catch {
            throw CLIError.failed("herdbot \(command) printed settings this app cannot read; update herdbot or this app")
        }
    }
}
