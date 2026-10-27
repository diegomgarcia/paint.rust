import AppKit
import ApplicationServices
import CoreGraphics
import Foundation

struct ToolError: Error, CustomStringConvertible {
    let description: String
    init(_ message: String) { description = message }
}

struct WindowInfo: Codable {
    let id: Int
    let owner: String
    let pid: Int
    let name: String
    let x: Double
    let y: Double
    let width: Double
    let height: Double
}

func number(_ value: Any?) -> Double? {
    if let number = value as? NSNumber { return number.doubleValue }
    if let number = value as? Double { return number }
    if let number = value as? Int { return Double(number) }
    return nil
}

func vmWindows() throws -> [WindowInfo] {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let rawWindows = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        throw ToolError("CGWindowListCopyWindowInfo failed; check Screen Recording permission")
    }
    var result: [WindowInfo] = []
    for window in rawWindows {
        let owner = window[kCGWindowOwnerName as String] as? String ?? ""
        let name = window[kCGWindowName as String] as? String ?? ""
        guard owner == "UTM", name == "Windows" else { continue }
        guard let idNumber = window[kCGWindowNumber as String] as? NSNumber,
              let pidNumber = window[kCGWindowOwnerPID as String] as? NSNumber,
              let bounds = window[kCGWindowBounds as String] as? [String: Any],
              let x = number(bounds["X"]), let y = number(bounds["Y"]),
              let width = number(bounds["Width"]), let height = number(bounds["Height"]),
              width > 0, height > 0 else { continue }
        result.append(WindowInfo(
            id: idNumber.intValue, owner: owner, pid: pidNumber.intValue, name: name,
            x: x, y: y, width: width, height: height
        ))
    }
    if result.isEmpty { throw ToolError("No visible UTM window named Windows (minimum 640x480)") }
    return result
}

func printJSON<T: Encodable>(_ value: T) throws {
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
    let data = try encoder.encode(value)
    guard let text = String(data: data, encoding: .utf8) else { throw ToolError("JSON encoding failed") }
    print(text)
}

func stringAttribute(_ element: AXUIElement, _ attribute: String) -> String? {
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(element, attribute as CFString, &value) == .success else { return nil }
    return value as? String
}

func focusVM() throws -> WindowInfo {
    guard AXIsProcessTrusted() else { throw ToolError("Accessibility permission is not granted") }
    guard let window = try vmWindows().first else { throw ToolError("Windows VM window not found") }
    guard let application = NSRunningApplication(processIdentifier: pid_t(window.pid)) else {
        throw ToolError("UTM process not found")
    }
    guard application.activate(options: [.activateAllWindows]) else { throw ToolError("Could not activate UTM") }
    usleep(300_000)
    guard let window = try vmWindows().first(where: { $0.width >= 640 && $0.height >= 480 }) else {
        throw ToolError("UTM Windows window remained smaller than 640x480 after activation")
    }
    let axApplication = AXUIElementCreateApplication(pid_t(window.pid))
    var value: AnyObject?
    guard AXUIElementCopyAttributeValue(axApplication, kAXWindowsAttribute as CFString, &value) == .success,
          let windows = value as? [AXUIElement] else {
        throw ToolError("Could not enumerate UTM accessibility windows")
    }
    var target: AXUIElement?
    for candidate in windows where stringAttribute(candidate, kAXTitleAttribute) == "Windows" {
        target = candidate
        break
    }
    guard let target else { throw ToolError("Could not find accessibility window titled Windows") }
    let raiseResult = AXUIElementPerformAction(target, kAXRaiseAction as CFString)
    if raiseResult != .success { throw ToolError("AXRaise failed with \(raiseResult.rawValue)") }
    _ = AXUIElementSetAttributeValue(axApplication, kAXFocusedWindowAttribute as CFString, target)
    usleep(300_000)
    var focusedValue: AnyObject?
    guard AXUIElementCopyAttributeValue(axApplication, kAXFocusedWindowAttribute as CFString, &focusedValue) == .success,
          let focused = focusedValue else {
        throw ToolError("Could not verify focused UTM window")
    }
    let focusedElement = focused as! AXUIElement
    guard stringAttribute(focusedElement, kAXTitleAttribute) == "Windows" else {
        throw ToolError("UTM Windows window is not focused")
    }
    return window
}

let keyCodes: [String: CGKeyCode] = [
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7,
    "c": 8, "v": 9, "b": 11, "q": 12, "w": 13, "e": 14, "r": 15,
    "y": 16, "t": 17, "o": 31, "u": 32, "i": 34, "p": 35, "l": 37,
    "j": 38, "k": 40, "n": 45, "m": 46, "return": 36, "enter": 36,
    "tab": 48, "space": 49, "delete": 51, "backspace": 51, "escape": 53,
    "home": 115, "end": 119, "left": 123, "right": 124, "down": 125, "up": 126,
]

func modifiers(_ text: String) throws -> CGEventFlags {
    var flags: CGEventFlags = []
    if text.isEmpty { return flags }
    for modifier in text.split(separator: ",").map({ $0.lowercased() }) {
        switch modifier {
        case "control", "ctrl": flags.insert(.maskControl)
        case "shift": flags.insert(.maskShift)
        case "alt", "option": flags.insert(.maskAlternate)
        case "command", "cmd": flags.insert(.maskCommand)
        default: throw ToolError("Unknown modifier \(modifier)")
        }
    }
    return flags
}

