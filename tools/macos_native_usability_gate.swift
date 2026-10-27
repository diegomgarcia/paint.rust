import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

// Read-only permission/session preflight and narrowly scoped native transport for
// macos_native_usability_gate.py. This tool never launches Paint.R, requests
// permissions, unlocks the session, or synthesizes success from process state.
private let maximumAXNodes = 512
private let maximumAXDepth = 64
private let closeTitle = "Discard unsaved changes?"
private let closeExplanation = "Closing Paint.R will discard the modified document and its undo history."

private struct NativeError: Error {
    let code: String
    let message: String
}

private func error(_ code: String, _ message: String) -> NativeError {
    NativeError(code: code, message: message)
}

private func emitJSON(_ object: [String: Any]) throws {
    guard JSONSerialization.isValidJSONObject(object) else {
        throw error("NATIVE_PROTOCOL_ERROR", "native result is not a JSON object")
    }
    let data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

private func boolean(_ value: Any?) -> Bool? {
    if let value = value as? Bool { return value }
    if let value = value as? NSNumber {
        let integer = value.intValue
        if (integer == 0 || integer == 1), value.doubleValue == Double(integer) {
            return integer == 1
        }
    }
    return nil
}

private func number(_ value: Any?) -> Double? {
    if let value = value as? NSNumber { return value.doubleValue }
    if let value = value as? Double { return value }
    if let value = value as? Int { return Double(value) }
    return nil
}

private func nativePreflight() -> [String: Any] {
    let session = CGSessionCopyCurrentDictionary() as? [String: Any]
    let console = boolean(session?[kCGSessionOnConsoleKey as String])
    let lockValue = session?["CGSSessionScreenIsLocked"]
    let lockKeyPresent = lockValue != nil
    let locked = boolean(lockValue)
    // macOS omits the lock key while the active console is unlocked. A present
    // key still has to be a strict boolean or the state remains inconclusive.
    let lockStateKnown = session != nil && (!lockKeyPresent || locked != nil)
    return [
        "console_session": console ?? false,
        "session_dictionary_available": session != nil,
        "screen_locked": locked ?? false,
        "screen_lock_state_known": lockStateKnown,
        "screen_lock_key_present": lockKeyPresent,
        "accessibility_trusted": AXIsProcessTrusted(),
        "screen_recording": CGPreflightScreenCaptureAccess(),
    ]
}

private func stateBoolean(_ state: [String: Any], _ key: String) -> Bool? {
    state[key] as? Bool
}

private func requirePreflight() throws {
    let state = nativePreflight()
    if stateBoolean(state, "session_dictionary_available") != true
        || stateBoolean(state, "screen_lock_state_known") != true {
        throw error("SESSION_STATE_UNAVAILABLE", "macOS console/lock state is inconclusive")
    }
    if stateBoolean(state, "console_session") != true {
        throw error("NO_CONSOLE_SESSION", "no active console session is observable")
    }
    if stateBoolean(state, "screen_locked") != false {
        throw error("SCREEN_LOCKED", "the macOS screen is locked")
    }
    if stateBoolean(state, "accessibility_trusted") != true {
        throw error("ACCESSIBILITY_UNAVAILABLE", "Accessibility permission is unavailable")
    }
    if stateBoolean(state, "screen_recording") != true {
        throw error("SCREEN_RECORDING_UNAVAILABLE", "Screen Recording permission is unavailable")
    }
}

private func processID(_ text: String) throws -> pid_t {
    guard let value = Int32(text), value > 0 else {
        throw error("INPUT_REJECTED", "pid must be a positive 32-bit integer")
    }
    return value
}

private func processIsRunning(_ pid: pid_t) -> Bool {
    if kill(pid, 0) == 0 { return true }
    return errno == EPERM
}

private func attribute(_ element: AXUIElement, _ name: String) -> AnyObject? {
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
        return nil
    }
    return value
}

private func stringAttribute(_ element: AXUIElement, _ name: String) -> String? {
    if let value = attribute(element, name) as? String { return value }
    if let value = attribute(element, name) as? NSAttributedString { return value.string }
    return nil
}

