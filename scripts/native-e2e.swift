import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import CryptoKit
import Foundation

// External native behavioral checks. No in-app hooks or permission prompts.
struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

struct AXReadFailure: Error, CustomStringConvertible {
    let element: AXUIElement
    let name: String
    let status: AXError
    var description: String { "AX \(name): \(status.rawValue)" }
}

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw Failure(message) }
}

func waitFor(_ message: String, _ predicate: () throws -> Bool) throws {
    let deadline = DispatchTime.now().uptimeNanoseconds + 5_000_000_000
    while try !predicate() {
        if DispatchTime.now().uptimeNanoseconds >= deadline {
            throw Failure("Timed out: \(message)")
        }
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
    }
}

func attribute(_ element: AXUIElement, _ name: String) throws -> CFTypeRef {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, name as CFString, &value)
    guard result == .success else {
        throw AXReadFailure(element: element, name: name, status: result)
    }
    guard let value else { throw Failure("AX \(name) has no value after success") }
    return value
}

func children(_ element: AXUIElement) throws -> [AXUIElement] {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, "AXChildren" as CFString, &value)
    if result == .noValue || result == .attributeUnsupported { return [] }
    guard result == .success else {
        throw AXReadFailure(element: element, name: "children", status: result)
    }
    guard let result = value as? [AXUIElement] else {
        throw Failure("AXChildren is not an array")
    }
    return result
}

func text(_ element: AXUIElement, _ name: String) throws -> String {
    guard let value = try attribute(element, name) as? String else {
        throw Failure("AX \(name) is not text")
    }
    return value
}

func findMenu(_ element: AXUIElement, title: String, depth: Int = 5) throws -> AXUIElement? {
    if try text(element, "AXRole") == "AXMenuItem",
       try text(element, "AXTitle") == title { return element }
    guard depth > 0 else { return nil }
    for child in try children(element) {
        if let item = try findMenu(child, title: title, depth: depth - 1) { return item }
    }
    return nil
}

func press(_ element: AXUIElement) throws {
    let result = AXUIElementPerformAction(element, "AXPress" as CFString)
    try require(result == .success, "AXPress failed: \(result.rawValue)")
}

func windowSize(_ window: AXUIElement) throws -> CGSize {
    let raw = try attribute(window, "AXSize")
    try require(CFGetTypeID(raw) == AXValueGetTypeID(), "AXSize is not AXValue")
    let value = raw as! AXValue
    var size = CGSize.zero
    try require(AXValueGetType(value) == .cgSize && AXValueGetValue(value, .cgSize, &size),
                "AXSize is not CGSize")
    try require(size.width.isFinite && size.height.isFinite && size.width > 0 && size.height > 0,
                "Invalid native window size")
    return size
}

final class Candidate {
    let process = Process()
    let app: AXUIElement
    let pid: pid_t
    let errorsURL: URL
    private let errors: FileHandle

    init(executable: URL, artifacts: URL) throws {
        errorsURL = artifacts.appendingPathComponent("candidate-\(UUID().uuidString).log")
        let fd = open(errorsURL.path, O_RDWR | O_CREAT | O_EXCL, 0o600)
        guard fd >= 0 else { throw Failure("Cannot create private candidate log: errno=\(errno)") }
        errors = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        process.executableURL = executable
        process.standardOutput = errors
        process.standardError = errors
        try process.run()
        pid = process.processIdentifier
        app = AXUIElementCreateApplication(pid)
    }

    deinit {
        do { try errors.close() } catch { fputs("Cleanup log close: \(error)\n", stderr) }
        // Retain private diagnostics as run evidence, including on failure.
    }

    func diagnostics() throws -> String {
        String(decoding: try Data(contentsOf: errorsURL), as: UTF8.self)
    }

    func alive() -> Bool { process.isRunning && kill(pid, 0) == 0 }

    func waitForStartup() throws {
        try waitFor("native AX server and initial scene for pid=\(pid)") {
            try require(alive(), "Candidate exited during startup")
            var value: CFTypeRef?
            let result = AXUIElementCopyAttributeValue(app, "AXWindows" as CFString, &value)
            // Before NSApplication installs its AX server, messaging cannot complete.
            if result == .cannotComplete { return false }
            guard result == .success, let windows = value as? [AXUIElement] else {
                throw Failure("Startup AXWindows: \(result.rawValue)")
            }
            return windows.count == 1
        }
    }

    func windows() throws -> [AXUIElement] {
        try require(alive(), "Candidate exited before the expected Quit: pid=\(pid)")
        guard let result = try attribute(app, "AXWindows") as? [AXUIElement] else {
            throw Failure("AXWindows is not an array")
        }
        return result
    }

    func expectWindows(_ count: Int) throws {
        try waitFor("\(count) native windows for pid=\(pid)") { try windows().count == count }
    }

    func settings() throws -> AXUIElement {
        for window in try windows() {
            if try text(window, "AXTitle") == "設定" { return window }
        }
        throw Failure("Settings window missing")
    }

    func scene() throws -> AXUIElement {
        for window in try windows() {
            if try text(window, "AXTitle") != "設定" { return window }
        }
        throw Failure("Scene window missing")
    }

    func menu(_ title: String) throws {
        let raw = try attribute(app, "AXMenuBar")
        try require(CFGetTypeID(raw) == AXUIElementGetTypeID(), "AXMenuBar is not an element")
        guard let item = try findMenu(raw as! AXUIElement, title: title) else {
            throw Failure("Candidate menu missing: \(title)")
        }
        try requireSession()
        try press(item)
    }

    func shortcut(_ keyCode: CGKeyCode, in window: AXUIElement) throws {
        try require(alive(), "Candidate exited before shortcut")
        try require(AXUIElementPerformAction(window, "AXRaise" as CFString) == .success,
                    "Cannot raise target window")
        try require(AXUIElementSetAttributeValue(window, "AXMain" as CFString, kCFBooleanTrue) == .success,
                    "Cannot select target window")
        guard let running = NSRunningApplication(processIdentifier: pid) else {
            throw Failure("Candidate application missing")
        }
        try require(running.activate(options: []), "Cannot activate candidate")
        try waitFor("candidate owns foreground and target receives keyboard input") {
            guard NSWorkspace.shared.frontmostApplication?.processIdentifier == self.pid else { return false }
            let focused = try attribute(app, "AXFocusedWindow")
            return CFGetTypeID(focused) == AXUIElementGetTypeID() && CFEqual(focused, window)
        }
        guard let source = CGEventSource(stateID: .combinedSessionState),
              let down = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: true),
              let up = CGEvent(keyboardEventSource: source, virtualKey: keyCode, keyDown: false) else {
            throw Failure("Cannot create keyboard events")
        }
        try requireSession()
        down.flags = .maskCommand
        up.flags = .maskCommand
        // Never post a global chord while another app owns the foreground.
        let focused = try attribute(app, "AXFocusedWindow")
        try require(NSWorkspace.shared.frontmostApplication?.processIdentifier == pid &&
                    CFGetTypeID(focused) == AXUIElementGetTypeID() && CFEqual(focused, window),
                    "Foreground or keyboard target changed before input; no chord posted")
        down.post(tap: .cghidEventTap)
        up.post(tap: .cghidEventTap)
    }

    func expectExit() throws {
        try waitFor("actual pid \(pid) disappears") { kill(pid, 0) == -1 && errno == ESRCH }
        try waitFor("owned process completes termination") { !process.isRunning }
        process.waitUntilExit()
        let log = try diagnostics()
        try require(process.terminationReason == .exit && process.terminationStatus == 0,
                    "Candidate abnormal exit: \(process.terminationStatus)\n\(log)")
        try require(!log.contains("Ambien:") && !log.contains("panicked"), "Candidate errors:\n\(log)")
    }

    func cleanupAfterFailure() throws {
        // Cleanup never counts as successful Quit. Only clean the owned Process.
        if process.isRunning { process.terminate() }
        do {
            try waitFor("owned candidate cleanup pid=\(pid)") { !process.isRunning }
        } catch {
            if process.isRunning {
                try require(kill(pid, SIGKILL) == 0, "Owned SIGKILL cleanup failed: errno=\(errno)")
            }
            try waitFor("owned candidate force cleanup pid=\(pid)") { !process.isRunning }
        }
        process.waitUntilExit()
    }
}

