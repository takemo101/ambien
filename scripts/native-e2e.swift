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
    let homeURL: URL
    private let errors: FileHandle

    init(executable: URL, artifacts: URL, home: URL? = nil) throws {
        let root = artifacts.standardizedFileURL.resolvingSymlinksInPath()
        homeURL = (home ?? root.appendingPathComponent("home-\(UUID().uuidString)", isDirectory: true))
            .standardizedFileURL.resolvingSymlinksInPath()
        try require(homeURL.path.hasPrefix(root.path + "/"), "Candidate HOME must remain in the private run")
        try FileManager.default.createDirectory(at: homeURL, withIntermediateDirectories: true,
                                               attributes: [.posixPermissions: 0o700])
        errorsURL = artifacts.appendingPathComponent("candidate-\(UUID().uuidString).log")
        let fd = open(errorsURL.path, O_RDWR | O_CREAT | O_EXCL, 0o600)
        guard fd >= 0 else { throw Failure("Cannot create private candidate log: errno=\(errno)") }
        errors = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        process.executableURL = executable
        var environment = ProcessInfo.processInfo.environment
        environment["HOME"] = homeURL.path
        process.environment = environment
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
func withCandidate(_ executable: URL, artifacts: URL, home: URL? = nil,
                   _ scenario: (Candidate) throws -> Void) throws {
    try requireSession()
    let candidate = try Candidate(executable: executable, artifacts: artifacts, home: home)
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

// Reveal the actual owned control, retaining all pointer/session guards.
// Scroll geometry is not fixed: settings gain rows as required controls appear.
func visible(_ candidate: Candidate, _ window: AXUIElement, _ node: AXUIElement) throws {
    try foreground(candidate, window)
    for attempt in 0..<16 {
        let rect = try bounds(node), viewport = try bounds(window).insetBy(dx: 24, dy: 48)
        let point = CGPoint(x: rect.midX, y: rect.midY)
        if viewport.contains(point) {
            try requirePointerTarget(candidate, window, target: node, point: point)
            return
        }
        try require(attempt < 15, "Cannot bring production control into owned viewport")
        try scroll(candidate, window, amount: point.y < viewport.minY ? 160 : -160)
        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.08))
    }
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

func observeOwnedSettings<T>(_ candidate: Candidate, _ window: AXUIElement,
                             _ read: () throws -> T) throws -> T? {
    try require(CFEqual(try candidate.settings(), window), "Owned settings window changed during observation")
    do { return try read() }
    catch let failure as AXReadFailure
        where failure.status == .invalidUIElement && !CFEqual(failure.element, window) {
        // AccessKit may retire a virtual descendant during a non-atomic walk.
        // Discard the entire owned observation; never catch input/capture errors.
        var pid: pid_t = 0
        try require(AXUIElementGetPid(failure.element, &pid) == .success && pid == candidate.pid,
                    "Invalidated AX descendant is not owned by the candidate")
        try require(CFEqual(try candidate.settings(), window), "Owned settings window invalidated")
        print("OBSERVE invalidated owned AX descendant (\(failure.name)); discard snapshot")
        return nil
    }
}

func expectReceipt(_ candidate: Candidate, _ window: AXUIElement, settings: Int, manual: Int) throws {
    func observe() throws -> ReceiptObservation {
        try observeOwnedSettings(candidate, window) {
            guard let main = try line(window, prefix: "現在の設定:"),
                  let receipt = try line(window, prefix: "音声側の受理:") else { return .waiting }
            let m = try generations(main), r = try generations(receipt)
            try require(m.0 <= settings && m.1 <= manual && r.0 <= settings && r.1 <= manual,
                        "Unexpected/duplicate revision: main=\(main), receipt=\(receipt), expected=\(settings)/\(manual)")
            return m.0 == settings && m.1 == manual && r.0 == settings && r.1 == manual &&
                !receipt.contains("受理待ち") ? .exact : .waiting
        } ?? .invalidated
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
        let toggle = try mute(window)
        try visible(candidate, window, toggle)
        try expectReceipt(candidate, window, settings: 9, manual: 5)
        let toggleBounds = try bounds(toggle)
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
                    receipt?.contains("表示対象 岩") == true && rockMarked && !rainMarked
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

func deep(_ executable: URL, helper: URL, artifacts: URL) throws -> String {
    var evidence: [String] = []
    var stage = "candidate launch"
    do {
        try withCandidate(executable, artifacts: artifacts) { candidate in
            func receipt(_ window: AXUIElement, settings: Int, manual: Int) throws {
                stage = "receipt \(settings)/\(manual) after \(stage)"
                try expectReceipt(candidate, window, settings: settings, manual: manual)
            }
            func selection(_ window: AXUIElement, label: String) throws {
                try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: \(label) /") == true &&
                            (try line(window, prefix: "音声側の受理:"))?.contains("表示対象 \(label)") == true,
                            "Deep case main/audio accepted target disagrees with \(label)")
                for choice in ["雨", "岩", "深海"] {
                    let selected = choice == label
                    try require((try number(try segment(window, label: choice))) == (selected ? 1 : 0) &&
                                (try menuMarked(candidate, title: choice)) == selected,
                                "Deep case owned radio/menu selection disagrees for \(choice)")
                }
                try require(try number(mute(window)) == 1 && menuMarked(candidate),
                            "Deep case target operation lost mute")
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
            stage = "menu deep"
            try candidate.menu("深海")
            try receipt(window, settings: 2, manual: 1)
            stage = "deep main/audio/menu/radio/mute consistency"
            try selection(window, label: "深海")
            stage = "deep metadata controls"
            let depth = try slider(window, label: "深さ")
            let air = try slider(window, label: "空気の量")
            let initials = [try number(depth), try number(air)]
            stage = "depth Home0"
            try focus(depth); try key(candidate, window, code: 115)
            try waitFor("shallow depth endpoint0") { abs(try number(depth)) < 0.001 }
            try receipt(window, settings: 3, manual: 2)
            stage = "air Home0"
            try focus(air); try key(candidate, window, code: 115)
            try waitFor("air endpoint0") { abs(try number(air)) < 0.001 }
            try receipt(window, settings: 4, manual: 3)
            stage = "zero endpoints retain accepted deep and mute"
            try selection(window, label: "深海")
            stage = "air End100"
            try focus(air); try key(candidate, window, code: 119)
            try waitFor("air upper endpoint100") { abs(try number(air) - 100) < 0.001 }
            try receipt(window, settings: 5, manual: 4)
            stage = "depth End100"
            try focus(depth); try key(candidate, window, code: 119)
            try waitFor("deep depth endpoint100") { abs(try number(depth) - 100) < 0.001 }
            try receipt(window, settings: 6, manual: 5)
            stage = "high endpoints retain accepted deep and mute"
            try selection(window, label: "深海")
            for (index, label) in ["雨", "岩"].enumerated() {
                stage = "actual menu \(label)"
                try candidate.menu(label)
                try receipt(window, settings: 7 + index, manual: 6 + index)
                stage = "menu \(label) main/audio/menu/radio/mute consistency"
                try selection(window, label: label)
            }
            for (index, label) in ["雨", "岩", "深海"].enumerated() {
                stage = "actual settings radio \(label)"
                try click(candidate, window, try segment(window, label: label))
                try receipt(window, settings: 9 + index, manual: 8 + index)
                stage = "settings \(label) main/audio/menu/radio/mute consistency"
                try selection(window, label: label)
            }
            stage = "returned deep endpoint retention"
            try require(abs(try number(try slider(window, label: "深さ")) - 100) < 0.001 &&
                        abs(try number(try slider(window, label: "空気の量")) - 100) < 0.001,
                        "Deep target return lost edited endpoints")
            evidence.append("All three actual menu choices and owned settings AXRadioButton choices agree with main/audio accepted targets and exclusive menu/radio marks while muted. Deep candidate initials \(initials) observed, not pinned. Menu mute1/0; menu deep2/1; depth Home0 exactly3/2; air Home0 exactly4/3; air End100 exactly5/4; depth End100 exactly6/5; menu rain7/6, rock8/7; settings rain9/8, rock10/9, deep11/10 retaining depth100/air100. Receipts prove accepted targets only: not actual gains, audible low-sound continuity, absence/frequency of bubbles, darkness, or motion speed.")
            stage = "settings close before owned deep scene capture"
            try press(try attribute(window, "AXCloseButton") as! AXUIElement)
            try candidate.expectWindows(1)
            stage = "post-selection deep capture wall separation (not DSP gain proof)"
            let captureSetupAt = DispatchTime.now().uptimeNanoseconds
            // Separate endpoint images from preceding selections, not AX failures.
            while DispatchTime.now().uptimeNanoseconds - captureSetupAt < 2_300_000_000 {
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.02))
            }
            evidence.append("After settings close, 2.3s deliberate wall separation precedes deep-muted capture. Elapsed time is not DSP arrival/gain proof; images do not establish Deep-only attribution or subjective quality.")
            stage = "three owned muted deep scene captures and two cropped comparisons"
            evidence.append(try sceneFrames(candidate, try candidate.scene(), executable: executable,
                                            helper: helper, artifacts: artifacts, prefix: "deep-muted"))
            stage = "normal menu Quit"
            try candidate.menu("終了"); try candidate.expectExit()
            evidence.append("Real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
        }
    } catch let blocked as Blocked {
        throw Blocked("deep stage=\(stage); \(blocked)")
    } catch {
        throw Failure("deep stage=\(stage); \(error)")
    }
    return evidence.joined(separator: "\n")
}

func mixing(_ executable: URL, helper: URL, artifacts: URL) throws -> String {
    var evidence: [String] = []
    var stage = "candidate launch"
    do {
        try withCandidate(executable, artifacts: artifacts) { candidate in
            func receipt(_ window: AXUIElement, settings: Int, manual: Int) throws {
                stage = "receipt \(settings)/\(manual) after \(stage)"
                try expectReceipt(candidate, window, settings: settings, manual: manual)
            }
            func selection(_ window: AXUIElement, label: String, muted: Bool = true) throws {
                try require((try line(window, prefix: "現在の設定:"))?.hasPrefix("現在の設定: \(label) /") == true &&
                            (try line(window, prefix: "音声側の受理:"))?.contains("表示対象 \(label)") == true,
                            "Mixing main/audio accepted target disagrees with \(label)")
                for choice in ["雨", "岩", "深海"] {
                    let selected = choice == label
                    try require((try number(try segment(window, label: choice))) == (selected ? 1 : 0) &&
                                (try menuMarked(candidate, title: choice)) == selected,
                                "Mixing owned radio/menu selection disagrees for \(choice)")
                }
                try require(try number(mute(window)) == (muted ? 1 : 0) &&
                            menuMarked(candidate) == muted, "Mixing target operation changed mute")
            }
            stage = "open owned settings and observe fresh state"
            try candidate.menu("設定…"); try candidate.expectWindows(2)
            let window = try candidate.settings()
            try foreground(candidate, window)
            try receipt(window, settings: 0, manual: 0)
            let initial = try line(window, prefix: "現在の設定:")
            for choice in ["雨", "岩", "深海"] {
                _ = try segment(window, label: choice)
                _ = try menuMarked(candidate, title: choice)
            }
            stage = "independent menu mute"
            try candidate.menu("消音")
            try receipt(window, settings: 1, manual: 0)
            try require(try number(mute(window)) == 1 && menuMarked(candidate), "Mixing menu mute not reflected")
            var previousMenuAt: UInt64?
            var intervals: [Double] = []
            for (index, label) in ["雨", "岩", "深海", "岩", "深海", "深海"].enumerated() {
                stage = "rapid menu \(index + 1) \(label)"
                let commandAt = DispatchTime.now().uptimeNanoseconds
                if let previousMenuAt {
                    let interval = Double(commandAt - previousMenuAt) / 1_000_000_000
                    intervals.append(interval)
                    try require(interval < 2, "Native selection cadence missed the 2s interruption input window: \(interval)s")
                }
                previousMenuAt = commandAt
                try candidate.menu(label)
                try receipt(window, settings: 2 + index, manual: 1 + index)
                try selection(window, label: label)
            }
            evidence.append("Fresh main state observed, not pinned: \(initial ?? "missing"). Muted menu rain2/1, rock3/2, deep4/3, already-requested rock5/4, deep6/5, selected deep7/6. Actual command-start intervals \(intervals)s each <2s; this bounds native input cadence only, not callback receipt time, actual overlap, gains or DSP arrival.")
            for (index, label) in ["雨", "岩", "深海", "深海"].enumerated() {
                stage = "actual settings radio \(index + 1) \(label)"
                try click(candidate, window, try segment(window, label: label))
                try receipt(window, settings: 8 + index, manual: 7 + index)
                try selection(window, label: label)
            }
            stage = "current deep knob one-step edit"
            let depth = try slider(window, label: "深さ")
            let depthBefore = try number(depth)
            let depthAfter = depthBefore < 100 ? min(100, depthBefore + 1) : depthBefore - 1
            try focus(depth); try key(candidate, window, code: depthBefore < 100 ? 124 : 123)
            try waitFor("owned depth knob one step") { abs(try number(depth) - depthAfter) < 0.001 }
            try receipt(window, settings: 12, manual: 11)
            try selection(window, label: "深海")
            stage = "muted volume Home0"
            let volume = try slider(window, label: "アプリの音量")
            try focus(volume); try key(candidate, window, code: 115)
            try waitFor("mixing volume0") { abs(try number(volume)) < 0.001 }
            try receipt(window, settings: 13, manual: 11)
            try selection(window, label: "深海")
            for (index, expected) in [10.0, 20.0, 19.0, 18.0].enumerated() {
                stage = "muted volume \(expected)"
                try key(candidate, window, code: index < 2 ? 116 : 123)
                try waitFor("mixing muted volume\(expected)") { abs(try number(volume) - expected) < 0.001 }
                try receipt(window, settings: 14 + index, manual: 11)
                try selection(window, label: "深海")
            }
            stage = "real settings viewport scroll without a control edit"
            let volumeY = try position(volume).y
            try scroll(candidate, window, amount: -220)
            try waitFor("mixing scroll changes actual viewport") { abs(try position(volume).y - volumeY) > 1 }
            try require((try optional(volume, "AXFocused") as? Bool) == true &&
                        CFEqual(volume, try slider(window, label: "アプリの音量")),
                        "Mixing viewport update lost focused volume identity")
            try receipt(window, settings: 17, manual: 11)
            try selection(window, label: "深海")
            stage = "settings pointer unmute latest target at18"
            try visible(candidate, window, try mute(window))
            try receipt(window, settings: 17, manual: 11)
            try click(candidate, window, try mute(window))
            try receipt(window, settings: 18, manual: 11)
            try selection(window, label: "深海", muted: false)
            try require(abs(try number(volume) - 18) < 0.001, "Unmute lost latest18% volume")
            try visible(candidate, window, volume)
            try focus(volume)
            for (index, expected) in [19.0, 20.0].enumerated() {
                stage = "unmuted volume \(expected)"
                try key(candidate, window, code: 124)
                try waitFor("mixing unmuted volume\(expected)") { abs(try number(volume) - expected) < 0.001 }
                try receipt(window, settings: 19 + index, manual: 11)
                try selection(window, label: "深海", muted: false)
            }
            stage = "menu remute latest20"
            try candidate.menu("消音")
            try receipt(window, settings: 21, manual: 11)
            try selection(window, label: "深海")
            stage = "selected latest deep menu command after volume and mute"
            try candidate.menu("深海")
            try receipt(window, settings: 22, manual: 12)
            try selection(window, label: "深海")
            evidence.append("Owned settings pointer rain8/7, rock9/8, deep10/9, selected deep11/10; observed depth \(depthBefore) → \(depthAfter) one-step12/11. Volume Home0 13/11; PageUp10 14/11, PageUp20 15/11, Left19 16/11, Left18 17/11, all muted. Settings pointer unmute18/11 at18%; Right19% 19/11, Right20% 20/11 without manual revision; menu remute21/11 and selected latest deep22/12. Every action checks exact main/audio receipts, exclusive menu/radio marks and independent mute; no older accepted target replay.")
            stage = "close settings and capture live muted scene"
            try press(try attribute(window, "AXCloseButton") as! AXUIElement)
            try candidate.expectWindows(1)
            evidence.append("No DSP-settle timer before mixing capture: images may include a crossing or later live frames. Capture/helper latency and 0.35s spacing do not establish three positive gains, exact crossing timing, one-hot arrival, no clicks, audible silence or physical synchronization.")
            evidence.append(try sceneFrames(candidate, try candidate.scene(), executable: executable,
                                            helper: helper, artifacts: artifacts, prefix: "mixing-muted"))
            stage = "reopen settings and reject older intent after captures"
            try candidate.menu("設定…"); try candidate.expectWindows(2)
            let reopened = try candidate.settings()
            try receipt(reopened, settings: 22, manual: 12)
            try selection(reopened, label: "深海")
            try require(abs(try number(try slider(reopened, label: "深さ")) - depthAfter) < 0.001 &&
                        abs(try number(try slider(reopened, label: "アプリの音量")) - 20) < 0.001,
                        "Mixing reopened controls lost latest depth/volume")
            evidence.append("After three owned muted PNGs/two comparisons, recreated settings remains deep22/12, depth\(depthAfter), volume20%, muted. Receipt stability is accepted-control evidence, not actual gains, maximum-three RT work, re-entry audio tails, waveform peak/RMS/delta or subjective quality; these require separate actual audio/runtime acceptance.")
            stage = "normal menu Quit"
            try candidate.menu("終了"); try candidate.expectExit()
            evidence.append("Real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
        }
    } catch let blocked as Blocked {
        throw Blocked("mixing stage=\(stage); \(blocked)")
    } catch {
        throw Failure("mixing stage=\(stage); \(error)")
    }
    return evidence.joined(separator: "\n")
}

func wandering(_ executable: URL, artifacts: URL) throws -> String {
    var evidence: [String] = []
    var stage = "candidate launch"
    do {
        try withCandidate(executable, artifacts: artifacts) { candidate in
            let began = DispatchTime.now().uptimeNanoseconds
            func seconds(_ since: UInt64) -> Double {
                Double(DispatchTime.now().uptimeNanoseconds - since) / 1_000_000_000
            }
            func record(_ value: String) {
                let entry = "t+\(String(format: "%.3f", seconds(began)))s \(value)"
                evidence.append(entry)
                print("WANDERING \(entry)")
            }
            stage = "open owned settings"
            try candidate.menu("設定…"); try candidate.expectWindows(2)
            let window = try candidate.settings()
            try foreground(candidate, window)
            try expectReceipt(candidate, window, settings: 0, manual: 0)
            var revisions = (0, 0)
            let choices = ["雨", "岩", "深海"]

            // AccessKit can retire virtual descendants during a non-atomic read.
            // Retry only owned invalid descendants, never mismatching stable data.
            func observation<T>(_ read: () throws -> T) throws -> T? {
                try observeOwnedSettings(candidate, window, read)
            }
            func wanderingSwitch() throws -> AXUIElement {
                guard let node = try named(window, role: "AXCheckBox", name: "ひとりで移る") else {
                    throw Failure("Production wandering checkbox missing")
                }
                return node
            }
            func wanderState(_ enabled: Bool) throws -> Bool {
                guard let main = try line(window, prefix: "現在の設定:") else { return false }
                return try number(wanderingSwitch()) == (enabled ? 1 : 0) &&
                    menuMarked(candidate, title: "ひとりで移る") == enabled &&
                    main.contains("/ ひとりで移る \(enabled ? "オン" : "オフ")")
            }
            func timeValues(_ minimum: Double, _ maximum: Double, _ transition: Double) throws -> Bool {
                guard let main = try line(window, prefix: "現在の設定:") else { return false }
                return try abs(number(slider(window, label: "最小滞在時間")) - minimum) < 0.001 &&
                    abs(number(slider(window, label: "最大滞在時間")) - maximum) < 0.001 &&
                    abs(number(slider(window, label: "自動切り替え時間")) - transition) < 0.001 &&
                    main.contains("/ 最小滞在時間 \(Int(minimum))分") &&
                    main.contains("/ 最大滞在時間 \(Int(maximum))分") &&
                    main.contains("/ 自動切り替え時間 \(Int(transition))秒")
            }
            func markedChoice() throws -> String {
                var selected: String?
                var reportedChangingSnapshot = false
                // Each menuMarked call obtains a fresh AX tree. A menu rebuild
                // between the three reads is not one multi-selection snapshot.
                try waitFor("coherent main/audio/exclusive registered menu display") {
                    try observation {
                        guard let main = try line(window, prefix: "現在の設定:"),
                              let receipt = try line(window, prefix: "音声側の受理:") else { return false }
                        var marked: [String] = []
                        for choice in choices where try menuMarked(candidate, title: choice) {
                            marked.append(choice)
                        }
                        let afterMain = try line(window, prefix: "現在の設定:")
                        let afterReceipt = try line(window, prefix: "音声側の受理:")
                        let mainRevisions = try generations(main), receiptRevisions = try generations(receipt)
                        let coherent = main == afterMain && receipt == afterReceipt &&
                            marked.count == 1 && main.hasPrefix("現在の設定: \(marked.first ?? "") /") &&
                            receipt.contains("表示対象 \(marked.first ?? "")") &&
                            mainRevisions == receiptRevisions && !receipt.contains("受理待ち")
                        if !coherent {
                            if !reportedChangingSnapshot {
                                record("AX snapshot not yet coherent; marks=\(marked); main=\(main); receipt=\(receipt); after-main=\(afterMain ?? "not exported"); after-receipt=\(afterReceipt ?? "not exported")")
                                reportedChangingSnapshot = true
                            }
                            return false
                        }
                        selected = marked[0]
                        return true
                    } == true
                }
                guard let selected else { throw Failure("No coherent exclusive registered menu display") }
                return selected
            }
            func consistentDisplay(_ label: String, enabled: Bool) throws -> Bool {
                guard let main = try line(window, prefix: "現在の設定:"),
                      let receipt = try line(window, prefix: "音声側の受理:") else { return false }
                let m = try generations(main), r = try generations(receipt)
                try require(m.0 == revisions.0 && m.1 == revisions.1 &&
                            r.0 == revisions.0 && r.1 == revisions.1 && !receipt.contains("受理待ち"),
                            "Uncommanded or stale wandering revision: main=\(main); receipt=\(receipt)")
                guard main.hasPrefix("現在の設定: \(label) /"),
                      receipt.contains("表示対象 \(label)"), try markedChoice() == label,
                      try wanderState(enabled) else { return false }
                for choice in choices {
                    if try number(segment(window, label: choice)) != (choice == label ? 1 : 0) { return false }
                }
                try require(try number(mute(window)) == 1 && menuMarked(candidate),
                            "Wandering unexpectedly lost mute")
                return true
            }
            func expectDisplay(_ label: String, enabled: Bool) throws {
                try waitFor("wandering main/audio/radio/menu display \(label), enabled=\(enabled)") {
                    try observation { try consistentDisplay(label, enabled: enabled) } == true
                }
            }
            func command(_ name: String, manualDelta: Int = 0,
                         _ action: () throws -> Void, accepted: () throws -> Bool) throws -> UInt64 {
                stage = name
                let before = revisions, at = DispatchTime.now().uptimeNanoseconds
                record("COMMAND \(name), from \(before.0)/\(before.1), UTC \(ISO8601DateFormatter().string(from: Date()))")
                try action()
                var actual = before
                try waitFor("observed accepted wandering operation: \(name)") {
                    try observation {
                        guard let main = try line(window, prefix: "現在の設定:") else { return false }
                        let next = try generations(main)
                        try require(next.0 >= before.0 && next.1 >= before.1 &&
                                    next.1 <= before.1 + manualDelta,
                                    "Unexpected revision during \(name): \(main)")
                        guard next.0 > before.0 && next.1 == before.1 + manualDelta,
                              try accepted() else { return false }
                        actual = next
                        return true
                    } == true
                }
                // Derive the real accepted settings generation rather than guess
                // whether paired min/max correction coalesces one or two edits.
                try expectReceipt(candidate, window, settings: actual.0, manual: actual.1)
                revisions = actual
                record("ACCEPTED \(name), exact \(actual.0)/\(actual.1), main=\(try line(window, prefix: "現在の設定:") ?? "missing"), receipt=\(try line(window, prefix: "音声側の受理:") ?? "missing")")
                return at
            }
            func menuWander(_ enabled: Bool) throws {
                _ = try command("menu wander \(enabled)", { try candidate.menu("ひとりで移る") },
                                accepted: { try wanderState(enabled) })
            }
            func settingsWander(_ enabled: Bool) throws {
                _ = try command("settings pointer wander \(enabled)", {
                    let node = try wanderingSwitch()
                    try visible(candidate, window, node); try click(candidate, window, node)
                }, accepted: { try wanderState(enabled) })
            }
            func timeKey(_ label: String, code: CGKeyCode, minimum: Double,
                         maximum: Double, transition: Double) throws {
                _ = try command("visible focused \(label) key\(code)", {
                    let node = try slider(window, label: label)
                    try visible(candidate, window, node); try focus(node); try key(candidate, window, code: code)
                }, accepted: { try timeValues(minimum, maximum, transition) && wanderState(true) })
            }

            try require(try wanderState(true), "Fresh candidate did not start with wander on")
            for choice in choices { _ = try segment(window, label: choice); _ = try menuMarked(candidate, title: choice) }
            record("Fresh owned PID \(candidate.pid), exact0/0, wander on; registered production menu/radio choices \(choices)")
            _ = try command("actual menu mute", { try candidate.menu("消音") },
                            accepted: { try number(mute(window)) == 1 && menuMarked(candidate) && wanderState(true) })
            try menuWander(false); try menuWander(true)
            try settingsWander(false); try settingsWander(true)
            _ = try command("explicit menu rock disables wander", manualDelta: 1, { try candidate.menu("岩") },
                            accepted: { try wanderState(false) && markedChoice() == "岩" })
            try expectDisplay("岩", enabled: false)
            try menuWander(true)
            _ = try command("explicit settings pointer rain disables wander", manualDelta: 1, {
                let node = try segment(window, label: "雨")
                try visible(candidate, window, node); try click(candidate, window, node)
            }, accepted: { try wanderState(false) && markedChoice() == "雨" })
            try expectDisplay("雨", enabled: false)
            try menuWander(true)
            let rain = try slider(window, label: "雨の量"), rainBefore = try number(rain)
            let rainAfter = rainBefore < 100 ? rainBefore + 1 : rainBefore - 1
            _ = try command("visible focused rain knob \(rainBefore)→\(rainAfter) disables wander", manualDelta: 1, {
                try visible(candidate, window, rain); try focus(rain)
                try key(candidate, window, code: rainBefore < 100 ? 124 : 123)
            }, accepted: { try abs(number(rain) - rainAfter) < 0.001 && wanderState(false) })
            try expectDisplay("雨", enabled: false)
            try menuWander(true)
            let volume = try slider(window, label: "アプリの音量"), volumeBefore = try number(volume)
            let volumeAfter = volumeBefore < 100 ? volumeBefore + 1 : volumeBefore - 1
            _ = try command("visible focused muted volume \(volumeBefore)→\(volumeAfter) retains wander", {
                try visible(candidate, window, volume); try focus(volume)
                try key(candidate, window, code: volumeBefore < 100 ? 124 : 123)
            }, accepted: {
                try abs(number(volume) - volumeAfter) < 0.001 && wanderState(true) &&
                    number(mute(window)) == 1 && menuMarked(candidate)
            })

            let initialMax = try number(slider(window, label: "最大滞在時間"))
            let initialTransition = try number(slider(window, label: "自動切り替え時間"))
            try timeKey("最小滞在時間", code: 115, minimum: 1, maximum: initialMax, transition: initialTransition)
            try timeKey("最大滞在時間", code: 115, minimum: 1, maximum: 1, transition: initialTransition)
            try timeKey("最小滞在時間", code: 124, minimum: 2, maximum: 2, transition: initialTransition)
            record("Paired upward correction: min Right1→2 raises max1→2, wander on, manual unchanged")
            try timeKey("最大滞在時間", code: 123, minimum: 1, maximum: 1, transition: initialTransition)
            record("Paired downward correction: max Left2→1 lowers min2→1, wander on, manual unchanged")
            try timeKey("自動切り替え時間", code: 115, minimum: 1, maximum: 1, transition: 1)
            try require(try named(window, role: "AXStaticText", name: "次の滞在から適用") != nil &&
                        named(window, role: "AXStaticText", name: "次の自動切り替えから適用") != nil,
                        "Exact production next-application time explanations missing")
            record("Production descriptions 次の滞在から適用 / 次の自動切り替えから適用; final named times1/1min and1sec")

            try menuWander(false)
            _ = try command("final explicit rain while off after time edits", manualDelta: 1, { try candidate.menu("雨") },
                            accepted: { try wanderState(false) && markedChoice() == "雨" })
            try expectDisplay("雨", enabled: false)
            let separated = DispatchTime.now().uptimeNanoseconds
            while seconds(separated) < 2.3 {
                try requireSession()
                try require(candidate.alive(), "Candidate exited before automatic dwell")
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.05))
            }
            record("2.3s wall separation from final manual receipt; not actual DSP arrival proof")
            let enabledAt = try command("final staying menu off→on starts fresh dwell", { try candidate.menu("ひとりで移る") },
                                        accepted: { try wanderState(true) && timeValues(1, 1, 1) })
            try expectDisplay("雨", enabled: true)
            stage = "bounded real automatic display observation, maximum95s"
            var automatic: String?
            var firstDisplayAt: UInt64?
            while seconds(enabledAt) <= 95 {
                try requireSession()
                try require(candidate.alive(), "Candidate exited during automatic wait")
                let label = try markedChoice()
                if label != "雨" {
                    let at = DispatchTime.now().uptimeNanoseconds, elapsed = seconds(enabledAt)
                    record("CLOCK_DIAGNOSTIC elapsed=\(elapsed)s; main=\(try line(window, prefix: "現在の設定:") ?? "not exported"); receipt=\(try line(window, prefix: "音声側の受理:") ?? "not exported"); output=\(try line(window, prefix: "標準出力で再生中:") ?? "not exported"); on-command-start=\(enabledAt); current-uptime=\(at)")
                    try require(elapsed >= 60 && elapsed <= 95, "Automatic display outside60…95 wall seconds: \(elapsed)s to \(label)")
                    automatic = label; firstDisplayAt = at
                    try expectDisplay(label, enabled: true)
                    record("AUTOMATIC rain→\(label) first menu mark at \(String(format: "%.3f", elapsed))s after on command; coherent menu/radio/main/receipt \(revisions.0)/\(revisions.1), muted; no manual/time edits during wait")
                    break
                }
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
            }
            guard let arrived = automatic, let firstDisplayAt else {
                throw Failure("No genuine different registered automatic display within95 real wall seconds; last=\(try markedChoice()), exact\(revisions.0)/\(revisions.1)")
            }
            func retainDisplay(_ label: String, enabled: Bool, since: UInt64, duration: Double) throws {
                var checkedAt = DispatchTime.now().uptimeNanoseconds
                while seconds(since) < duration {
                    try requireSession()
                    try require(candidate.alive(), "Candidate exited while retaining wandering display")
                    try require(try markedChoice() == label, "Display did not stay \(label) for \(duration)s, enabled=\(enabled)")
                    if seconds(checkedAt) >= 5 {
                        try expectDisplay(label, enabled: enabled)
                        checkedAt = DispatchTime.now().uptimeNanoseconds
                    }
                    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
                }
                try expectDisplay(label, enabled: enabled)
                record("Retained actual \(label) display \(String(format: "%.3f", seconds(since))) wall seconds, wander=\(enabled), muted, exact\(revisions.0)/\(revisions.1)")
            }
            stage = "another real minute at automatic display with wander on"
            try retainDisplay(arrived, enabled: true, since: firstDisplayAt, duration: 60)
            stage = "second bounded real automatic display after arrival dwell"
            var nextArrival: String?
            while seconds(firstDisplayAt) <= 95 {
                try requireSession()
                try require(candidate.alive(), "Candidate exited during next automatic dwell")
                let label = try markedChoice()
                if label != arrived {
                    let elapsed = seconds(firstDisplayAt)
                    try require(elapsed >= 60 && elapsed <= 95, "Next automatic display outside another60…95 wall seconds: \(elapsed)s")
                    try expectDisplay(label, enabled: true)
                    nextArrival = label
                    record("AUTOMATIC \(arrived)→\(label) at \(String(format: "%.3f", elapsed))s after first automatic mark; second real timer-driven display, unchanged exact\(revisions.0)/\(revisions.1)")
                    break
                }
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
            }
            guard let staying = nextArrival else {
                throw Failure("No second different automatic display within95 wall seconds after first automatic display")
            }
            let offAt = try command("menu off preserves latest automatic destination \(staying)", { try candidate.menu("ひとりで移る") },
                                    accepted: { try wanderState(false) })
            try expectDisplay(staying, enabled: false)
            stage = "off state retains automatic destination beyond a real minute"
            try retainDisplay(staying, enabled: false, since: offAt, duration: 65)
            try require(try timeValues(1, 1, 1) && abs(number(slider(window, label: "アプリの音量")) - volumeAfter) < 0.001,
                        "Automatic/off observations lost final time or volume settings")
            record("UI receipt/display and wall cadence only: no generated-sample counter, actual gain/arrival, PRNG uniformity, audible silence or physical-audio synchronization claim; no captures or virtual-time hooks")
            stage = "normal menu Quit"
            try candidate.menu("終了"); try candidate.expectExit()
            record("Real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
        }
    } catch let blocked as Blocked {
        throw Blocked("wandering stage=\(stage); \(blocked)")
    } catch {
        throw Failure("wandering stage=\(stage); \(error)")
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

func matchingSceneWindowIDs(_ candidate: Candidate, _ scene: AXUIElement) throws -> [CGWindowID] {
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
    return matches
}

func windowID(_ candidate: Candidate, _ scene: AXUIElement) throws -> CGWindowID {
    let matches = try matchingSceneWindowIDs(candidate, scene)
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
    var readyID: CGWindowID?
    try waitFor("owned scene opening animation reaches its exact AX bounds") {
        try requireSession()
        try require(try isForeground(candidate, scene) && bounds(scene) == initialBounds,
                    "Scene ownership or AX bounds changed while awaiting its CG frame")
        let matches = try matchingSceneWindowIDs(candidate, scene)
        try require(matches.count <= 1, "Ambiguous owned scene CG frames: \(matches)")
        readyID = matches.first
        return readyID != nil
    }
    guard let id = readyID else { throw Failure("Owned scene CG frame was not accepted") }
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

func presentation(_ executable: URL, helper: URL, artifacts: URL) throws -> String {
    guard CGPreflightScreenCaptureAccess() else {
        throw Blocked("Presentation requires own-window captures; no capture requested without authorization")
    }
    guard FileManager.default.fileExists(atPath: helper.path) else {
        throw Blocked("Screenshot helper missing at \(helper.path)")
    }
    let worldExpression = try NSRegularExpression(pattern: "受理通知時点の世界時刻 ([0-9]+(?:\\.[0-9]+)?) 秒")
    var evidence: [String] = []
    let began = DispatchTime.now().uptimeNanoseconds
    func elapsed(_ from: UInt64) -> Double {
        Double(DispatchTime.now().uptimeNanoseconds - from) / 1_000_000_000
    }
    func record(_ value: String) {
        let entry = String(format: "t+%.3fs ", elapsed(began)) + value
        evidence.append(entry)
        print("PRESENTATION \(entry)")
    }
    try withCandidate(executable, artifacts: artifacts) { candidate in
        var stage = "fresh owned presentation setup"
        do {
            let scene = try candidate.scene()
            let originalSize = try windowSize(scene)
            try candidate.menu("設定…"); try candidate.expectWindows(2)
            let settings = try candidate.settings()
            try foreground(candidate, settings)
            try expectReceipt(candidate, settings, settings: 0, manual: 0)
            try candidate.menu("消音")
            try expectReceipt(candidate, settings, settings: 1, manual: 0)
            try candidate.menu("ひとりで移る")
            try expectReceipt(candidate, settings, settings: 2, manual: 0)
            for (revision, label) in [(3, "最小滞在時間"), (4, "最大滞在時間"), (5, "自動切り替え時間")] {
                let control = try slider(settings, label: label)
                try visible(candidate, settings, control)
                try focus(control); try key(candidate, settings, code: 115)
                try waitFor("\(label) reaches production minimum1") { abs(try number(control) - 1) < 0.001 }
                try expectReceipt(candidate, settings, settings: revision, manual: 0)
            }
            let onAt = DispatchTime.now().uptimeNanoseconds
            try candidate.menu("ひとりで移る")
            try expectReceipt(candidate, settings, settings: 6, manual: 0)
            let choices = ["雨", "岩", "深海"]
            func displayed(_ revision: Int) throws -> (String, Double)? {
                try observeOwnedSettings(candidate, settings) { () throws -> (String, Double)? in
                    guard let main = try line(settings, prefix: "現在の設定:"),
                          let receipt = try line(settings, prefix: "音声側の受理:") else { return nil }
                    let m = try generations(main), r = try generations(receipt)
                    guard m == (revision, 0), r == (revision, 0), !receipt.contains("受理待ち"),
                          main.contains("/ ひとりで移る オン /"), main.contains("/ 消音 オン /") else { return nil }
                    guard let choice = choices.first(where: {
                        main.hasPrefix("現在の設定: \($0) /") && receipt.contains("表示対象 \($0)")
                    }) else { return nil }
                    var marks: [String] = []
                    for label in choices where try menuMarked(candidate, title: label) { marks.append(label) }
                    guard marks == [choice], try menuMarked(candidate) else { return nil }
                    for label in choices {
                        guard try number(segment(settings, label: label)) == (label == choice ? 1 : 0) else { return nil }
                    }
                    guard main == (try line(settings, prefix: "現在の設定:")),
                          receipt == (try line(settings, prefix: "音声側の受理:")) else { return nil }
                    let range = NSRange(receipt.startIndex..<receipt.endIndex, in: receipt)
                    guard let match = worldExpression.firstMatch(in: receipt, range: range),
                          let secondsRange = Range(match.range(at: 1), in: receipt),
                          let world = Double(receipt[secondsRange]), world.isFinite else {
                        throw Failure("Receipt lacks finite observed world timestamp: \(receipt)")
                    }
                    return (choice, world)
                } ?? nil
            }
            var initial: (String, Double)?
            try waitFor("coherent initial rain6/0 before scene minimize") {
                initial = try displayed(6)
                return initial?.0 == "雨"
            }
            stage = "minimized scene with actual automatic display, bound95s"
            try foreground(candidate, scene)
            try press(try attribute(scene, "AXMinimizeButton") as! AXUIElement)
            try waitFor("owned scene minimizes") { (try attribute(scene, "AXMinimized") as? Bool) == true }
            try foreground(candidate, settings)
            record("Scene minimized; settings remains available; on6/0 receipt world=\(initial!.1); UI dwell1/1min and transition1sec")
            var automatic: (String, Double)?
            while elapsed(onAt) <= 95 {
                try requireSession(); try require(candidate.alive(), "Candidate exited while scene minimized")
                try require((try attribute(scene, "AXMinimized") as? Bool) == true, "Scene restored during minimized observation")
                if let value = try displayed(6), value.0 != "雨" {
                    try require(elapsed(onAt) >= 60, "Coherent automatic display arrived before60 wall seconds")
                    try require(value.1 > initial!.1, "World did not progress while minimized")
                    automatic = value
                    record("Actual automatic 雨→\(value.0), unchanged6/0, world=\(value.1), \(elapsed(onAt)) wall seconds after on; scene still minimized")
                    break
                }
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
            }
            guard let automatic else { throw Failure("No coherent automatic display within95s while minimized") }
            let restored = AXUIElementSetAttributeValue(scene, "AXMinimized" as CFString, kCFBooleanFalse)
            try require(restored == .success, "Cannot restore owned scene")
            try waitFor("owned scene restores") { (try attribute(scene, "AXMinimized") as? Bool) == false }
            try require(try windowSize(scene) == originalSize, "Minimize changed fixed scene size")
            stage = "minimize restoration owned frames"
            evidence.append(try sceneFrames(candidate, scene, executable: executable, helper: helper,
                                            artifacts: artifacts, prefix: "presentation-minimize"))

            stage = "owned opaque full-occlusion fixture"
            try foreground(candidate, scene)
            let rect = try bounds(scene)
            guard let mainScreen = NSScreen.screens.first else { throw Failure("No actual primary screen for AppKit coordinates") }
            // This runner-owned blank window covers only the candidate's bounds;
            // it never receives input, captures another app, or replaces GPUI.
            NSApplication.shared.setActivationPolicy(.accessory)
            let overlay = NSWindow(contentRect: CGRect(x: rect.minX, y: mainScreen.frame.maxY - rect.maxY,
                                                       width: rect.width, height: rect.height),
                                   styleMask: [.borderless], backing: .buffered, defer: false)
            overlay.isReleasedWhenClosed = false
            overlay.isOpaque = true; overlay.backgroundColor = .black
            overlay.hasShadow = false; overlay.ignoresMouseEvents = true
            overlay.level = .floating
            overlay.orderFrontRegardless()
            defer { overlay.orderOut(nil); overlay.close() }
            try waitFor("runner-owned opaque cover is visible") { overlay.isVisible }
            let coverAt = DispatchTime.now().uptimeNanoseconds
            while elapsed(coverAt) < 3 {
                try requireSession(); try require(candidate.alive() && overlay.isVisible, "Owned occlusion fixture disappeared")
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
            }
            overlay.orderOut(nil)
            record("Runner-owned noninteractive opaque cover held candidate bounds3s; removed without input or capture")

            stage = "hide and re-show the owned application"
            try foreground(candidate, scene)
            guard let running = NSRunningApplication(processIdentifier: candidate.pid) else { throw Failure("Owned app disappeared") }
            var hiddenSettable = DarwinBoolean(false)
            try require(AXUIElementIsAttributeSettable(candidate.app, "AXHidden" as CFString, &hiddenSettable) == .success &&
                        hiddenSettable.boolValue, "Owned application AXHidden is not settable")
            try require(AXUIElementSetAttributeValue(candidate.app, "AXHidden" as CFString, kCFBooleanTrue) == .success,
                        "Cannot set owned application AXHidden")
            try waitFor("owned application hides") {
                try (attribute(candidate.app, "AXHidden") as? Bool) == true && running.isHidden
            }
            let hideAt = DispatchTime.now().uptimeNanoseconds
            while elapsed(hideAt) < 3 {
                try requireSession(); try require(candidate.alive() && running.isHidden, "Owned application became visible during hide")
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
            }
            try require(AXUIElementSetAttributeValue(candidate.app, "AXHidden" as CFString, kCFBooleanFalse) == .success,
                        "Cannot clear owned application AXHidden")
            try waitFor("owned application unhides") {
                try (attribute(candidate.app, "AXHidden") as? Bool) == false && !running.isHidden
            }
            stage = "hide restoration owned frames"
            evidence.append(try sceneFrames(candidate, scene, executable: executable, helper: helper,
                                            artifacts: artifacts, prefix: "presentation-unhide"))

            stage = "closed scene preserves world and wander"
            try press(try attribute(scene, "AXCloseButton") as! AXUIElement)
            try candidate.expectWindows(1)
            let closeAt = DispatchTime.now().uptimeNanoseconds
            while elapsed(closeAt) < 2 {
                try requireSession(); try require(candidate.alive(), "Scene close stopped the app")
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.1))
            }
            let transition = try slider(settings, label: "自動切り替え時間")
            try visible(candidate, settings, transition)
            try focus(transition); try key(candidate, settings, code: 124)
            try expectReceipt(candidate, settings, settings: 7, manual: 0)
            var afterClose: (String, Double)?
            try waitFor("time edit after scene close retains latest target/wander and advancing world") {
                afterClose = try displayed(7)
                return afterClose != nil && (afterClose?.1 ?? 0) > automatic.1 + 1
            }
            record("Scene closed; actual time1→2 edit receipt7/0, wander/mute on, latest \(afterClose!.0), world=\(afterClose!.1); no scene window")
            try candidate.menu("シーンを表示"); try candidate.expectWindows(2)
            let reopened = try candidate.scene()
            try require(try windowSize(reopened) == originalSize, "Recreated scene changed fixed size")
            stage = "close restoration owned frames"
            evidence.append(try sceneFrames(candidate, reopened, executable: executable, helper: helper,
                                            artifacts: artifacts, prefix: "presentation-reopen"))
            record("Three restoration sets each own PNG3/two changed interior pairs; image/UI only, not old particle identity, task/fps measurement, audible continuity or physical sync")
            try candidate.menu("終了"); try candidate.expectExit()
            record("Real menu Quit; owned PID \(candidate.pid) disappeared normal exit0")
        } catch let blocked as Blocked {
            throw Blocked("presentation stage=\(stage); \(blocked); partial evidence:\n\(evidence.joined(separator: "\n"))")
        } catch {
            throw Failure("presentation stage=\(stage); \(error); partial evidence:\n\(evidence.joined(separator: "\n"))")
        }
    }
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

func persistence(_ executable: URL, artifacts: URL) throws -> String {
    var evidence: [String] = []
    let manager = FileManager.default
    let unknown: [String: Any] = ["nested": [["value": 7]]]
    let initial: [String: Any] = [
        "version": 1, "volume": 0.8, "mute": true, "wander": false,
        "wander_min_minutes": 20, "wander_max_minutes": 10, "transition_seconds": 2.6,
        "last": "rock",
        "window": ["x": 200, "y": 100, "width": 9999, "height": 9999, "display_uuid": NSNull()],
        "situations": [
            "rain": ["amount": -2, "wind": 2, "removed_knob": ["kept": true]],
            "rock": ["amount": 0.9, "size": 0.25, "hardness": 0.1],
            "removed_situation": ["kept": 7],
        ],
        "retained_unknown": unknown,
    ]
    func encoded(_ object: Any) throws -> Data {
        try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    }
    func fixture(_ name: String, _ bytes: Data) throws -> (URL, URL, URL) {
        let home = artifacts.appendingPathComponent("persistence-\(name)", isDirectory: true)
        try require(mkdir(home.path, 0o700) == 0, "Cannot create owned persistence HOME: errno=\(errno)")
        let directory = home.appendingPathComponent("Library/Application Support/Ambien", isDirectory: true)
        try manager.createDirectory(at: directory, withIntermediateDirectories: true,
                                    attributes: [.posixPermissions: 0o700])
        let state = directory.appendingPathComponent("state.json")
        try privateFile(bytes, at: state)
        return (home, state, directory)
    }
    func document(_ state: URL) throws -> [String: Any] {
        guard let value = try JSONSerialization.jsonObject(with: Data(contentsOf: state)) as? [String: Any] else {
            throw Failure("Saved document is not an object")
        }
        return value
    }
    func scalar(_ value: Any?) -> Double { (value as? NSNumber)?.doubleValue ?? .nan }
    func retained(_ value: [String: Any], times: (Double, Double, Double) = (1, 1, 1)) throws {
        guard let extra = value["retained_unknown"],
              let situations = value["situations"] as? [String: Any],
              let rain = situations["rain"] as? [String: Any],
              let removed = situations["removed_situation"] as? [String: Any] else {
            throw Failure("Unknown document/situation fields were removed")
        }
        try require(try encoded(extra) == encoded(unknown) &&
                    scalar(removed["kept"]) == 7 && rain["removed_knob"] is [String: Any],
                    "Unknown nested values were changed")
        try require(scalar(rain["amount"]) == 0 && scalar(rain["wind"]) == 1,
                    "Known saved knobs were not clamped")
        try require(scalar(value["wander_min_minutes"]) == times.0 &&
                    scalar(value["wander_max_minutes"]) == times.1 &&
                    scalar(value["transition_seconds"]) == times.2,
                    "Saved normalized/edited time endpoints did not round-trip")
    }
    func restored(_ candidate: Candidate, _ window: AXUIElement,
                  target: String, volume: Double, muted: Bool, point: CGPoint,
                  times: (Double, Double, Double) = (1, 1, 1), wandering: Bool = false) throws {
        try expectReceipt(candidate, window, settings: 0, manual: 0)
        try waitFor("restored production controls and menu") {
            try observeOwnedSettings(candidate, window) {
                guard let main = try line(window, prefix: "現在の設定:") else { return false }
                return try main.hasPrefix("現在の設定: \(target) /") &&
                    abs(number(slider(window, label: "アプリの音量")) - volume) < 0.001 &&
                    number(mute(window)) == (muted ? 1 : 0) &&
                    menuMarked(candidate) == muted &&
                    menuMarked(candidate, title: "ひとりで移る") == wandering &&
                    abs(number(slider(window, label: "最小滞在時間")) - times.0) < 0.001 &&
                    abs(number(slider(window, label: "最大滞在時間")) - times.1) < 0.001 &&
                    abs(number(slider(window, label: "自動切り替え時間")) - times.2) < 0.001
            } == true
        }
        let scene = try candidate.scene()
        try require(try windowSize(scene) == CGSize(width: 960, height: 600),
                    "Saved arbitrary dimensions replaced the current fixed scene size")
        let actual = try position(scene)
        try require(abs(actual.x - point.x) < 1 && abs(actual.y - point.y) < 1,
                    "Saved native scene position not restored: \(actual), expected \(point)")
        evidence.append("pid=\(candidate.pid), restored \(target), volume=\(volume), mute=\(muted), times=\(times), wander=\(wandering), scene=\(try bounds(scene)), fresh receipt0/0")
    }
    let worldExpression = try NSRegularExpression(pattern: "受理通知時点の世界時刻 ([0-9]+(?:\\.[0-9]+)?) 秒")
    func world(_ window: AXUIElement) throws -> Double {
        guard let receipt = try line(window, prefix: "音声側の受理:"),
              let match = worldExpression.firstMatch(in: receipt, range: NSRange(receipt.startIndex..<receipt.endIndex, in: receipt)),
              let range = Range(match.range(at: 1), in: receipt), let value = Double(receipt[range]) else {
            throw Failure("Current audio world receipt is missing")
        }
        return value
    }
    let (home, state, _) = try fixture("roundtrip", encoded(initial))
    try withCandidate(executable, artifacts: artifacts, home: home) { candidate in
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let window = try candidate.settings()
        try restored(candidate, window, target: "岩", volume: 80, muted: true,
                     point: CGPoint(x: 200, y: 100), times: (10, 20, 3))
        try foreground(candidate, window)
        try focus(slider(window, label: "アプリの音量")); try key(candidate, window, code: 115)
        try expectReceipt(candidate, window, settings: 1, manual: 0)
        try focus(slider(window, label: "大きさ")); try key(candidate, window, code: 115)
        try expectReceipt(candidate, window, settings: 2, manual: 1)
        for (offset, label) in ["最小滞在時間", "最大滞在時間", "自動切り替え時間"].enumerated() {
            try focus(slider(window, label: label)); try key(candidate, window, code: 115)
            try expectReceipt(candidate, window, settings: 3 + offset, manual: 1)
        }
        try candidate.menu("消音"); try expectReceipt(candidate, window, settings: 6, manual: 1)
        var point = CGPoint(x: 250, y: 150)
        guard let value = AXValueCreate(.cgPoint, &point) else { throw Failure("Cannot create owned scene position") }
        try require(AXUIElementSetAttributeValue(try candidate.scene(), "AXPosition" as CFString, value) == .success,
                    "Cannot move the owned scene")
        try waitFor("actual settled state.json save") {
            let value = try document(state)
            guard let situations = value["situations"] as? [String: Any],
                  let rock = situations["rock"] as? [String: Any],
                  let geometry = value["window"] as? [String: Any] else { return false }
            return scalar(value["volume"]) == 0 && value["mute"] as? Bool == false &&
                scalar(rock["size"]) == 0 && scalar(geometry["x"]) == 250 &&
                scalar(geometry["y"]) == 150 && scalar(geometry["width"]) == 960 &&
                scalar(geometry["height"]) == 600 && geometry["display_uuid"] is String
        }
        try retained(document(state))
        try candidate.menu("終了"); try candidate.expectExit()
        evidence.append("Settled production edit/position save, independent mute=false at volume0, unknown nested fields retained; normal menu Quit pid=\(candidate.pid)")
    }
    try withCandidate(executable, artifacts: artifacts, home: home) { candidate in
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let window = try candidate.settings()
        try restored(candidate, window, target: "岩", volume: 0, muted: false, point: CGPoint(x: 250, y: 150))
        try require(try number(slider(window, label: "大きさ")) == 0, "Saved rock knob did not restore")
        try candidate.menu("消音"); try expectReceipt(candidate, window, settings: 1, manual: 0)
        try foreground(candidate, window)
        try focus(slider(window, label: "アプリの音量")); try key(candidate, window, code: 124)
        try expectReceipt(candidate, window, settings: 2, manual: 0)
        try candidate.menu("深海"); try expectReceipt(candidate, window, settings: 3, manual: 1)
        try candidate.menu("終了"); try candidate.expectExit()
        let value = try document(state)
        try require(value["last"] as? String == "deep" &&
                    abs(scalar(value["volume"]) - 0.01) < 0.000001 && value["mute"] as? Bool == true,
                    "Quit did not flush the latest endpoint/manual target")
        try retained(value)
        evidence.append("Immediate normal Quit during latest explicit 岩→深海 crossing flushed volume1%/mute=true/last=deep; pid=\(candidate.pid)")
    }
    try withCandidate(executable, artifacts: artifacts, home: home) { candidate in
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let window = try candidate.settings()
        try restored(candidate, window, target: "深海", volume: 1, muted: true, point: CGPoint(x: 250, y: 150))
        try retained(document(state))
        let started = DispatchTime.now().uptimeNanoseconds
        try candidate.menu("ひとりで移る")
        try expectReceipt(candidate, window, settings: 1, manual: 0)
        var automatic: (String, String)?
        let choices = ["rain": "雨", "rock": "岩"]
        while Double(DispatchTime.now().uptimeNanoseconds - started) / 1_000_000_000 <= 95 {
            try requireSession()
            try require(candidate.alive(), "Candidate exited while waiting for automatic saved last")
            if let saved = try document(state)["last"] as? String, let label = choices[saved] {
                let coherent = try observeOwnedSettings(candidate, window) {
                    guard let main = try line(window, prefix: "現在の設定:"),
                          let receipt = try line(window, prefix: "音声側の受理:") else { return false }
                    return try main.hasPrefix("現在の設定: \(label) /") &&
                        receipt.contains("表示対象 \(label)") &&
                        generations(main) == (1, 0) && generations(receipt) == (1, 0) &&
                        menuMarked(candidate, title: label) && menuMarked(candidate, title: "ひとりで移る")
                } == true
                if coherent {
                    automatic = (saved, label)
                    break
                }
            }
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.2))
        }
        guard let automatic else { throw Failure("Automatic current-excluding last was not saved within95 wall seconds") }
        let elapsed = Double(DispatchTime.now().uptimeNanoseconds - started) / 1_000_000_000
        try require(elapsed >= 60, "Automatic saved last appeared before the production minimum dwell")
        try retained(document(state))
        evidence.append("Automatic 深海→\(automatic.1), actual state.json last=\(automatic.0), unchanged receipt1/0, audio world=\(try world(window)), \(elapsed) wall seconds; no time-shortening hook")
        try candidate.menu("終了"); try candidate.expectExit()
        evidence.append("Automatic saved target normal Quit pid=\(candidate.pid)")
    }
    guard let savedAutomatic = try document(state)["last"] as? String,
          let automaticLabel = ["rain": "雨", "rock": "岩"][savedAutomatic] else {
        throw Failure("Saved automatic target is not registered/current-excluding")
    }
    try withCandidate(executable, artifacts: artifacts, home: home) { candidate in
        try candidate.menu("設定…"); try candidate.expectWindows(2)
        let window = try candidate.settings()
        try restored(candidate, window, target: automaticLabel, volume: 1, muted: true,
                     point: CGPoint(x: 250, y: 150), wandering: true)
        try require(try world(window) < 5, "Restart replayed the old world clock instead of fresh staying")
        try retained(document(state))
        try candidate.menu("終了"); try candidate.expectExit()
    }
    var future = initial
    future["version"] = 2
    let corrupt = Data("{broken owned state".utf8)
    let fixtures: [(String, Data, Bool, Bool, Bool)] = [
        ("future", try encoded(future), false, false, false),
        ("corrupt", corrupt, false, false, true),
        ("unreadable", try encoded(initial), false, true, true),
        ("backup-failed", corrupt, true, false, false),
        ("save-failed", try encoded(initial), true, false, false),
    ]
    for (name, original, readonlyParent, unreadable, backupExpected) in fixtures {
        let (fixtureHome, fixtureState, directory) = try fixture(name, original)
        if unreadable { try require(chmod(fixtureState.path, 0o000) == 0, "Cannot protect owned unreadable fixture") }
        if readonlyParent { try require(chmod(directory.path, 0o500) == 0, "Cannot protect owned readonly-parent fixture") }
        defer {
            if readonlyParent && chmod(directory.path, 0o700) != 0 { fputs("Cannot restore owned fixture directory mode\n", stderr) }
        }
        try withCandidate(executable, artifacts: artifacts, home: fixtureHome) { candidate in
            try candidate.menu("設定…"); try candidate.expectWindows(2)
            let window = try candidate.settings()
            var revision = 0
            if try !menuMarked(candidate) {
                try candidate.menu("消音"); revision += 1
            }
            try expectReceipt(candidate, window, settings: revision, manual: 0)
            let before = try world(window)
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 1.1))
            try foreground(candidate, window)
            try focus(slider(window, label: "アプリの音量")); try key(candidate, window, code: 115)
            revision += 1
            try expectReceipt(candidate, window, settings: revision, manual: 0)
            let after = try world(window)
            try require(after > before + 0.8, "Audio world stopped across protected load/save failure")
            let prefix = backupExpected ? "保存状態:" : "状態を保存できません:"
            var notice: String?
            try waitFor("explicit settings and actionable-menu persistence notice") {
                try observeOwnedSettings(candidate, window) {
                    guard let value = try line(window, prefix: prefix) else { return false }
                    let raw = try attribute(candidate.app, "AXMenuBar")
                    guard let item = try findMenu(raw as! AXUIElement, title: value) else { return false }
                    try require((try attribute(item, "AXEnabled") as? Bool) == true,
                                "Persistence notice cannot open settings details")
                    notice = value
                    return true
                } == true
            }
            guard let notice else { throw Failure("Persistence notification was not retained") }
            try candidate.menu(notice)
            try candidate.expectWindows(2)
            try require(CFEqual(try candidate.settings(), window),
                        "Persistence notification duplicated the settings window")
            try require(try number(mute(window)) == 1 && candidate.alive(),
                        "Persistence failure changed mute or terminated audio host")
            if name == "save-failed" {
                // A genuine write failure must keep the app, its sound and the failure notice.
                try candidate.menu("終了")
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 1.5))
                try require(candidate.alive(), "Quit terminated the app despite a failed final save")
                try require(CFEqual(try candidate.settings(), window), "Settings window lost after refused Quit")
                let refusedAt = try world(window)
                // Editing resumed: an accepted edit refreshes the displayed receipt, whose
                // world time shows the audio kept running through the refused Quit.
                try foreground(candidate, window)
                try focus(slider(window, label: "アプリの音量")); try key(candidate, window, code: 124)
                revision += 1
                try expectReceipt(candidate, window, settings: revision, manual: 0)
                try require(try world(window) > refusedAt + 0.3, "Audio stopped after the refused Quit")
                try require(try line(window, prefix: prefix) != nil, "Failure notice vanished after the refused Quit")
                try require(chmod(directory.path, 0o700) == 0, "Cannot restore owned fixture directory mode")
            }
            try candidate.menu("終了"); try candidate.expectExit()
            evidence.append("\(name): actionable UI/menu=\(notice); audio world \(before)→\(after), accepted revision\(revision)/0, menu Quit pid=\(candidate.pid)")
        }
        if backupExpected {
            let files = try manager.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
                .filter { $0.lastPathComponent != "state.json" }
            try require(files.count == 1, "Expected one protected original backup, got \(files)")
            let backup = files[0].appendingPathComponent("state.json")
            try require(try manager.contentsOfDirectory(atPath: files[0].path) == ["state.json"],
                        "Unique backup reservation does not contain exactly the original")
            if unreadable { try require(chmod(backup.path, 0o600) == 0, "Cannot inspect owned unreadable backup") }
            try require(try Data(contentsOf: backup) == original, "Original backup bytes changed")
            try require((try document(fixtureState))["version"] as? Int == 1, "Writable defaults were not saved after backup")
            evidence.append("\(name): original byte-identical in \(files[0].lastPathComponent)/state.json, new version1 state saved")
        } else if name == "save-failed" {
            // Quit was refused while the write failed. After the user-visible failure is gone the
            // same Quit saved the final state; the old bytes are intentionally replaced.
            let saved = try document(fixtureState)
            try require(saved["version"] as? Int == 1 && (saved["mute"] as? Bool) == true,
                        "Retried Quit did not save the final state after the write failure cleared")
            try require(try Data(contentsOf: fixtureState) != original, "Final state was not written")
            evidence.append("\(name): Quit refused during the genuine write failure (alive, notice kept, audio world progressing); after restoring the owned directory mode the same menu Quit saved version1 state and exited normally")
        } else {
            try require(try Data(contentsOf: fixtureState) == original, "Protected original was overwritten")
            evidence.append("\(name): original state.json remains byte-identical")
        }
    }
    return evidence.joined(separator: "\n")
}

