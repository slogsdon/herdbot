import SwiftUI

/// `openWindow` only exists inside a view. A view that is always alive (the menu bar item's icon)
/// hands it over here, so a notification click can open the chat window too.
@MainActor
final class WindowOpener {
    var open: () -> Void = {}
}

struct OpenerCapture: ViewModifier {
    let opener: WindowOpener
    @Environment(\.openWindow) private var openWindow

    func body(content: Content) -> some View {
        content.task { opener.open = { openWindow(id: "chat") } }
    }
}