func minimizeRestore(_ window: AXUIElement) throws {
    let button = try attribute(window, "AXMinimizeButton")
    try require(CFGetTypeID(button) == AXUIElementGetTypeID(), "Minimize button missing")
    try press(button as! AXUIElement)
    try waitFor("window minimizes") { (try attribute(window, "AXMinimized") as? Bool) == true }
    let result = AXUIElementSetAttributeValue(window, "AXMinimized" as CFString, kCFBooleanFalse)
    try require(result == .success, "Cannot restore minimized native window")
    try waitFor("window restores") { (try attribute(window, "AXMinimized") as? Bool) == false }
}
func withCandidate(_ executable: URL, artifacts: URL, _ scenario: (Candidate) throws -> Void) throws {
    try requireSession()
    let candidate = try Candidate(executable: executable, artifacts: artifacts)
    do {
        try require(AXUIElementSetMessagingTimeout(candidate.app, 1) == .success,
                    "Cannot bound candidate AX messages")
        try candidate.waitForStartup()
        try scenario(candidate)
    } catch {
        var cleanupError: Error?
        do { try candidate.cleanupAfterFailure() } catch { cleanupError = error }
        // Preserve the original scenario failure even if diagnostic collection fails.
        do {
            let log = try candidate.diagnostics()
            if !log.isEmpty { fputs("Candidate stderr pid=\(candidate.pid):\n\(log)", stderr) }
        } catch { fputs("Cannot read candidate stderr: \(error)\n", stderr) }
        if let cleanupError {
            throw Failure("owned pid=\(candidate.pid); \(error); cleanup also failed: \(cleanupError)")
        }
        if let blocked = error as? Blocked {
            throw Blocked("owned pid=\(candidate.pid); \(blocked); failure cleanup is not Quit proof")
        }
        throw Failure("owned pid=\(candidate.pid); \(error); failure cleanup is not Quit proof")
    }
}

func lifecycle(_ executable: URL, artifacts: URL) throws -> String {
    var pids: [pid_t] = []
    try withCandidate(executable, artifacts: artifacts) { candidate in
        pids.append(candidate.pid)
        let initialScene = try candidate.scene()
        let sceneSize = try windowSize(initialScene)
        try candidate.menu("設定…")
        try candidate.expectWindows(2)
        let settings = try candidate.settings()
        let settingsSize = try windowSize(settings)
        try candidate.menu("設定…")
        try candidate.expectWindows(2)
        try require(CFEqual(settings, try candidate.settings()), "Repeated settings created another window")
        print("PASS settings real menu / same native window")
        for window in [initialScene, settings] {
            let size = try windowSize(window)
            try minimizeRestore(window)
            try require(try windowSize(window) == size, "Minimize/restore changed frame size")
            let zoom = try attribute(window, "AXZoomButton")
            try require(CFGetTypeID(zoom) == AXUIElementGetTypeID(), "Zoom button missing")
            try require((try attribute(zoom as! AXUIElement, "AXEnabled") as? Bool) == false,
                        "Native zoom is enabled")
        }
        print("PASS both windows native minimize / AX restore / unchanged frame / disabled zoom")
        let settingsClose = try attribute(settings, "AXCloseButton") as! AXUIElement
        try press(settingsClose)
        try candidate.expectWindows(1)
        print("PASS settings native close / app survives")
        try candidate.shortcut(43, in: initialScene) // ANSI comma; actual OS foreground input.
        try candidate.expectWindows(2)
        try require(try windowSize(candidate.settings()) == settingsSize, "Shortcut reopened settings at another size")
        print("PASS foreground cmd-comma / settings recreation")
        try press(try attribute(candidate.settings(), "AXCloseButton") as! AXUIElement)
        try candidate.expectWindows(1)
        try press(try attribute(initialScene, "AXCloseButton") as! AXUIElement)
        try candidate.expectWindows(0)
        try candidate.menu("シーンを表示")
        try candidate.expectWindows(1)
        let reopenedScene = try candidate.scene()
        try require(!CFEqual(initialScene, reopenedScene), "Scene did not get a new native window")
        try require(try windowSize(reopenedScene) == sceneSize, "Scene recreation changed frame size")
        print("PASS scene native close / zero-window host / menu recreation")
        try candidate.menu("終了")
        try candidate.expectExit()
        print("PASS real menu Quit / actual pid disappeared / exit0 pid=\(candidate.pid)")
    }
    for settingsFocused in [false, true] {
        try withCandidate(executable, artifacts: artifacts) { candidate in
            pids.append(candidate.pid)
            if settingsFocused {
                try candidate.menu("設定…")
                try candidate.expectWindows(2)
            }
            let target = try settingsFocused ? candidate.settings() : candidate.scene()
            try candidate.shortcut(12, in: target) // ANSI Q; not in-app dispatch or background postToPid.
            try candidate.expectExit()
            print("PASS foreground cmd-Q / \(settingsFocused ? "settings" : "scene") / actual pid disappeared / exit0 pid=\(candidate.pid)")
        }
    }
    return "Native menu, singleton settings, both minimize/restore and disabled zoom, close/recreate sizes, zero-window survival, menu Quit and foreground Cmd-Q from both windows; owned PIDs \(pids) disappeared with normal exit0."
}

struct Blocked: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
}

// Missing lock metadata is not unlocked proof: require corroborating live console,
// completed login, matching UID, awake/active display, and a non-loginwindow foreground.
func requireSession() throws {
    guard let session = CGSessionCopyCurrentDictionary() as? [String: Any] else {
        throw Blocked("No native console session metadata; no launch/input")
    }
    let locked = (session["CGSSessionScreenIsLocked"] as? NSNumber)?.boolValue
    let console = (session[kCGSessionOnConsoleKey as String] as? NSNumber)?.boolValue
    let login = (session[kCGSessionLoginDoneKey as String] as? NSNumber)?.boolValue
    let uid = (session[kCGSessionUserIDKey as String] as? NSNumber)?.uint32Value
    let display = CGMainDisplayID()
    guard locked != true, console == true, login == true, uid == geteuid(),
          CGDisplayIsActive(display) != 0, CGDisplayIsAsleep(display) == 0,
          let foreground = NSWorkspace.shared.frontmostApplication,
          foreground.bundleIdentifier != "com.apple.loginwindow" else {
        throw Blocked("Unsafe/unconfirmed session: lock=\(String(describing: locked)), console=\(String(describing: console)), login=\(String(describing: login)), uid=\(String(describing: uid)), active=\(CGDisplayIsActive(display)), asleep=\(CGDisplayIsAsleep(display)); no launch/input")
    }
    guard AXIsProcessTrusted(), CGPreflightPostEventAccess() else {
        throw Blocked("Accessibility/event posting unavailable; no prompt requested")
    }
}