let usage = """
Usage: /usr/bin/swift scripts/native-e2e.swift /absolute/path/to/ambien [--case lifecycle|controls|situations|deep|mixing|wandering|presentation|persistence|visual|all] [--artifacts PARENT_DIR]
       /usr/bin/swift scripts/native-e2e.swift --help

Default case: all. Each run creates a unique private directory (0700) under
PARENT_DIR (default target/native-e2e), with report.json (0600), candidate logs,
isolated private HOME trees and authorized own-window PNG/helper measurements.
Persistence alone reuses its own HOME across restarts; user state is never used.
No permission prompts, desktop captures, image uploads, or production test hooks.

Exit: 0 every selected case passed; 1 any failure; 77 prerequisites blocked
when no failure. Missing capture permission blocks visual/situations/deep/mixing/presentation image stages;
independent lifecycle/controls/wandering still run. Null lock metadata alone is not unlocked proof.
Visual evidence is rendered-frame change while muted, not audible silence,
particle identity, physical synchronization, or subjective glass/music.
Mixing verifies rapid real commands and exact accepted-control revisions, not
actual gains, maximum-three RT work, re-entry audio tails, waveform bounds or clicks.
Wandering uses production UI only, muted, dwell1/1min and transition1sec; it
allows at most95s for each of two real automatic display changes, observing
another60s with wander on after the first, then65s staying off at the latest
destination. These are case-local wall-clock bounds, not generated-sample
timing, gain, PRNG uniformity or physical-sync proof.
No capture permission is needed for wandering; retain an awake active output.
Presentation keeps the scene minimized during one real automatic display change,
then exercises an owned opaque full-cover fixture, application hide/show and
scene close/recreation. Each of three restoration stages captures own PNG3/two
interior comparisons. UI/world receipts and images do not measure scene task/fps,
old particle identity, audible continuity, RT or physical synchronization.
Persistence verifies the production state.json path in isolated HOME: restored
controls and fixed scene position, actual settled saves and final Quit flush,
unknown-field retention, normalization, protected future/corrupt/unreadable files
and real create-temp/backup failures with UI/menu notices and progressing audio
receipts. It does not establish physical silence, subjective quality or update/
quit-veto choreography, which has a separate lifecycle acceptance gate.
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
                try require(["lifecycle", "controls", "situations", "deep", "mixing", "wandering", "presentation", "persistence", "visual", "all"].contains(value), "Unknown case \(value)\n\(usage)")
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
    for selected in ["lifecycle", "controls", "situations", "deep", "mixing", "wandering", "presentation", "persistence", "visual"] where options.selected == "all" || options.selected == selected {
        let expected: String
        switch selected {
        case "lifecycle":
            expected = "Real menus, same settings singleton, minimize/restore and disabled zoom on both windows, settings Cmd-comma recreation, scene close/zero-window/menu recreation, menu Quit and foreground Cmd-Q from both windows with real PID disappearance/normal0"
        case "controls":
            expected = "P03 current rain metadata; exact main/audio settings+manual receipts for selected Space/Enter/mouse, real rain/wind edits, menu mute/mark, muted volume0/10/20, real scroll with unchanged revisions/focus/AX identity, settings unmute latest20%, close/recreate retaining main controls, menu Quit normal0"
        case "situations":
            expected = "Registered rock menu/settings and owned labeled radio choices; exact accepted target revisions and menu marks; rock amount/size/hardness endpoints; settings pointer rain→rock latest selection; muted volume0/10/20 then unmute latest20 and remute; focused-other rain Space and rock Return with exact target receipts/radio/menu/main; real size100/hardness0 edits for large/soft muted capture latest16/10; three owned muted endpoint PNGs/two interior-change pairs; actual scene menu rain then rock return immediately initiating another three owned muted PNGs/two pairs; menu Quit normal0. Not actual-gain, audible, particle-only, exact crossing timing/brightness or physical-sync proof."
        case "deep":
            expected = "Registered deep and all three actual menu/settings radio choices; main/audio accepted targets and exclusive menu/radio marks while muted; menu mute1/0, deep2/1; depth Home0 exactly3/2 and air Home0 exactly4/3; air End100 exactly5/4 and depth End100 exactly6/5; menu rain7/6 then rock8/7; settings rain9/8, rock10/9, deep11/10 retaining depth100/air100; three owned muted deep PNGs/two cropped-interior-change pairs; menu Quit normal0. Candidate initials unpinned; receipts not actual-gain, audible continuity, bubble-frequency, darkness, speed or physical-sync proof."
        case "mixing":
            expected = "Fresh owned candidate; all three actual menu/radio choices; mute1/0; rapid menu rain2/1, rock3/2, deep4/3, rock5/4, deep6/5, selected deep7/6 with command-start intervals <2s (input cadence only); settings pointer rain8/7, rock9/8, deep10/9, selected deep11/10; observed depth one-step12/11; muted volume0 13/11,10 14/11,20 15/11,19 16/11,18 17/11; settings unmute18/11, volume19 19/11,20 20/11; menu remute21/11 and selected deep22/12; three owned mixing-muted PNGs/two cropped-interior-change pairs with crossing-or-live-frame limits; reopened latest deep/depth/20%/mute exact22/12; menu Quit normal0. Not actual gains, maximum-three RT work, audio re-entry, waveform bounds, no-click or physical-sync proof."
        case "wandering":
            expected = "Fresh owned candidate; actual menu mute; visible focused production time sliders and exact next-application descriptions; paired min/max correction both directions; time/volume/mute preserve wander/manual revision; menu/settings on/off, explicit menu/settings selection and knob turn disable wander; final dwell1/1min/transition1sec and staying off→on starts fresh; within95 wall seconds observe a different registered automatic menu/settings/audio-receipt display, not before60 wall seconds; retain that display another60s on and require a second actual automatic change within95s of the first, then65s off at latest destination while muted; observed monotonic main revisions and exact accepted receipts recorded per real command; menu Quit normal0. No capture prerequisite; not generated-sample count, actual gains, uniform PRNG, physical-audio synchronization or audible proof."
        case "presentation":
            expected = "Production focused time sliders set dwell1/1min and transition1sec, muted wander on6/0; keep owned scene minimized until a coherent current-excluding automatic menu/radio/main/audio display within60..95 wall seconds; unchanged manual0 and progressing receipt world; restore same fixed scene and PNG3/two cropped-interior changes; runner-owned noninteractive opaque full-cover3s, own app hide3s/unhide and second PNG3/two pairs; scene close leaves settings/world/wander alive, actual time1→2 receipt7/0 latest retained; recreate fixed scene and third PNG3/two pairs; real menu Quit normal0. Not generated-sample counts, old particle identity, task/fps measurement, audible continuity, RT or physical synchronization."
        case "persistence":
            expected = "Isolated private HOME only; four normal roundtrip launches restore normalized/edited times, independent mute/volume, saved knobs and fixed scene position; settled save and immediate Quit flush retain unknown fields/latest explicit crossing last; real >=60..95s automatic last agrees with coherent main/audio1/0 and is restored with wander/fresh world; future-version and failed-backup documents remain byte-identical; corrupt/unreadable backups retain original bytes in the unique reservation's state.json leaf; genuine readonly-parent create-temp failure is visible in actionable menu/settings notices while audio receipts/world progress; all nine candidates menu Quit normal0."
        default:
            expected = "Authorized actual owned scene-window PNGs; real menu mute; stable CG owner/bounds; three spaced captures; two cropped-interior nonblank/nontransparent pixel-change comparisons; retained PNGs and measurements; menu Quit normal0"
        }
        checks.record(selected, expected: expected) {
            guard nativeReady else { throw Blocked("Not executed: binary/session prerequisites did not pass; see preceding checks") }
            switch selected {
            case "lifecycle": return try lifecycle(options.executable, artifacts: artifacts)
            case "controls": return try controls(options.executable, artifacts: artifacts)
            case "situations": return try situations(options.executable, helper: helper, artifacts: artifacts)
            case "deep": return try deep(options.executable, helper: helper, artifacts: artifacts)
            case "mixing": return try mixing(options.executable, helper: helper, artifacts: artifacts)
            case "wandering": return try wandering(options.executable, artifacts: artifacts)
            case "presentation": return try presentation(options.executable, helper: helper, artifacts: artifacts)
            case "persistence": return try persistence(options.executable, artifacts: artifacts)
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
