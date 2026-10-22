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
        try await run(["focus", agent])
    }

    /// The text is one argv entry, so spaces, quotes and newlines reach herdbot untouched.
    public func send(to recipient: Recipient, text: String) async throws {
        switch recipient {
        case .bot(let slug): try await run(["send", slug, text])
        case .room(let slug): try await run(["room", "send", slug, text])
        }
    }

    /// Run `herdbot <arguments>` to completion; a non-zero exit throws its stderr.
    private func run(_ arguments: [String]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let process = Process()
            let errors = Pipe()
            process.executableURL = binary
            process.arguments = arguments
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