private func boolAttribute(_ element: AXUIElement, _ name: String) -> Bool? {
    boolean(attribute(element, name))
}

private func pointAttribute(_ element: AXUIElement, _ name: String) -> CGPoint? {
    guard let value = attribute(element, name), CFGetTypeID(value) == AXValueGetTypeID() else {
        return nil
    }
    var point = CGPoint.zero
    guard AXValueGetValue(value as! AXValue, .cgPoint, &point) else { return nil }
    return point
}

private func sizeAttribute(_ element: AXUIElement, _ name: String) -> CGSize? {
    guard let value = attribute(element, name), CFGetTypeID(value) == AXValueGetTypeID() else {
        return nil
    }
    var size = CGSize.zero
    guard AXValueGetValue(value as! AXValue, .cgSize, &size) else { return nil }
    return size
}

private struct AXEntry {
    let element: AXUIElement
    let attributes: [String: Any]
    let insideCloseDialog: Bool
}

private func accessibilityTree(_ application: AXUIElement) throws -> (entries: [AXEntry], truncated: Bool) {
    var entries: [AXEntry] = []
    var truncated = false

    func children(of element: AXUIElement, root: Bool) throws -> [AXUIElement] {
        var value: AnyObject?
        let result = AXUIElementCopyAttributeValue(element, kAXChildrenAttribute as CFString, &value)
        if result == .noValue || result == .attributeUnsupported { return [] }
        guard result == .success else {
            if root {
                throw error("AX_UNAVAILABLE", "cannot enumerate the Paint.R Accessibility children (\(result.rawValue))")
            }
            truncated = true
            return []
        }
        guard let children = value as? [AXUIElement] else {
            if root { throw error("AX_UNAVAILABLE", "Paint.R Accessibility children have an invalid type") }
            truncated = true
            return []
        }
        return children
    }

    func visit(_ element: AXUIElement, path: [Int], depth: Int, insideCloseDialog: Bool) throws {
        if entries.count >= maximumAXNodes || depth > maximumAXDepth {
            truncated = true
            return
        }
        var node: [String: Any] = ["path": path]
        if let role = stringAttribute(element, kAXRoleAttribute) { node["role"] = role }
        for (attributeName, field) in [
            (kAXTitleAttribute, "title"),
            (kAXDescriptionAttribute, "description"),
            (kAXValueAttribute, "value"),
        ] {
            if let value = stringAttribute(element, attributeName), !value.isEmpty {
                node[field] = value
            }
        }
        if let enabled = boolAttribute(element, kAXEnabledAttribute) { node["enabled"] = enabled }
        if let focused = boolAttribute(element, kAXFocusedAttribute) { node["focused"] = focused }
        let title = node["title"] as? String
        let inDialog = insideCloseDialog || title == closeTitle
        entries.append(AXEntry(element: element, attributes: node, insideCloseDialog: inDialog))
        let descendants = try children(of: element, root: false)
        if entries.count + descendants.count > maximumAXNodes { truncated = true }
        for (index, child) in descendants.enumerated() {
            if entries.count >= maximumAXNodes {
                truncated = true
                break
            }
            try visit(child, path: path + [index], depth: depth + 1, insideCloseDialog: inDialog)
        }
    }

    let roots = try children(of: application, root: true)
    if roots.count > maximumAXNodes { truncated = true }
    for (index, child) in roots.enumerated() {
        if entries.count >= maximumAXNodes {
            truncated = true
            break
        }
        try visit(child, path: [index], depth: 1, insideCloseDialog: false)
    }
    return (entries, truncated)
}

private func sameElement(_ first: AnyObject?, _ second: AXUIElement) -> Bool {
    guard let first = first else { return false }
    return CFGetTypeID(first) == AXUIElementGetTypeID() && CFEqual(first, second)
}

private struct AXWindowDescriptor {
    let element: AXUIElement
    let title: String?
    let position: CGPoint?
    let size: CGSize?
    let main: Bool
    let focused: Bool
}

