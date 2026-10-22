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
    /// `herdbot send <bot> <text>` or `herdbot room send <room> <text>`: queues the text.
    func send(to recipient: Recipient, text: String) async throws
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