func sendKey(_ name: String, _ modifierText: String) throws {
    guard let code = keyCodes[name.lowercased()] else { throw ToolError("Unknown key \(name)") }
    let flags = try modifiers(modifierText)
    _ = try focusVM()
    let source = CGEventSource(stateID: .hidSystemState)
    guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
          let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else {
        throw ToolError("Could not create keyboard events")
    }
    let modifierKeys: [(CGKeyCode, CGEventFlags)] = [
        (59, .maskControl), (56, .maskShift), (58, .maskAlternate), (55, .maskCommand),
    ]
    var pressedModifiers: [CGKeyCode] = []
    for (modifierCode, modifierFlag) in modifierKeys where flags.contains(modifierFlag) {
        guard let modifierDown = CGEvent(keyboardEventSource: source, virtualKey: modifierCode, keyDown: true) else {
            throw ToolError("Could not create modifier event")
        }
        modifierDown.flags = flags
        modifierDown.post(tap: .cgSessionEventTap)
        pressedModifiers.append(modifierCode)
        usleep(40_000)
    }
    down.flags = flags
    up.flags = flags
    down.post(tap: .cgSessionEventTap)
    usleep(80_000)
    up.post(tap: .cgSessionEventTap)
    usleep(40_000)
    for modifierCode in pressedModifiers.reversed() {
        guard let modifierUp = CGEvent(keyboardEventSource: source, virtualKey: modifierCode, keyDown: false) else {
            throw ToolError("Could not release modifier event")
        }
        modifierUp.post(tap: .cgSessionEventTap)
        usleep(30_000)
    }
    usleep(150_000)
}

func typeText(_ text: String) throws {
    if text.isEmpty { throw ToolError("Text must be nonempty") }
    let digitCodes: [Character: CGKeyCode] = [
        "1": 18, "2": 19, "3": 20, "4": 21, "5": 23,
        "6": 22, "7": 26, "8": 28, "9": 25, "0": 29,
    ]
    let symbolCodes: [Character: (CGKeyCode, Bool)] = [
        "\\": (42, false), "/": (44, false), ".": (47, false), ",": (43, false),
        "-": (27, false), "_": (27, true), "=": (24, false), "+": (24, true),
        ";": (41, false), ":": (41, true), "'": (39, false), "\"": (39, true),
        "[": (33, false), "]": (30, false), "{": (33, true), "}": (30, true),
        "`": (50, false), "~": (50, true), " ": (49, false),
        "!": (18, true), "@": (19, true), "#": (20, true), "$": (21, true),
        "%": (23, true), "^": (22, true), "&": (26, true), "*": (28, true),
        "(": (25, true), ")": (29, true),
    ]
    var strokes: [(CGKeyCode, Bool)] = []
    for character in text {
        if character >= "a" && character <= "z", let code = keyCodes[String(character)] {
            strokes.append((code, false))
        } else if character >= "A" && character <= "Z", let code = keyCodes[String(character).lowercased()] {
            strokes.append((code, true))
        } else if let code = digitCodes[character] {
            strokes.append((code, false))
        } else if let stroke = symbolCodes[character] {
            strokes.append(stroke)
        } else {
            throw ToolError("Unsupported US-keyboard ASCII character \(character)")
        }
    }
    _ = try focusVM()
    let source = CGEventSource(stateID: .hidSystemState)
    for (code, shifted) in strokes {
        let flags: CGEventFlags = shifted ? [.maskShift] : []
        if shifted {
            guard let shiftDown = CGEvent(keyboardEventSource: source, virtualKey: 56, keyDown: true) else {
                throw ToolError("Could not create Shift event")
            }
            shiftDown.flags = flags
            shiftDown.post(tap: .cgSessionEventTap)
            usleep(25_000)
        }
        guard let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else {
            throw ToolError("Could not create keyboard events")
        }
        down.flags = flags
        up.flags = flags
        down.post(tap: .cgSessionEventTap)
        usleep(25_000)
        up.post(tap: .cgSessionEventTap)
        if shifted {
            guard let shiftUp = CGEvent(keyboardEventSource: source, virtualKey: 56, keyDown: false) else {
                throw ToolError("Could not release Shift event")
            }
            shiftUp.post(tap: .cgSessionEventTap)
        }
        usleep(30_000)
    }
}

func click(_ x: Double, _ y: Double) throws {
    _ = try focusVM()
    let point = CGPoint(x: x, y: y)
    let source = CGEventSource(stateID: .hidSystemState)
    guard let move = CGEvent(mouseEventSource: source, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: .left),
          let down = CGEvent(mouseEventSource: source, mouseType: .leftMouseDown, mouseCursorPosition: point, mouseButton: .left),
          let up = CGEvent(mouseEventSource: source, mouseType: .leftMouseUp, mouseCursorPosition: point, mouseButton: .left) else {
        throw ToolError("Could not create mouse events")
    }
    move.post(tap: .cgSessionEventTap)
    usleep(150_000)
    down.post(tap: .cgSessionEventTap)
    usleep(80_000)
    up.post(tap: .cgSessionEventTap)
}

func run() throws {
    let arguments = CommandLine.arguments
    guard arguments.count >= 2 else { throw ToolError("Usage: host-window list|focus|key|type|click ...") }
    switch arguments[1] {
    case "list": try printJSON(vmWindows())
    case "focus": try printJSON(focusVM())
    case "key":
        guard arguments.count == 3 || arguments.count == 4 else { throw ToolError("Usage: host-window key <name> [control,shift]") }
        try sendKey(arguments[2], arguments.count == 4 ? arguments[3] : "")
    case "type":
        guard arguments.count == 3 else { throw ToolError("Usage: host-window type <text>") }
        try typeText(arguments[2])
    case "click":
        guard arguments.count == 4, let x = Double(arguments[2]), let y = Double(arguments[3]) else {
            throw ToolError("Usage: host-window click <global-x> <global-y>")
        }
        try click(x, y)
    default: throw ToolError("Unknown command \(arguments[1])")
    }
}

do {
    try run()
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(2)
}