private func accessibilityWindows(_ application: AXUIElement) throws -> [AXWindowDescriptor] {
    guard let value = attribute(application, kAXWindowsAttribute) else {
        throw error("AX_UNAVAILABLE", "cannot enumerate Paint.R Accessibility windows")
    }
    guard let windows = value as? [AXUIElement] else {
        throw error("AX_UNAVAILABLE", "Paint.R Accessibility windows have an invalid type")
    }
    let mainWindow = attribute(application, kAXMainWindowAttribute)
    let focusedWindow = attribute(application, kAXFocusedWindowAttribute)
    return windows.map { window in
        AXWindowDescriptor(
            element: window,
            title: stringAttribute(window, kAXTitleAttribute),
            position: pointAttribute(window, kAXPositionAttribute),
            size: sizeAttribute(window, kAXSizeAttribute),
            main: boolAttribute(window, kAXMainAttribute) ?? sameElement(mainWindow, window),
            focused: boolAttribute(window, kAXFocusedAttribute) ?? sameElement(focusedWindow, window)
        )
    }
}

private struct CGWindowDescriptor {
    let id: Int
    let pid: Int
    let title: String
    let onScreen: Bool
    let bounds: CGRect
}

private func coreGraphicsWindows(pid: pid_t, title: String) throws -> [CGWindowDescriptor] {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let rawWindows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        throw error("SCREEN_RECORDING_UNAVAILABLE", "CGWindowListCopyWindowInfo failed")
    }
    var result: [CGWindowDescriptor] = []
    for window in rawWindows {
        guard let ownerPID = number(window[kCGWindowOwnerPID as String]), Int(ownerPID) == Int(pid),
              let windowTitle = window[kCGWindowName as String] as? String, windowTitle == title,
              let identifier = number(window[kCGWindowNumber as String]), identifier > 0,
              let rawBounds = window[kCGWindowBounds as String] as? [String: Any],
              let x = number(rawBounds["X"]), let y = number(rawBounds["Y"]),
              let width = number(rawBounds["Width"]), let height = number(rawBounds["Height"]),
              width > 0, height > 0 else { continue }
        result.append(CGWindowDescriptor(
            id: Int(identifier),
            pid: Int(pid),
            title: windowTitle,
            onScreen: boolean(window[kCGWindowIsOnscreen as String]) ?? true,
            bounds: CGRect(x: x, y: y, width: width, height: height)
        ))
    }
    return result
}

private func frameMatches(_ ax: AXWindowDescriptor, _ cg: CGWindowDescriptor) -> Bool {
    guard ax.title == cg.title, let position = ax.position, let size = ax.size else { return false }
    return abs(position.x - cg.bounds.origin.x) <= 2
        && abs(position.y - cg.bounds.origin.y) <= 2
        && abs(size.width - cg.bounds.size.width) <= 2
        && abs(size.height - cg.bounds.size.height) <= 2
}

private func observe(pid: pid_t, title: String) throws -> [String: Any] {
    let running = processIsRunning(pid)
    let frontmost = NSRunningApplication(processIdentifier: pid)?.isActive ?? false
    guard running else {
        return [
            "schema_version": 1,
            "process": ["pid": Int(pid), "running": false, "frontmost": false],
            "windows": [],
            "ax": ["node_count": 0, "truncated": false, "nodes": []],
        ]
    }
    let application = AXUIElementCreateApplication(pid)
    let cgWindows = try coreGraphicsWindows(pid: pid, title: title)
    let axWindows = try accessibilityWindows(application)
    let tree = try accessibilityTree(application)
    var windows: [[String: Any]] = []
    for cg in cgWindows {
        var match = axWindows.first(where: { frameMatches($0, cg) })
        if match == nil && cgWindows.count == 1 {
            let titled = axWindows.filter { $0.title == title }
            if titled.count == 1 { match = titled[0] }
        }
        windows.append([
            "id": cg.id,
            "pid": cg.pid,
            "title": cg.title,
            "on_screen": cg.onScreen,
            "main": match?.main ?? false,
            "focused": match?.focused ?? false,
            "bounds": [
                "x": Double(cg.bounds.origin.x),
                "y": Double(cg.bounds.origin.y),
                "width": Double(cg.bounds.size.width),
                "height": Double(cg.bounds.size.height),
            ],
        ])
    }
    return [
        "schema_version": 1,
        "process": ["pid": Int(pid), "running": true, "frontmost": frontmost],
        "windows": windows,
        "ax": [
            "node_count": tree.entries.count,
            "truncated": tree.truncated,
            "nodes": tree.entries.map { $0.attributes },
        ],
    ]
}

