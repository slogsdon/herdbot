import Carbon.HIToolbox

/// One system-wide hotkey via Carbon. It needs no Accessibility permission and never sees other
/// keys, so it cannot interfere with dictation tools that watch a modifier (MacWhisper's Right ⌘).
@MainActor
final class GlobalHotkey {
    private static var action: (@MainActor () -> Void)?
    private var reference: EventHotKeyRef?

    /// False when another app already owns the combination.
    func register(keyCode: Int, modifiers: Int, action: @escaping @MainActor () -> Void) -> Bool {
        Self.action = action
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in
            // Carbon delivers hotkey events on the main thread.
            MainActor.assumeIsolated { GlobalHotkey.action?() }
            return noErr
        }, 1, &spec, nil, nil)
        let id = EventHotKeyID(signature: OSType(0x4842_4B59), id: 1)  // 'HBKY'
        let status = RegisterEventHotKey(UInt32(keyCode), UInt32(modifiers), id, GetApplicationEventTarget(), 0, &reference)
        return status == noErr
    }
}