func optional(_ node: AXUIElement, _ name: String) throws -> CFTypeRef? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(node, name as CFString, &value)
    if result == .noValue || result == .attributeUnsupported { return nil }
    guard result == .success else {
        throw AXReadFailure(element: node, name: name, status: result)
    }
    return value
}

func ownNodes(_ node: AXUIElement) throws -> [AXUIElement] {
    var result: [AXUIElement] = []
    func visit(_ item: AXUIElement, depth: Int) throws {
        try require(depth < 16 && result.count < 4096, "Own AX traversal exceeds bounded tree")
        result.append(item)
        let items = try children(item)
        try require(items.count <= 256, "Own AX sibling bound exceeded")
        for child in items { try visit(child, depth: depth + 1) }
    }
    try visit(node, depth: 0)
    return result
}

func named(_ window: AXUIElement, role: String, name: String) throws -> AXUIElement? {
    for node in try ownNodes(window) where try text(node, "AXRole") == role {
        for key in ["AXTitle", "AXDescription", "AXValue"] {
            if let value = try optional(node, key) as? String, value == name { return node }
        }
    }
    return nil
}

func segment(_ window: AXUIElement, label: String) throws -> AXUIElement {
    guard let group = try named(window, role: "AXGroup", name: "シチュエーション"),
          let labelNode = try named(group, role: "AXRadioButton", name: label) else {
        throw Failure("Owned selector label missing: \(label)")
    }
    return labelNode
}

func line(_ window: AXUIElement, prefix: String) throws -> String? {
    for node in try ownNodes(window) where try text(node, "AXRole") == "AXStaticText" {
        for key in ["AXTitle", "AXDescription", "AXValue"] {
            if let value = try optional(node, key) as? String, value.hasPrefix(prefix) { return value }
        }
    }
    return nil
}

func generations(_ value: String) throws -> (Int, Int) {
    let expression = try NSRegularExpression(pattern: "設定世代 ([0-9]+)・手動世代 ([0-9]+)")
    let range = NSRange(value.startIndex..<value.endIndex, in: value)
    guard let match = expression.firstMatch(in: value, range: range),
          let r1 = Range(match.range(at: 1), in: value), let r2 = Range(match.range(at: 2), in: value),
          let settings = Int(value[r1]), let manual = Int(value[r2]) else {
        throw Failure("Generation text missing: \(value)")
    }
    return (settings, manual)
}

enum ReceiptObservation {
    case waiting, exact, invalidated
}

func expectReceipt(_ candidate: Candidate, _ window: AXUIElement, settings: Int, manual: Int) throws {
    func requireSameWindow() throws {
        let current = try candidate.settings()
        try require(CFEqual(current, window), "Owned settings window changed during receipt observation")
    }
    func observe() throws -> ReceiptObservation {
        try requireSameWindow()
        do {
            guard let main = try line(window, prefix: "現在の設定:"),
                  let receipt = try line(window, prefix: "音声側の受理:") else { return .waiting }
            let m = try generations(main), r = try generations(receipt)
            try require(m.0 <= settings && m.1 <= manual && r.0 <= settings && r.1 <= manual,
                        "Unexpected/duplicate revision: main=\(main), receipt=\(receipt), expected=\(settings)/\(manual)")
            return m.0 == settings && m.1 == manual && r.0 == settings && r.1 == manual &&
                !receipt.contains("受理待ち") ? .exact : .waiting
        } catch let failure as AXReadFailure
            where failure.status == .invalidUIElement && !CFEqual(failure.element, window) {
            // AccessKit destroys removed virtual descendants while this non-atomic
            // walk is in flight. Discard the whole observation, never a required field.
            var pid: pid_t = 0
            try require(AXUIElementGetPid(failure.element, &pid) == .success && pid == candidate.pid,
                        "Invalidated receipt descendant does not belong to the candidate")
            try requireSameWindow()
            print("OBSERVE invalidated owned AX descendant (\(failure.name)); discard receipt snapshot \(settings)/\(manual)")
            return .invalidated
        }
    }
    try waitFor("main endpoints and actual audio receipt \(settings)/\(manual)") {
        try observe() == .exact
    }
    // A late duplicate remains a failure; only invalidated observations may be
    // replaced within the existing bounded wait, not mismatching stable values.
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.15))
    try waitFor("stable exact receipt \(settings)/\(manual)") {
        switch try observe() {
        case .exact: return true
        case .invalidated: return false
        case .waiting: throw Failure("Expected receipt did not remain exact \(settings)/\(manual)")
        }
    }
}

func isForeground(_ candidate: Candidate, _ window: AXUIElement) throws -> Bool {
    guard candidate.alive(), NSWorkspace.shared.frontmostApplication?.processIdentifier == candidate.pid else {
        return false
    }
    let focus = try attribute(candidate.app, "AXFocusedWindow")
    return CFGetTypeID(focus) == AXUIElementGetTypeID() && CFEqual(focus, window)
}

func foreground(_ candidate: Candidate, _ window: AXUIElement) throws {
    try requireSession()
    try require(AXUIElementPerformAction(window, "AXRaise" as CFString) == .success, "Cannot raise own window")
    try require(AXUIElementSetAttributeValue(window, "AXMain" as CFString, kCFBooleanTrue) == .success,
                "Cannot select own window")
    guard let app = NSRunningApplication(processIdentifier: candidate.pid) else { throw Failure("Own app missing") }
    try require(app.activate(options: []), "Cannot activate own app")
    try waitFor("own foreground and keyboard window") { try isForeground(candidate, window) }
}

func key(_ candidate: Candidate, _ window: AXUIElement, code: CGKeyCode) throws {
    guard let source = CGEventSource(stateID: .combinedSessionState),
          let down = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: true),
          let up = CGEvent(keyboardEventSource: source, virtualKey: code, keyDown: false) else {
        throw Failure("Cannot create own key")
    }
    down.flags = []; up.flags = []
    try requireSession()
    try require(try isForeground(candidate, window), "Foreground changed; no key posted")
    down.post(tap: .cghidEventTap); up.post(tap: .cghidEventTap)
}

func position(_ node: AXUIElement) throws -> CGPoint {
    let raw = try attribute(node, "AXPosition")
    try require(CFGetTypeID(raw) == AXValueGetTypeID(), "Position is not AXValue")
    let value = raw as! AXValue
    var point = CGPoint.zero
    try require(AXValueGetType(value) == .cgPoint && AXValueGetValue(value, .cgPoint, &point) &&
                point.x.isFinite && point.y.isFinite, "Position is not finite CGPoint")
    return point
}

func bounds(_ node: AXUIElement) throws -> CGRect {
    CGRect(origin: try position(node), size: try windowSize(node))
}

