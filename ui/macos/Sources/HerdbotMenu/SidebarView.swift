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
    let unread: Int

    var body: some View {
        HStack(spacing: 10) {
            AvatarView(slug: row.slug, title: row.title)
            VStack(alignment: .leading, spacing: 1) {
                Text(row.title).font(.body.weight(unread > 0 ? .bold : .medium)).lineLimit(1)
                Text(row.preview.isEmpty ? " " : row.preview).font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            VStack(alignment: .trailing, spacing: 3) {
                if let at = row.lastAt { Text(time(at)).font(.caption2).foregroundStyle(.secondary) }
                if unread > 0 {
                    Text(unread > 99 ? "99+" : "\(unread)")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(Color.accentColor, in: Capsule())
                } else if row.working {
                    ProgressView().controlSize(.mini)
                }
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

/// Bots, then rooms, newest activity first, with the system search field on top, and Settings
/// pinned below the list.
struct SidebarView: View {
    let menu: MenuState
    let chat: ChatState
    let unread: UnreadTracker
    let settings: SettingsState

    var body: some View {
        let rows = menu.snapshot.map { Sidebar.rows(from: $0, filter: chat.filter) } ?? (bots: [], rooms: [])
        List(selection: Binding(get: { settings.isPresented ? nil : chat.selection }, set: { choice in
            settings.dismiss()
            Task { await chat.select(choice) }
        })) {
            if !rows.bots.isEmpty {
                Section("Bots") { ForEach(rows.bots) { Row(row: $0, unread: unread.unread($0.id)).tag(Optional($0.id)) } }
            }
            if !rows.rooms.isEmpty {
                Section("Rooms") { ForEach(rows.rooms) { Row(row: $0, unread: unread.unread($0.id)).tag(Optional($0.id)) } }
            }
            Section {
                Button { Task { await settings.present(.global) } } label: {
                    Label("Settings", systemImage: "gearshape")
                        .font(.body.weight(settings.isPresented ? .semibold : .regular))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .listRowBackground(settings.isPresented ? Color.accentColor.opacity(0.18) : Color.clear)
                .help("Global and per-bot settings")
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