private func focusTarget(pid: pid_t, title: String) throws {
    guard processIsRunning(pid), let runningApplication = NSRunningApplication(processIdentifier: pid) else {
        throw error("PROCESS_NOT_RUNNING", "Paint.R is not running as a native application")
    }
    let application = AXUIElementCreateApplication(pid)
    let targets = try accessibilityWindows(application).filter { $0.title == title }
    guard targets.count == 1 else {
        throw error("AMBIGUOUS_NATIVE_WINDOW", "expected exactly one Accessibility window titled \(title)")
    }
    guard runningApplication.activate(options: [.activateAllWindows]) else {
        throw error("WINDOW_FOCUS_FAILED", "could not activate Paint.R")
    }
    let raiseResult = AXUIElementPerformAction(targets[0].element, kAXRaiseAction as CFString)
    guard raiseResult == .success else {
        throw error("WINDOW_FOCUS_FAILED", "AXRaise failed (\(raiseResult.rawValue))")
    }
    let focusResult = AXUIElementSetAttributeValue(application, kAXFocusedWindowAttribute as CFString, targets[0].element)
    guard focusResult == .success else {
        throw error("WINDOW_FOCUS_FAILED", "could not focus the Paint.R window (\(focusResult.rawValue))")
    }
    usleep(200_000)
    guard processIsRunning(pid), runningApplication.isActive else {
        throw error("WINDOW_FOCUS_FAILED", "Paint.R did not become frontmost")
    }
    guard let focused = attribute(application, kAXFocusedWindowAttribute),
          sameElement(focused, targets[0].element) else {
        throw error("WINDOW_FOCUS_FAILED", "Paint.R did not retain native window focus")
    }
}

private func sendKey(pid: pid_t, title: String, name: String, modifierText: String) throws -> [String: Any] {
    let allowed = (name == "i" && modifierText == "command")
        || (name == "w" && modifierText == "command")
        || (name == "q" && modifierText == "command")
        || (name == "escape" && modifierText.isEmpty)
    guard allowed else { throw error("INPUT_REJECTED", "key is outside the native gate allowlist") }
    let keyCodes: [String: CGKeyCode] = ["i": 34, "w": 13, "q": 12, "escape": 53]
    guard let keyCode = keyCodes[name] else { throw error("INPUT_REJECTED", "unknown native key") }
    try focusTarget(pid: pid, title: title)
    try requirePreflight() // Recheck immediately before posting into the console session.

    let source = CGEventSource(stateID: .hidSystemState)
    let flags: CGEventFlags = modifierText == "command" ? [.maskCommand] : []
    guard let down = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true),
          let up = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: false) else {
        throw error("CG_EVENT_FAILED", "could not create keyboard events")
    }
    down.flags = flags
    up.flags = flags
    var commandDown: CGEvent?
    var commandUp: CGEvent?
    if modifierText == "command" {
        commandDown = CGEvent(keyboardEventSource: source, virtualKey: 55, keyDown: true)
        commandUp = CGEvent(keyboardEventSource: source, virtualKey: 55, keyDown: false)
        guard commandDown != nil, commandUp != nil else {
            throw error("CG_EVENT_FAILED", "could not create Command modifier events")
        }
        commandDown?.flags = flags
    }
    commandDown?.post(tap: .cgSessionEventTap)
    if commandDown != nil { usleep(40_000) }
    down.post(tap: .cgSessionEventTap)
    usleep(80_000)
    up.post(tap: .cgSessionEventTap)
    usleep(40_000)
    commandUp?.post(tap: .cgSessionEventTap)
    usleep(100_000)
    return [
        "event": "CGEvent keyboard down/up posted",
        "pid": Int(pid),
        "key": name,
        "modifiers": modifierText.isEmpty ? [] : [modifierText],
    ]
}