func requirePointerTarget(_ candidate: Candidate, _ window: AXUIElement,
                          target: AXUIElement, point: CGPoint) throws {
    do {
        try requireSession()
        guard try isForeground(candidate, window),
              (try bounds(window)).contains(point), (try bounds(target)).contains(point) else {
            throw Blocked("Pointer target is outside the current owned foreground window; no input posted")
        }
        var targetPID: pid_t = 0
        guard AXUIElementGetPid(target, &targetPID) == .success, targetPID == candidate.pid else {
            throw Blocked("Pointer target is not owned by candidate PID \(candidate.pid); no input posted")
        }
        let system = AXUIElementCreateSystemWide()
        guard AXUIElementSetMessagingTimeout(system, 1) == .success else {
            throw Blocked("Cannot bound pointer hit lookup; no input posted")
        }
        let nodes = try ownNodes(window)
        guard nodes.contains(where: { CFEqual($0, target) }) else {
            throw Blocked("Pointer target is outside the intended owned AX window; no input posted")
        }
        try requireSession()
        guard try isForeground(candidate, window),
              (try bounds(window)).contains(point), (try bounds(target)).contains(point) else {
            throw Blocked("Pointer target changed during verification; no input posted")
        }
        // Traverse before hit lookup, not after it: no further AX tree work before posting.
        // Native hit testing can return the NSWindow rather than a GPUI logical child.
        var hit: AXUIElement?
        let result = AXUIElementCopyElementAtPosition(system, Float(point.x), Float(point.y), &hit)
        guard result == .success, let hit else {
            throw Blocked("Pointer hit lookup unavailable: \(result.rawValue); no input posted")
        }
        var hitPID: pid_t = 0
        guard AXUIElementGetPid(hit, &hitPID) == .success, hitPID == candidate.pid,
              nodes.contains(where: { CFEqual($0, hit) }) else {
            throw Blocked("Pointer hit is not in candidate PID \(candidate.pid)'s intended AX window; no input posted")
        }
    } catch let blocked as Blocked {
        throw blocked
    } catch {
        throw Blocked("Cannot verify pointer recipient: \(error); no input posted")
    }
}

func click(_ candidate: Candidate, _ window: AXUIElement, _ node: AXUIElement) throws {
    let rect = try bounds(node), p = CGPoint(x: rect.midX, y: rect.midY)
    guard let source = CGEventSource(stateID: .combinedSessionState),
          let down = CGEvent(mouseEventSource: source, mouseType: .leftMouseDown,
                             mouseCursorPosition: p, mouseButton: .left),
          let up = CGEvent(mouseEventSource: source, mouseType: .leftMouseUp,
                           mouseCursorPosition: p, mouseButton: .left) else {
        throw Failure("Cannot create own mouse click")
    }
    try requirePointerTarget(candidate, window, target: node, point: p)
    down.post(tap: .cghidEventTap); up.post(tap: .cghidEventTap)
}

func number(_ node: AXUIElement) throws -> Double {
    guard let value = try attribute(node, "AXValue") as? NSNumber, value.doubleValue.isFinite else {
        throw Failure("AXValue is not finite numeric")
    }
    return value.doubleValue
}

func slider(_ window: AXUIElement, label: String) throws -> AXUIElement {
    guard let group = try named(window, role: "AXGroup", name: label) else {
        throw Failure("Metadata group missing: \(label)")
    }
    for node in try ownNodes(group) where try text(node, "AXRole") == "AXSlider" { return node }
    throw Failure("Slider missing: \(label)")
}

func focus(_ node: AXUIElement) throws {
    try require(AXUIElementSetAttributeValue(node, "AXFocused" as CFString, kCFBooleanTrue) == .success,
                "Cannot focus controlled slider")
    try waitFor("slider focus acknowledged") { (try optional(node, "AXFocused") as? Bool) == true }
}

func menuMarked(_ candidate: Candidate, title: String = "消音") throws -> Bool {
    let raw = try attribute(candidate.app, "AXMenuBar")
    try require(CFGetTypeID(raw) == AXUIElementGetTypeID(), "Menu bar is not AX element")
    guard let item = try findMenu(raw as! AXUIElement, title: title) else { throw Failure("Menu missing: \(title)") }
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(item, "AXMenuItemMarkChar" as CFString, &value)
    if result == .noValue { return false } // Native unchecked menu item representation.
    guard result == .success, let mark = value as? String else {
        throw Failure("AXMenuItemMarkChar unreadable for \(title): \(result.rawValue)")
    }
    return !mark.isEmpty
}

func mute(_ window: AXUIElement) throws -> AXUIElement {
    guard let node = try named(window, role: "AXCheckBox", name: "消音") else { throw Failure("Mute switch missing") }
    return node
}

func scroll(_ candidate: Candidate, _ window: AXUIElement, amount: Int32) throws {
    let rect = try bounds(window), p = CGPoint(x: rect.midX, y: rect.midY)
    guard let source = CGEventSource(stateID: .combinedSessionState),
          let move = CGEvent(mouseEventSource: source, mouseType: .mouseMoved,
                             mouseCursorPosition: p, mouseButton: .left),
          let event = CGEvent(scrollWheelEvent2Source: source, units: .pixel, wheelCount: 1,
                              wheel1: amount, wheel2: 0, wheel3: 0) else {
        throw Failure("Cannot create own scroll")
    }
    event.location = p
    try requirePointerTarget(candidate, window, target: window, point: p)
    move.post(tap: .cghidEventTap); event.post(tap: .cghidEventTap)
}

