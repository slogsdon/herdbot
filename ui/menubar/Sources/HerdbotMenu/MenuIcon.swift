import HerdbotKit
import SwiftUI

struct MenuIcon: View {
    let icon: IconState

    var body: some View {
        switch icon {
        case .idle: Image(systemName: "circle")
        case .running: Image(systemName: "circle.fill")
        case .blocked(let count):
            HStack(spacing: 2) {
                Image(systemName: "exclamationmark.circle.fill")
                Text("\(count)")
            }
        case .daemonDown: Image(systemName: "circle.slash")
        case .unavailable: Image(systemName: "questionmark.circle")
        }
    }
}
