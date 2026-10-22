import HerdbotKit
import SwiftUI

struct AvatarView: View {
    let slug: String
    let title: String
    var size: CGFloat = 34

    var body: some View {
        Circle()
            .fill(Color(hue: Avatar.hue(for: slug), saturation: 0.45, brightness: 0.85))
            .frame(width: size, height: size)
            .overlay(Text(Avatar.initial(for: title)).font(.system(size: size * 0.42, weight: .semibold)).foregroundStyle(.white))
    }
}

private struct Row: View {
    let row: SidebarRow

    var body: some View {
        HStack(spacing: 10) {
            AvatarView(slug: row.slug, title: row.title)
            VStack(alignment: .leading, spacing: 1) {
                Text(row.title).font(.body.weight(.medium)).lineLimit(1)
                Text(row.preview.isEmpty ? " " : row.preview).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 3) {
                if let at = row.lastAt { Text(time(at)).font(.caption2).foregroundStyle(.secondary) }
                if row.working { ProgressView().controlSize(.mini) }
            }
        }
        .opacity(row.dimmed ? 0.55 : 1)
        .padding(.vertical, 2)
    }

    private func time(_ seconds: Int) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        return Calendar.current.isDateInToday(date)
            ? date.formatted(.dateTime.hour().minute())
            : date.formatted(.dateTime.month(.abbreviated).day())
    }
}

/// Bots, then rooms, newest activity first, with the system search field on top.
struct SidebarView: View {
    let menu: MenuState
    let chat: ChatState

    var body: some View {
        let rows = menu.snapshot.map { Sidebar.rows(from: $0, filter: chat.filter) } ?? (bots: [], rooms: [])
        List(selection: Binding(get: { chat.selection }, set: { choice in Task { await chat.select(choice) } })) {
            if !rows.bots.isEmpty {
                Section("Bots") { ForEach(rows.bots) { Row(row: $0).tag(Optional($0.id)) } }
            }
            if !rows.rooms.isEmpty {
                Section("Rooms") { ForEach(rows.rooms) { Row(row: $0).tag(Optional($0.id)) } }
            }
        }
        .searchable(text: Binding(get: { chat.filter }, set: { chat.filter = $0 }), placement: .sidebar, prompt: "Filter")
        .overlay {
            if rows.bots.isEmpty && rows.rooms.isEmpty {
                Text(menu.problem ?? (chat.filter.isEmpty ? "No bots yet" : "No matches")).foregroundStyle(.secondary)
            }
        }
    }
}