func controls(_ executable: URL, artifacts: URL) throws -> String {
    var evidence: [String] = []
    try withCandidate(executable, artifacts: artifacts) { candidate in
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let window = try candidate.settings()
        try foreground(candidate, window)
        try waitFor("controlled metadata and readable main summary") { try line(window, prefix: "現在の設定:") != nil }
        try expectReceipt(candidate, window, settings: 0, manual: 0)
        try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 雨 /") == true,
                    "Initial selected situation is not rain")
        try key(candidate, window, code: 48) // Tab to selected segment.
        try key(candidate, window, code: 49) // Space.
        try expectReceipt(candidate, window, settings: 1, manual: 1)
        try key(candidate, window, code: 36) // Return.
        try expectReceipt(candidate, window, settings: 2, manual: 2)
        try click(candidate, window, try segment(window, label: "雨"))
        try expectReceipt(candidate, window, settings: 3, manual: 3)
        try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 雨 /") == true,
                    "Explicit reselection changed rain target")
        evidence.append("rain metadata + selected Space/Enter/mouse exactly1 settings/manual each; receipt3/3")
        let rain = try slider(window, label: "雨の量")
        let rainBefore = try number(rain)
        let rainAfter = rainBefore < 100 ? min(100, rainBefore + 1) : rainBefore - 1
        try focus(rain); try key(candidate, window, code: rainBefore < 100 ? 124 : 123)
        try waitFor("rain knob accepts one step to \(rainAfter)") { abs(try number(rain) - rainAfter) < 0.001 }
        try expectReceipt(candidate, window, settings: 4, manual: 4)
        let wind = try slider(window, label: "風の量")
        let windBefore = try number(wind)
        let windAfter = windBefore < 100 ? min(100, windBefore + 1) : windBefore - 1
        try focus(wind); try key(candidate, window, code: windBefore < 100 ? 124 : 123)
        try waitFor("wind knob accepts one step to \(windAfter)") { abs(try number(wind) - windAfter) < 0.001 }
        try expectReceipt(candidate, window, settings: 5, manual: 5)
        try candidate.menu("消音")
        try waitFor("menu mute reflected in switch/check") { try number(mute(window)) == 1 && menuMarked(candidate) }
        try expectReceipt(candidate, window, settings: 6, manual: 5)
        let volume = try slider(window, label: "アプリの音量")
        try foreground(candidate, window); try focus(volume)
        try key(candidate, window, code: 115) // Home.
        try waitFor("volume reaches0") { abs(try number(volume)) < 0.001 }
        try expectReceipt(candidate, window, settings: 7, manual: 5)
        try require(try number(mute(window)) == 1 && menuMarked(candidate), "Volume0 unmuted")
        for revision in 8...9 {
            try key(candidate, window, code: 116) // PageUp = +10%.
            let expected = Double(revision - 7) * 10
            try waitFor("volume reaches\(expected)") { abs(try number(volume) - expected) < 0.001 }
            try expectReceipt(candidate, window, settings: revision, manual: 5)
            try require(try number(mute(window)) == 1 && menuMarked(candidate), "Volume operation unmuted")
        }
        let oldY = try position(volume).y
        try scroll(candidate, window, amount: -220)
        try waitFor("actual scroll changes viewport") { abs(try position(volume).y - oldY) > 1 }
        try require((try optional(volume, "AXFocused") as? Bool) == true, "Viewport update lost volume focus")
        try require(CFEqual(volume, try slider(window, label: "アプリの音量")), "Viewport rebuilt control identity")
        try expectReceipt(candidate, window, settings: 9, manual: 5)
        evidence.append("rain \(rainBefore)→\(rainAfter)/wind \(windBefore)→\(windAfter); menu mute switch+mark; muted volume0/10/20 manual5; real viewport scroll preserves focus/AX identity and receipt9/5")
        let toggle = try mute(window), toggleBounds = try bounds(toggle)
        try require((try bounds(window)).contains(CGPoint(x: toggleBounds.midX, y: toggleBounds.midY)),
                    "Mute switch outside viewport")
        try press(toggle)
        try waitFor("settings unmute reflected in menu") { try number(mute(window)) == 0 && !menuMarked(candidate) }
        try expectReceipt(candidate, window, settings: 10, manual: 5)
        try require(abs(try number(volume) - 20) < 0.001, "Unmute lost latest volume")
        try press(try attribute(window, "AXCloseButton") as! AXUIElement)
        try candidate.expectWindows(1)
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let restored = try candidate.settings()
        try waitFor("recreated controlled values") { try line(restored, prefix: "現在の設定:") != nil }
        try require(abs(try number(slider(restored, label: "アプリの音量")) - 20) < 0.001 &&
                    (try number(mute(restored))) == 0 &&
                    abs(try number(slider(restored, label: "雨の量")) - rainAfter) < 0.001 &&
                    abs(try number(slider(restored, label: "風の量")) - windAfter) < 0.001,
                    "Recreated settings lost current main controls")
        try expectReceipt(candidate, restored, settings: 10, manual: 5)
        evidence.append("settings unmute keeps20%; close/recreate keeps edited rain/wind/unmuted20/main receipt10/5")
        try candidate.menu("終了"); try candidate.expectExit()
        evidence.append("real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
    }
    return evidence.joined(separator: "; ")
}

func situations(_ executable: URL, helper: URL, artifacts: URL) throws -> String {
    var evidence: [String] = []
    var stage = "candidate launch"
    do {
        try withCandidate(executable, artifacts: artifacts) { candidate in
            func receipt(_ window: AXUIElement, settings: Int, manual: Int) throws {
                stage = "receipt \(settings)/\(manual) after \(stage)"
                try expectReceipt(candidate, window, settings: settings, manual: manual)
            }
            stage = "open settings"
            try candidate.menu("設定…"); try candidate.expectWindows(2)
            let window = try candidate.settings()
            stage = "settings foreground"
            try foreground(candidate, window)
            try receipt(window, settings: 0, manual: 0)
            stage = "menu mute"
            try candidate.menu("消音")
            try receipt(window, settings: 1, manual: 0)
            stage = "menu rock"
            try candidate.menu("岩")
            try receipt(window, settings: 2, manual: 1)
            stage = "rock target form/receipt/menu consistency"
            try waitFor("registered rock target in form, accepted manual display, and menu mark") {
                let main = try line(window, prefix: "現在の設定:")
                let receipt = try line(window, prefix: "音声側の受理:")
                let rockMarked = try menuMarked(candidate, title: "岩")
                let rainMarked = try menuMarked(candidate, title: "雨")
                return main?.hasPrefix("現在の設定: 岩 /") == true &&
                    receipt?.contains("手動の表示対象 岩") == true && rockMarked && !rainMarked
            }
            stage = "rock metadata sliders and initial values"
            let amount = try slider(window, label: "岩の量")
            let size = try slider(window, label: "大きさ")
            let hardness = try slider(window, label: "硬さ")
            let initials = [try number(amount), try number(size), try number(hardness)]
            stage = "rock size Home0"
            try focus(size); try key(candidate, window, code: 115)
            try waitFor("smallest-rock size0") { abs(try number(size)) < 0.001 }
            try receipt(window, settings: 3, manual: 2)
            stage = "rock hardness End100"
            try focus(hardness); try key(candidate, window, code: 119)
            try waitFor("hardness upper endpoint100") { abs(try number(hardness) - 100) < 0.001 }
            try receipt(window, settings: 4, manual: 3)
            stage = "rock amount End100"
            try focus(amount); try key(candidate, window, code: 119)
            try waitFor("rock amount upper endpoint100") { abs(try number(amount) - 100) < 0.001 }
            try receipt(window, settings: 5, manual: 4)
            stage = "settings pointer rain"
            try click(candidate, window, try segment(window, label: "雨"))
            try receipt(window, settings: 6, manual: 5)
            stage = "pointer rain main/menu consistency"
            try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 雨 /") == true &&
                        (try menuMarked(candidate, title: "雨")) && !(try menuMarked(candidate, title: "岩")),
                        "Settings rain selection and generic menu disagree")
            stage = "settings pointer rock"
            try click(candidate, window, try segment(window, label: "岩"))
            try receipt(window, settings: 7, manual: 6)
            stage = "pointer rock main/menu consistency"
            try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 岩 /") == true &&
                        (try menuMarked(candidate, title: "岩")) && !(try menuMarked(candidate, title: "雨")),
                        "Settings rock selection and generic menu disagree")
            stage = "muted volume Home0"
            let volume = try slider(window, label: "アプリの音量")
            try focus(volume); try key(candidate, window, code: 115)
            try waitFor("muted volume0") { abs(try number(volume)) < 0.001 }
            try receipt(window, settings: 8, manual: 6)
            for revision in 9...10 {
                let expected = Double(revision - 8) * 10
                stage = "muted volume PageUp\(expected)"
                try key(candidate, window, code: 116)
                try waitFor("muted volume\(expected)") { abs(try number(volume) - expected) < 0.001 }
                try receipt(window, settings: revision, manual: 6)
                stage = "volume\(expected) retained mute"
                try require(try number(mute(window)) == 1 && menuMarked(candidate), "Volume operation unmuted")
            }
            stage = "menu unmute latest20"
            try candidate.menu("消音")
            try receipt(window, settings: 11, manual: 6)
            stage = "unmute volume/mute/menu consistency"
            try require(try number(mute(window)) == 0 && !menuMarked(candidate) &&
                        abs(try number(volume) - 20) < 0.001, "Unmute lost latest20% endpoint")
            stage = "menu remute"
            try candidate.menu("消音")
            try receipt(window, settings: 12, manual: 6)
            stage = "remute form/menu consistency"
            try require(try number(mute(window)) == 1 && menuMarked(candidate), "Capture setup did not mute")
            stage = "focused nonselected rain Space"
            try focus(try segment(window, label: "雨"))
            try key(candidate, window, code: 49)
            try receipt(window, settings: 13, manual: 7)
            stage = "Space rain main/menu/radio consistency"
            try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 雨 /") == true &&
                        (try menuMarked(candidate, title: "雨")) && !(try menuMarked(candidate, title: "岩")) &&
                        (try number(try segment(window, label: "雨"))) == 1,
                        "Space activated current target instead of focused rain choice")
            stage = "focused nonselected rock Return"
            try focus(try segment(window, label: "岩"))
            try key(candidate, window, code: 36)
            try receipt(window, settings: 14, manual: 8)
            stage = "Return rock main/menu/radio/mute consistency"
            try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 岩 /") == true &&
                        (try menuMarked(candidate, title: "岩")) && !(try menuMarked(candidate, title: "雨")) &&
                        (try number(try segment(window, label: "岩"))) == 1 &&
                        (try number(mute(window))) == 1,
                        "Return activated current target instead of focused rock choice, or unmuted")
            stage = "large-rock capture size End100"
            let captureSize = try slider(window, label: "大きさ")
            try focus(captureSize); try key(candidate, window, code: 119)
            try waitFor("capture size100") { abs(try number(captureSize) - 100) < 0.001 }
            try receipt(window, settings: 15, manual: 9)
            stage = "soft-rock capture hardness Home0"
            let captureHardness = try slider(window, label: "硬さ")
            try focus(captureHardness); try key(candidate, window, code: 115)
            try waitFor("capture hardness0") { abs(try number(captureHardness)) < 0.001 }
            try receipt(window, settings: 16, manual: 10)
            stage = "capture target/mute/menu consistency"
            try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: 岩 /") == true &&
                        (try number(mute(window))) == 1 && (try menuMarked(candidate)) &&
                        (try menuMarked(candidate, title: "岩")), "Capture edits changed target or mute")
            let selectedAt = DispatchTime.now().uptimeNanoseconds
            stage = "post-selection capture separation"
            // This delay separates capture from selection; it is NOT proof of DSP arrival.
            while DispatchTime.now().uptimeNanoseconds - selectedAt < 2_300_000_000 {
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
            }
            evidence.append("Registered menu/settings rock; three knob metadata initials \(initials); size0/hardness100/amount100 receipts5/4; settings pointer rain→rock latest7/6; muted volume0/10/20 manual6; unmute20 then remute receipt12/6; focused-other rain Space13/7 then rock Return14/8; large/soft capture size100/hardness0 latest16/10 with retained rock/mute. Receipt proves accepted target, not actual gains/audible output.")
            stage = "settings close before owned scene capture"
            try press(try attribute(window, "AXCloseButton") as! AXUIElement)
            try candidate.expectWindows(1)
            stage = "three owned muted rock scene captures and two cropped comparisons"
            evidence.append(try sceneFrames(candidate, try candidate.scene(), executable: executable,
                                            helper: helper, artifacts: artifacts, prefix: "rock-muted"))
            stage = "scene menu rain before live crossing captures"
            try candidate.menu("雨")
            try require((try menuMarked(candidate, title: "雨")) &&
                        !(try menuMarked(candidate, title: "岩")) &&
                        (try menuMarked(candidate)), "Scene menu rain selection lost target or mute")
            let rainSelectedAt = DispatchTime.now().uptimeNanoseconds
            stage = "rain selection separation before rock return"
            // Separates the two selections; elapsed wall time is not DSP arrival proof.
            while DispatchTime.now().uptimeNanoseconds - rainSelectedAt < 2_300_000_000 {
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
            }
            stage = "scene menu rock and immediate live crossing captures"
            try candidate.menu("岩")
            try require((try menuMarked(candidate, title: "岩")) &&
                        !(try menuMarked(candidate, title: "雨")) &&
                        (try menuMarked(candidate)), "Scene menu rock return lost target or mute")
            evidence.append(try sceneFrames(candidate, try candidate.scene(), executable: executable,
                                            helper: helper, artifacts: artifacts, prefix: "rock-crossing-muted"))
            evidence.append("After endpoint frames, actual scene menu rain→2.3s wall separation→rock with target/mute marks; second three-frame capture helper started without an endpoint delay after rock return. Images prove live rendered-frame changes, not exact DSP gains, crossing phase, arrival timing, or gain-dependent brightness.")
            stage = "normal menu Quit"
            try candidate.menu("終了"); try candidate.expectExit()
            evidence.append("Real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
        }
    } catch let blocked as Blocked {
        throw Blocked("situations stage=\(stage); \(blocked)")
    } catch {
        throw Failure("situations stage=\(stage); \(error)")
    }
    return evidence.joined(separator: "\n")
}