private func entryText(_ entry: AXEntry) -> [String] {
    ["title", "description", "value"].compactMap { entry.attributes[$0] as? String }
}

private func pressDiscard(pid: pid_t, title: String) throws -> [String: Any] {
    try focusTarget(pid: pid, title: title)
    let application = AXUIElementCreateApplication(pid)
    let tree = try accessibilityTree(application)
    guard !tree.truncated else { throw error("INCONCLUSIVE_AX", "Accessibility tree is truncated") }
    let dialogRoots = tree.entries.filter { $0.attributes["title"] as? String == closeTitle }
    guard dialogRoots.count == 1 else {
        throw error("AX_ACTION_FAILED", "expected exactly one dirty-close confirmation")
    }
    let dialog = tree.entries.filter { $0.insideCloseDialog }
    guard dialog.contains(where: { entryText($0).contains(closeExplanation) }) else {
        throw error("AX_ACTION_FAILED", "dirty-close explanation is absent")
    }
    let buttons = dialog.filter { $0.attributes["role"] as? String == kAXButtonRole }
    let cancel = buttons.filter { $0.attributes["title"] as? String == "Cancel" }
    let discard = buttons.filter { $0.attributes["title"] as? String == "Discard" }
    guard cancel.count == 1, discard.count == 1,
          boolAttribute(cancel[0].element, kAXEnabledAttribute) == true,
          boolAttribute(cancel[0].element, kAXFocusedAttribute) == true,
          boolAttribute(discard[0].element, kAXEnabledAttribute) == true else {
        throw error("AX_ACTION_FAILED", "enabled Cancel/Discard with focused Cancel are not unambiguous")
    }
    try requirePreflight() // Do not activate a destructive control if the session changed.
    let result = AXUIElementPerformAction(discard[0].element, kAXPressAction as CFString)
    guard result == .success else {
        throw error("AX_ACTION_FAILED", "AXPress on Discard failed (\(result.rawValue))")
    }
    return ["event": "AXPress", "pid": Int(pid), "title": "Discard"]
}

private func run() throws {
    let arguments = CommandLine.arguments
    guard arguments.count >= 2 else {
        throw error("USAGE", "usage: macos-native-ui preflight|observe|key|press ...")
    }
    switch arguments[1] {
    case "preflight":
        guard arguments.count == 2 else { throw error("USAGE", "preflight takes no arguments") }
        try emitJSON(nativePreflight())
    case "observe":
        guard arguments.count == 4 else { throw error("USAGE", "observe <pid> <window-title>") }
        try requirePreflight()
        let pid = try processID(arguments[2])
        try emitJSON(observe(pid: pid, title: arguments[3]))
    case "key":
        guard arguments.count == 6 else { throw error("USAGE", "key <pid> <window-title> <name> <modifiers>") }
        try requirePreflight()
        let pid = try processID(arguments[2])
        try emitJSON(sendKey(pid: pid, title: arguments[3], name: arguments[4], modifierText: arguments[5]))
    case "press":
        guard arguments.count == 6, arguments[5] == "Discard" else {
            throw error("USAGE", "press <pid> <window-title> Discard")
        }
        try requirePreflight()
        let pid = try processID(arguments[2])
        try emitJSON(pressDiscard(pid: pid, title: arguments[3]))
    default:
        throw error("USAGE", "unknown native command \(arguments[1])")
    }
}

do {
    try run()
} catch let nativeError as NativeError {
    let message = nativeError.message.replacingOccurrences(of: "\n", with: " ")
    FileHandle.standardError.write(Data("native-ui-error: \(nativeError.code): \(message)\n".utf8))
    exit(2)
} catch {
    let message = String(describing: error).replacingOccurrences(of: "\n", with: " ")
    FileHandle.standardError.write(Data("native-ui-error: NATIVE_HELPER_FAILED: \(message)\n".utf8))
    exit(2)
}
