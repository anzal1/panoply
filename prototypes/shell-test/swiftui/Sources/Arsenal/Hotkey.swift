import Carbon.HIToolbox
import Foundation

/// Control+Option+1 through Carbon RegisterEventHotKey (no Accessibility or Input Monitoring permission).
enum Hotkey {
    nonisolated(unsafe) static var handler: ((Double) -> Void)?
    nonisolated(unsafe) private static var ref: EventHotKeyRef?

    static func register(_ onPress: @escaping (Double) -> Void) {
        handler = onPress
        var spec = EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
        InstallEventHandler(GetApplicationEventTarget(), { _, _, _ in
            let t0 = nowMs()
            Hotkey.handler?(t0)
            return noErr
        }, 1, &spec, nil, nil)
        let id = EventHotKeyID(signature: OSType(0x41525345), id: 1)
        RegisterEventHotKey(UInt32(kVK_ANSI_1), UInt32(controlKey | optionKey), id, GetApplicationEventTarget(), 0, &ref)
    }
}