func privateFile(_ data: Data, at url: URL) throws {
    let fd = open(url.path, O_WRONLY | O_CREAT | O_EXCL, 0o600)
    guard fd >= 0 else { throw Failure("Cannot create private artifact \(url.path): errno=\(errno)") }
    let file = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
    do {
        try file.write(contentsOf: data)
        try file.close()
    } catch {
        do { try file.close() } catch { fputs("Artifact close: \(error)\n", stderr) }
        throw error
    }
}

func windowID(_ candidate: Candidate, _ scene: AXUIElement) throws -> CGWindowID {
    let expected = try bounds(scene)
    guard let entries = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements],
                                                  kCGNullWindowID) as? [[String: Any]] else {
        throw Failure("Cannot read native CG window metadata")
    }
    var matches: [CGWindowID] = []
    for entry in entries {
        guard (entry[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == candidate.pid,
              (entry[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
              let rawBounds = entry[kCGWindowBounds as String] as? [String: Any],
              let frame = CGRect(dictionaryRepresentation: rawBounds as CFDictionary),
              let id = (entry[kCGWindowNumber as String] as? NSNumber)?.uint32Value else { continue }
        if abs(frame.minX - expected.minX) <= 2 && abs(frame.minY - expected.minY) <= 2 &&
            abs(frame.width - expected.width) <= 2 && abs(frame.height - expected.height) <= 2 {
            matches.append(id)
        }
    }
    try require(matches.count == 1, "Expected exactly one on-screen owned scene matching AX bounds; got \(matches)")
    return matches[0]
}

struct HelperResult {
    let status: String
    let json: [String: Any]
    let evidence: String
}

func screenshotHelper(_ script: URL, arguments: [String], artifacts: URL) throws -> HelperResult {
    guard FileManager.default.fileExists(atPath: script.path) else {
        throw Blocked("Screenshot helper missing at \(script.path)")
    }
    let token = UUID().uuidString
    let resultURL = artifacts.appendingPathComponent("screenshot-helper-\(token).json")
    let errorURL = artifacts.appendingPathComponent("screenshot-helper-\(token).log")
    try privateFile(Data(), at: resultURL)
    try privateFile(Data(), at: errorURL)
    let out = try FileHandle(forWritingTo: resultURL), err = try FileHandle(forWritingTo: errorURL)
    defer {
        do { try out.close() } catch { fputs("Helper stdout close: \(error)\n", stderr) }
        do { try err.close() } catch { fputs("Helper stderr close: \(error)\n", stderr) }
    }
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/swift")
    process.arguments = [script.path] + arguments
    process.standardOutput = out; process.standardError = err
    try process.run()
    let deadline = DispatchTime.now().uptimeNanoseconds + 90_000_000_000
    while process.isRunning && DispatchTime.now().uptimeNanoseconds < deadline {
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
    }
    if process.isRunning {
        try require(kill(process.processIdentifier, SIGKILL) == 0, "Cannot clean owned timed-out screenshot helper")
        try waitFor("owned screenshot helper cleanup") { !process.isRunning }
        process.waitUntilExit()
        throw Failure("Screenshot helper exceeded bounded90s; diagnostics \(errorURL.path)")
    }
    process.waitUntilExit()
    let data = try Data(contentsOf: resultURL)
    let diagnostics = String(decoding: try Data(contentsOf: errorURL), as: UTF8.self)
    guard process.terminationReason == .exit,
          let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          let status = json["status"] as? String,
          let expectedExit: Int32 = ["pass": 0, "fail": 1, "blocked": 77][status],
          process.terminationStatus == expectedExit else {
        throw Failure("Screenshot helper invalid JSON/exit\(process.terminationStatus): \(String(decoding: data, as: UTF8.self)); \(diagnostics)")
    }
    return HelperResult(status: status, json: json, evidence: String(decoding: data, as: UTF8.self))
}

func sceneFrames(_ candidate: Candidate, _ scene: AXUIElement, executable: URL,
                 helper: URL, artifacts: URL, prefix: String) throws -> String {
    guard CGPreflightScreenCaptureAccess() else {
        throw Blocked("Screen Recording unavailable (CGPreflightScreenCaptureAccess=false); no capture requested, no PNG fabricated. User must prepare authorization externally.")
    }
    guard FileManager.default.fileExists(atPath: helper.path) else {
        throw Blocked("Screenshot helper missing at \(helper.path)")
    }
    try foreground(candidate, scene)
    let initialBounds = try bounds(scene)
    let id = try windowID(candidate, scene)
    var evidence: [String] = []
    var images: [URL] = []
    for index in 1...3 {
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.35))
        try requireSession()
        try require(try isForeground(candidate, scene), "Scene no longer foreground before capture")
        try require(try bounds(scene) == initialBounds && windowID(candidate, scene) == id,
                    "Scene frame or owned CG window changed between captures")
        try require(try menuMarked(candidate), "Menu unexpectedly unmuted during visual evidence")
        let image = artifacts.appendingPathComponent("\(prefix)-\(index).png")
        let result = try screenshotHelper(helper, arguments: [
            "capture", "--pid", String(candidate.pid), "--window-id", String(id), "--output", image.path,
        ], artifacts: artifacts)
        if result.status == "blocked" { throw Blocked(result.evidence) }
        try require(result.status == "pass", "Own-window capture failed: \(result.evidence)")
        guard let owner = result.json["pid"] as? NSNumber,
              let capturedID = result.json["window_id"] as? NSNumber,
              let path = result.json["executable"] as? String,
              let output = result.json["output"] as? String,
              let windowOnly = result.json["window_only"] as? Bool else {
            throw Failure("Capture lacks owned identity evidence: \(result.evidence)")
        }
        try require(owner.int32Value == candidate.pid && capturedID.uint32Value == id &&
                    URL(fileURLWithPath: path).resolvingSymlinksInPath() == executable &&
                    output == image.path && windowOnly && FileManager.default.fileExists(atPath: image.path),
                    "Capture identity/output does not match owned candidate: \(result.evidence)")
        images.append(image)
        evidence.append(result.evidence)
    }
    var comparisonFailures: [String] = []
    var comparisonBlockers: [String] = []
    for index in 1..<images.count {
        let result = try screenshotHelper(helper, arguments: [
            "compare", images[index - 1].path, images[index].path,
        ], artifacts: artifacts)
        evidence.append(result.evidence)
        if result.status == "fail" { comparisonFailures.append(result.evidence) }
        if result.status == "blocked" { comparisonBlockers.append(result.evidence) }
        if result.status == "pass" {
            guard let changed = result.json["changed_pixels"] as? NSNumber,
                  let fraction = result.json["changed_fraction"] as? NSNumber,
                  result.json["crop"] is [String: Any], changed.intValue > 0,
                  fraction.doubleValue.isFinite, fraction.doubleValue > 0,
                  fraction.doubleValue <= 1 else {
                throw Failure("Compare pass lacks valid cropped pixel-change measurements: \(result.evidence)")
            }
        }
    }
    if !comparisonFailures.isEmpty { throw Failure(comparisonFailures.joined(separator: "; ")) }
    if !comparisonBlockers.isEmpty { throw Blocked(comparisonBlockers.joined(separator: "; ")) }
    evidence.append("Owned scene PID \(candidate.pid), CGWindowID \(id); real menu mute; three spaced actual PNGs/two changed cropped-interior pairs. Evidence only rendered-frame change: not audible silence, particle identity, physical synchronization, or subjective glass/music. Through-glass background can affect pixels.")
    return evidence.joined(separator: "\n")
}

func visual(_ executable: URL, helper: URL, artifacts: URL) throws -> String {
    guard CGPreflightScreenCaptureAccess() else {
        throw Blocked("Screen Recording unavailable (CGPreflightScreenCaptureAccess=false); no capture requested, no PNG fabricated. User must prepare authorization externally.")
    }
    guard FileManager.default.fileExists(atPath: helper.path) else {
        throw Blocked("Screenshot helper missing at \(helper.path)")
    }
    var evidence: [String] = []
    try withCandidate(executable, artifacts: artifacts) { candidate in
        let scene = try candidate.scene()
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let settings = try candidate.settings()
        try expectReceipt(candidate, settings, settings: 0, manual: 0)
        try candidate.menu("消音")
        try waitFor("visual menu mute reflected in settings and menu mark") {
            try number(mute(settings)) == 1 && menuMarked(candidate)
        }
        try expectReceipt(candidate, settings, settings: 1, manual: 0)
        try press(try attribute(settings, "AXCloseButton") as! AXUIElement)
        try candidate.expectWindows(1)
        evidence.append(try sceneFrames(candidate, scene, executable: executable,
                                        helper: helper, artifacts: artifacts, prefix: "scene-muted"))
        try candidate.menu("終了"); try candidate.expectExit()
        evidence.append("Real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
    }
    return evidence.joined(separator: "\n")
}

struct Check: Encodable {
    let id: String
    let status: String
    let expected: String
    let observed: String
    let artifacts: [String]
}

struct Report: Encodable {
    let schema_version = 1
    let executable: String
    let selected_case: String
    let binary_sha256: String?
    let started_at: String
    let finished_at: String
    let status: String
    let checks: [Check]
}

final class Checks {
    let artifacts: URL
    var checks: [Check] = []
    init(artifacts: URL) { self.artifacts = artifacts }

    func record(_ id: String, expected: String, _ body: () throws -> String) {
        var status = "pass"
        let observed: String
        do { observed = try body() }
        catch let blocked as Blocked { status = "blocked"; observed = blocked.description }
        catch { status = "fail"; observed = String(describing: error) }
        let files: [String]
        do {
            files = try FileManager.default.contentsOfDirectory(at: artifacts,
                includingPropertiesForKeys: nil).map(\.path).sorted()
        } catch {
            status = "fail"
            checks.append(Check(id: "\(id).artifacts", status: "fail", expected: "Discover retained private run evidence",
                                observed: String(describing: error), artifacts: []))
            files = []
        }
        // Paths may be shared between case results; they remain local, private run evidence.
        checks.append(Check(id: id, status: status, expected: expected, observed: observed, artifacts: files))
        print("\(status.uppercased()) \(id): \(observed.prefix(320))")
    }

    var status: String {
        if checks.contains(where: { $0.status == "fail" }) { return "fail" }
        if checks.isEmpty || checks.contains(where: { $0.status == "blocked" }) { return "blocked" }
        return "pass"
    }
}

let usage = """
Usage: /usr/bin/swift scripts/native-e2e.swift /absolute/path/to/ambien [--case lifecycle|controls|situations|visual|all] [--artifacts PARENT_DIR]
       /usr/bin/swift scripts/native-e2e.swift --help

Default case: all. Each run creates a unique private directory (0700) under
PARENT_DIR (default target/native-e2e), with report.json (0600), candidate logs,
and authorized own-window PNG/helper measurements. No permission prompts,
desktop captures, image uploads, or production test hooks.

Exit: 0 every selected case passed; 1 any failure; 77 prerequisites blocked
when no failure. Missing capture permission blocks visual/situations image stages;
independent lifecycle/controls still run. Null lock metadata alone is not unlocked proof.
Visual evidence is rendered-frame change while muted, not audible silence,
particle identity, physical synchronization, or subjective glass/music.
"""

struct Options {
    let executable: URL
    let selected: String
    let parent: URL

    init(_ arguments: [String]) throws {
        guard let binary = arguments.first, binary.hasPrefix("/"), !binary.hasPrefix("--") else {
            throw Failure(usage)
        }
        var selected = "all"
        var parent = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
            .appendingPathComponent("target/native-e2e", isDirectory: true)
        var seen = Set<String>()
        var index = 1
        while index < arguments.count {
            let option = arguments[index]
            try require(["--case", "--artifacts"].contains(option) &&
                        seen.insert(option).inserted && index + 1 < arguments.count,
                        "Unknown, duplicate, or incomplete argument: \(option)\n\(usage)")
            let value = arguments[index + 1]
            if option == "--case" {
                try require(["lifecycle", "controls", "situations", "visual", "all"].contains(value), "Unknown case \(value)\n\(usage)")
                selected = value
            } else {
                try require(!value.isEmpty && !value.hasPrefix("--"), "Invalid artifacts parent\n\(usage)")
                parent = URL(fileURLWithPath: value).standardizedFileURL
            }
            index += 2
        }
        executable = URL(fileURLWithPath: binary).standardizedFileURL.resolvingSymlinksInPath()
        self.selected = selected; self.parent = parent
    }
}

func runDirectory(_ parent: URL) throws -> URL {
    try FileManager.default.createDirectory(at: parent, withIntermediateDirectories: true)
    let run = parent.appendingPathComponent("run-\(UUID().uuidString)", isDirectory: true)
    try require(mkdir(run.path, 0o700) == 0, "Cannot create unique private run directory: errno=\(errno)")
    return run
}

func binaryIdentity(_ executable: URL) throws -> String {
    try require(executable.lastPathComponent == "ambien" &&
                FileManager.default.isExecutableFile(atPath: executable.path),
                "Pass the built ambien executable, not another application: \(executable.path)")
    let file = try FileHandle(forReadingFrom: executable)
    defer {
        do { try file.close() } catch { fputs("Binary close: \(error)\n", stderr) }
    }
    var hash = SHA256()
    while let chunk = try file.read(upToCount: 1_048_576), !chunk.isEmpty { hash.update(data: chunk) }
    return hash.finalize().map { String(format: "%02x", $0) }.joined()
}

setbuf(stdout, nil)
do {
    let arguments = Array(CommandLine.arguments.dropFirst())
    if arguments == ["--help"] { print(usage); exit(0) }
    let options = try Options(arguments)
    let started = ISO8601DateFormatter().string(from: Date())
    let artifacts = try runDirectory(options.parent)
    let checks = Checks(artifacts: artifacts)
    var sha: String?
    checks.record("binary.identity", expected: "Existing executable Ambien candidate, absolute resolved path and SHA256") {
        sha = try binaryIdentity(options.executable)
        return "executable=\(options.executable.path); SHA256=\(sha!)"
    }
    let binaryReady = checks.checks.last?.status == "pass"
    checks.record("session.preflight", expected: "Safe active/awake logged-in own console; AX and event posting preflight without prompts; bounded AX before child launch") {
        try requireSession()
        try require(AXUIElementSetMessagingTimeout(AXUIElementCreateSystemWide(), 1) == .success,
                    "Cannot bound AX messages globally for this runner process")
        return "Own console + completed login + matching UID + awake/active main display + non-loginwindow foreground corroborated; no positive lock indication (nil alone not proof); AX=true,event_post=true; capture=\(CGPreflightScreenCaptureAccess()). No authorization/OS settings changed."
    }
    let nativeReady = checks.checks.last?.status == "pass" && binaryReady
    let helper = URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL
        .deletingLastPathComponent().appendingPathComponent("native-screenshot.swift")
    for selected in ["lifecycle", "controls", "situations", "visual"] where options.selected == "all" || options.selected == selected {
        let expected: String
        switch selected {
        case "lifecycle":
            expected = "Real menus, same settings singleton, minimize/restore and disabled zoom on both windows, settings Cmd-comma recreation, scene close/zero-window/menu recreation, menu Quit and foreground Cmd-Q from both windows with real PID disappearance/normal0"
        case "controls":
            expected = "P03 current rain metadata; exact main/audio settings+manual receipts for selected Space/Enter/mouse, real rain/wind edits, menu mute/mark, muted volume0/10/20, real scroll with unchanged revisions/focus/AX identity, settings unmute latest20%, close/recreate retaining main controls, menu Quit normal0"
        case "situations":
            expected = "Registered rock menu/settings and owned labeled radio choices; exact accepted target revisions and menu marks; rock amount/size/hardness endpoints; settings pointer rain→rock latest selection; muted volume0/10/20 then unmute latest20 and remute; focused-other rain Space and rock Return with exact target receipts/radio/menu/main; real size100/hardness0 edits for large/soft muted capture latest16/10; three owned muted endpoint PNGs/two interior-change pairs; actual scene menu rain then rock return immediately initiating another three owned muted PNGs/two pairs; menu Quit normal0. Not actual-gain, audible, particle-only, exact crossing timing/brightness or physical-sync proof."
        default:
            expected = "Authorized actual owned scene-window PNGs; real menu mute; stable CG owner/bounds; three spaced captures; two cropped-interior nonblank/nontransparent pixel-change comparisons; retained PNGs and measurements; menu Quit normal0"
        }
        checks.record(selected, expected: expected) {
            guard nativeReady else { throw Blocked("Not executed: binary/session prerequisites did not pass; see preceding checks") }
            switch selected {
            case "lifecycle": return try lifecycle(options.executable, artifacts: artifacts)
            case "controls": return try controls(options.executable, artifacts: artifacts)
            case "situations": return try situations(options.executable, helper: helper, artifacts: artifacts)
            default: return try visual(options.executable, helper: helper, artifacts: artifacts)
            }
        }
    }
    let report = Report(executable: options.executable.path, selected_case: options.selected,
                        binary_sha256: sha, started_at: started,
                        finished_at: ISO8601DateFormatter().string(from: Date()),
                        status: checks.status, checks: checks.checks)
    let encoder = JSONEncoder()
    encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
    let reportURL = artifacts.appendingPathComponent("report.json")
    try privateFile(encoder.encode(report), at: reportURL)
    print("REPORT \(reportURL.path)")
    print("\(checks.status.uppercased()) native E2E --case \(options.selected)")
    exit(checks.status == "pass" ? 0 : checks.status == "fail" ? 1 : 77)
} catch {
    fputs("FAIL native E2E: \(error)\n", stderr)
    exit(1)
}
