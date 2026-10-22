import HerdbotKit
import SwiftUI

/// Inline markdown, with fenced code blocks in monospace. Selectable.
struct MarkdownText: View {
    let source: String

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(Array(Markdown.blocks(source).enumerated()), id: \.offset) { _, block in
                switch block {
                case .text(let text):
                    Text(inline(text))
                case .code(_, let body):
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(body).font(.system(.callout, design: .monospaced)).padding(8)
                    }
                    .background(.black.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
            }
        }
        .textSelection(.enabled)
    }

    private func inline(_ text: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        return (try? AttributedString(markdown: text, options: options)) ?? AttributedString(text)
    }
}

struct SpeakerColor {
    static func color(for slug: String) -> Color {
        Color(hue: Avatar.hue(for: slug), saturation: 0.6, brightness: 0.8)
    }
}

/// One message: yours on the right, the bot's on the left, notices small and centred.
struct MessageRow: View {
    let item: HistoryItem
    /// Rooms name the speaker above each bot message.
    let showSpeaker: Bool

    var body: some View {
        switch item.kind {
        case "user": bubble(alignment: .trailing, fill: AnyShapeStyle(Color.accentColor), foreground: .white)
        case "notice":
            Text(item.text).font(.caption).foregroundStyle(.secondary).frame(maxWidth: .infinity)
        default: bubble(alignment: .leading, fill: AnyShapeStyle(.quaternary), foreground: .primary)
        }
    }

    private func bubble(alignment: HorizontalAlignment, fill: AnyShapeStyle, foreground: Color) -> some View {
        VStack(alignment: alignment, spacing: 2) {
            if showSpeaker, let speaker = item.speaker {
                Text(speaker).font(.caption.weight(.semibold)).foregroundStyle(SpeakerColor.color(for: speaker))
            }
            MarkdownText(source: item.text)
                .foregroundStyle(foreground)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(fill, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
                .opacity(item.isPending ? 0.6 : 1)
            if let state = item.state {
                HStack(spacing: 4) {
                    if state == "inflight" { ProgressView().controlSize(.mini) }
                    Text(state == "inflight" ? "working" : "queued").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .frame(maxWidth: .infinity, alignment: alignment == .trailing ? .trailing : .leading)
        .padding(alignment == .trailing ? .leading : .trailing, 60)
    }
}

struct DaySeparator: View {
    let label: String

    var body: some View {
        Text(label)
            .font(.caption.weight(.medium))
            .foregroundStyle(.secondary)
            .padding(.horizontal, 10)
            .padding(.vertical, 3)
            .background(.quaternary, in: Capsule())
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
    }
}
