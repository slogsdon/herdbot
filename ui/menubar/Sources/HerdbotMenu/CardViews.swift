import HerdbotKit
import SwiftUI

/// One card in a conversation. `kind` picks the shape; unknown kinds render generically.
struct CardView: View {
    let card: Card
    let answers: CardAnswers
    let cli: ProcessCLI?

    var body: some View {
        switch card.kind {
        case "approval": ApprovalCardView(card: card, answers: answers, cli: cli)
        default: GenericCardView(card: card, cli: cli)
        }
    }
}

struct ApprovalCardView: View {
    let card: Card
    let answers: CardAnswers
    let cli: ProcessCLI?

    var body: some View {
        let state = answers.state(for: card)
        VStack(alignment: .leading, spacing: 10) {
            Label(card.title, systemImage: "hand.raised.fill").font(.headline).foregroundStyle(.orange)
            ScrollView(.horizontal, showsIndicators: false) {
                Text(card.body).font(.system(.callout, design: .monospaced)).textSelection(.enabled).padding(8)
            }
            .background(.black.opacity(0.12), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            HStack {
                ForEach(card.options) { option in
                    Button(role: option.style == .destructive ? .destructive : nil) {
                        Task { await answers.answer(card, option: option, using: cli) }
                    } label: { Text(option.label) }
                    .disabled(isAnswering(state))
                }
                if isAnswering(state) { ProgressView().controlSize(.small) }
                Spacer()
                Button("Jump to pane") { Task { try? await cli?.focus(agent: card.agent) } }.controlSize(.small)
            }
            if case .failed(let message) = state {
                Text(message).font(.caption).foregroundStyle(.red).textSelection(.enabled)
            }
        }
        .padding(12)
        .background(.orange.opacity(0.12), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Approval needed: \(card.title)")
    }

    private func isAnswering(_ state: CardAnswerState) -> Bool {
        if case .answering = state { true } else { false }
    }
}

struct GenericCardView: View {
    let card: Card
    let cli: ProcessCLI?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(card.title).font(.headline)
            if !card.body.isEmpty { Text(card.body).font(.callout).textSelection(.enabled) }
            Button("Jump to pane") { Task { try? await cli?.focus(agent: card.agent) } }.controlSize(.small)
        }
        .padding(12)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
    }
}
