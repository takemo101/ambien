import AppKit
import ApplicationServices
import CoreGraphics
import Darwin
import Foundation

// External native UI tests: no in-app dispatch hooks, capture, or permission prompts.
struct Failure: Error, CustomStringConvertible {
    let description: String
    init(_ description: String) { self.description = description }
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
    guard result == .success, let value else {
        throw Failure("AX \(name): \(result.rawValue)")
    }
    return value
}

func children(_ element: AXUIElement) throws -> [AXUIElement] {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, "AXChildren" as CFString, &value)
    if result == .noValue || result == .attributeUnsupported { return [] }
    guard result == .success, let result = value as? [AXUIElement] else {
        throw Failure("AX children: \(result.rawValue)")
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
    private let errorsURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("ambien-native-e2e-\(UUID().uuidString).log")
    private let errors: FileHandle

    init(executable: URL) throws {
        let fd = open(errorsURL.path, O_RDWR | O_CREAT | O_EXCL, 0o600)
        guard fd >= 0 else { throw Failure("Cannot create private candidate stderr: errno=\(errno)") }
        errors = FileHandle(fileDescriptor: fd, closeOnDealloc: true)
        process.executableURL = executable
        process.standardError = errors
        do {
            try process.run()
        } catch {
            do { try errors.close() } catch { fputs("Cleanup stderr close: \(error)\n", stderr) }
            do { try FileManager.default.removeItem(at: errorsURL) } catch { fputs("Cleanup stderr file: \(error)\n", stderr) }
            throw error
        }
        pid = process.processIdentifier
        app = AXUIElementCreateApplication(pid)
    }

    deinit {
        do { try errors.close() } catch { fputs("Cleanup stderr close: \(error)\n", stderr) }
        do { try FileManager.default.removeItem(at: errorsURL) } catch { fputs("Cleanup stderr file: \(error)\n", stderr) }
    }

    func diagnostics() throws -> String {
        String(decoding: try Data(contentsOf: errorsURL), as: UTF8.self)
    }

    func alive() -> Bool { kill(pid, 0) == 0 }

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
        process.waitUntilExit()
        let log = try diagnostics()
        try require(process.terminationReason == .exit && process.terminationStatus == 0,
                    "Candidate abnormal exit: \(process.terminationStatus)\n\(log)")
        try require(!log.contains("Ambien:") && !log.contains("panicked"), "Candidate errors:\n\(log)")
        if !log.isEmpty { print("Candidate stderr pid=\(pid):\n\(log)") }
    }

    func cleanupAfterFailure() {
        // Cleanup is not a successful Quit assertion. Never force-kill another process.
        if process.isRunning { process.terminate() }
        do {
            try waitFor("owned candidate cleanup pid=\(pid)") { !process.isRunning }
            process.waitUntilExit()
        } catch { fputs("Candidate cleanup failed pid=\(pid): \(error)\n", stderr) }
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

func withCandidate(_ executable: URL, _ scenario: (Candidate) throws -> Void) throws {
    let candidate = try Candidate(executable: executable)
    do {
        try candidate.waitForStartup()
        try scenario(candidate)
    } catch {
        candidate.cleanupAfterFailure()
        // Preserve the original scenario failure even if diagnostic collection fails.
        do {
            let log = try candidate.diagnostics()
            if !log.isEmpty { fputs("Candidate stderr pid=\(candidate.pid):\n\(log)", stderr) }
        } catch { fputs("Cannot read candidate stderr: \(error)\n", stderr) }
        throw error
    }
}

setbuf(stdout, nil)
do {
    try require(CommandLine.arguments.count == 2, "Usage: swift scripts/native-e2e.swift /absolute/path/to/ambien")
    let executable = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
    try require(executable.lastPathComponent == "ambien" && FileManager.default.isExecutableFile(atPath: executable.path),
                "Pass the built ambien executable, not another application")
    try require(AXIsProcessTrusted(), "Accessibility unavailable; no permission prompt requested")
    try require(CGPreflightPostEventAccess(), "Event posting unavailable; no permission prompt requested")
    try require(AXUIElementSetMessagingTimeout(AXUIElementCreateSystemWide(), 1) == .success,
                "Cannot bound AX messages globally for this runner process")

    try withCandidate(executable) { candidate in
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
        try withCandidate(executable) { candidate in
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
    print("PASS native E2E completed; no screenshot or OS settings modified")
} catch {
    fputs("FAIL native E2E: \(error)\n", stderr)
    exit(1)
}
