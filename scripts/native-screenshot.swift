import CoreGraphics
import Darwin
import Foundation
import ImageIO
import ScreenCaptureKit
import UniformTypeIdentifiers

// Standalone, local-only checker. Self-tests verify this checker, never the app.
func selfTest() throws -> [String: Any] {
    func fixture(width: Int = 64, height: Int = 64, flat: Bool = false,
                 transparent: Bool = false, changed: Bool = false,
                 borderOnly: Bool = false, monochromeShape: Bool = false) throws -> CGImage {
        var bytes = [UInt8](repeating: 0, count: width * height * 4)
        for y in 0..<height {
            for x in 0..<width {
                let offset = (y * width + x) * 4
                bytes[offset] = flat ? 12 : UInt8(30 + (x * 3 + y) % 120)
                bytes[offset + 1] = flat ? 12 : UInt8(40 + (y * 2) % 120)
                bytes[offset + 2] = flat ? 12 : 80
                bytes[offset + 3] = transparent ? 0 : 255
                if changed && (24..<32).contains(x) && (24..<32).contains(y) {
                    bytes[offset] = 240
                }
                if borderOnly && (x < 3 || y < 3) { bytes[offset] = 240 }
                if monochromeShape {
                    bytes[offset] = 120
                    bytes[offset + 1] = 80
                    bytes[offset + 2] = 20
                    bytes[offset + 3] = (24..<32).contains(x) && (24..<32).contains(y)
                        ? (changed ? 128 : 64) : 0
                }
            }
        }
        guard let provider = CGDataProvider(data: Data(bytes) as CFData),
              let space = CGColorSpace(name: CGColorSpace.sRGB),
              let image = CGImage(width: width, height: height, bitsPerComponent: 8,
                                  bitsPerPixel: 32, bytesPerRow: width * 4, space: space,
                                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue)
                                      .union(.byteOrder32Big), provider: provider, decode: nil,
                                  shouldInterpolate: false, intent: .defaultIntent) else {
            throw Problem("self_test_fixture", "Cannot make deterministic pixel fixture")
        }
        // Exercise actual PNG encoding, ImageIO decoding, and pixel normalization too.
        let png = try pngData(image)
        guard let source = CGImageSourceCreateWithData(png as CFData, nil),
              let decoded = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw Problem("self_test_fixture", "Cannot decode deterministic PNG fixture")
        }
        return decoded
    }

    let original = try fixture()
    let cases: [(String, CGImage, CGImage, String, String, Int?)] = [
        ("unchanged", original, original, "fail", "unchanged", 0),
        ("different", original, try fixture(changed: true), "pass", "pixels_changed", 64),
        ("blank", try fixture(flat: true), try fixture(flat: true), "fail", "blank_image", nil),
        ("transparent", try fixture(transparent: true), try fixture(transparent: true),
         "fail", "transparent_image", nil),
        ("dimension_mismatch", original, try fixture(width: 65),
         "fail", "dimension_mismatch", nil),
        ("chrome_only", original, try fixture(borderOnly: true), "fail", "unchanged", 0),
        ("monochrome_transparent_shape", try fixture(monochromeShape: true),
         try fixture(changed: true, monochromeShape: true), "pass", "pixels_changed", 64),
    ]
    var checks: [[String: Any]] = []
    for (id, first, second, status, reason, changedPixels) in cases {
        let observed = try compareImages(first, second)
        let matches = observed["status"] as? String == status
            && observed["reason"] as? String == reason
            && (changedPixels == nil || observed["changed_pixels"] as? Int == changedPixels)
        checks.append(["id": id, "status": matches ? "pass" : "fail",
                       "expected_status": status, "expected_reason": reason,
                       "observed": observed])
    }
    let passed = checks.allSatisfy { $0["status"] as? String == "pass" }
    return ["status": passed ? "pass" : "fail", "operation": "self-test",
            "reason": passed ? "checker_cases_passed" : "checker_case_failed",
            "evidence_scope": "synthetic_checker_only_not_app_acceptance", "checks": checks]
}

struct Problem: Error, CustomStringConvertible {
    let reason: String
    let description: String
    let status: String
    init(_ reason: String, _ description: String, status: String = "fail") {
        self.reason = reason
        self.description = description
        self.status = status
    }
}

let maximumDimension = 8192
let maximumPixels = 16_777_216
let maximumFileBytes = 128 * 1024 * 1024

func validateDimensions(_ width: Int, _ height: Int) throws {
    guard width >= 16, height >= 16, width <= maximumDimension, height <= maximumDimension,
          width <= maximumPixels / height else {
        throw Problem("image_dimensions_out_of_bounds",
                      "Dimensions must be 16...8192 with at most \(maximumPixels) pixels")
    }
}

struct Pixels {
    let width: Int
    let height: Int
    let rgba: [UInt8]
}

func normalize(_ image: CGImage) throws -> Pixels {
    try validateDimensions(image.width, image.height)
    guard let space = CGColorSpace(name: CGColorSpace.sRGB) else {
        throw Problem("color_space_failed", "Cannot create sRGB color space")
    }
    var bytes = [UInt8](repeating: 0, count: image.width * image.height * 4)
    try bytes.withUnsafeMutableBytes { buffer in
        guard let context = CGContext(data: buffer.baseAddress, width: image.width,
                                      height: image.height, bitsPerComponent: 8,
                                      bytesPerRow: image.width * 4, space: space,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
                                          | CGBitmapInfo.byteOrder32Big.rawValue) else {
            throw Problem("pixel_decode_failed", "Cannot create normalized RGBA8 bitmap")
        }
        context.setBlendMode(.copy)
        context.interpolationQuality = .none
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    }
    return Pixels(width: image.width, height: image.height, rgba: bytes)
}

func compareImages(_ first: CGImage, _ second: CGImage) throws -> [String: Any] {
    guard first.width == second.width, first.height == second.height else {
        return ["status": "fail", "reason": "dimension_mismatch",
                "error": "Decoded images have different dimensions",
                "first_dimensions": ["width": first.width, "height": first.height],
                "second_dimensions": ["width": second.width, "height": second.height]]
    }
    let a = try normalize(first)
    let b = try normalize(second)
    let marginX = max(1, a.width / 10)
    let marginY = max(1, a.height * 15 / 100)
    let cropWidth = a.width - marginX * 2
    let cropHeight = a.height - marginY * 2
    let compared = cropWidth * cropHeight
    var visible = [0, 0]
    var minimum = [[Int](repeating: 255, count: 4), [Int](repeating: 255, count: 4)]
    var maximum = [[Int](repeating: 0, count: 4), [Int](repeating: 0, count: 4)]
    var changed = 0
    var deltaTotal: UInt64 = 0
    var deltaMaximum = 0
    for y in marginY..<(a.height - marginY) {
        for x in marginX..<(a.width - marginX) {
            let offset = (y * a.width + x) * 4
            let alphaA = Int(a.rgba[offset + 3])
            let alphaB = Int(b.rgba[offset + 3])
            if alphaA > 0 { visible[0] += 1 }
            if alphaB > 0 { visible[1] += 1 }
            var different = false
            for channel in 0..<4 {
                let valueA = Int(a.rgba[offset + channel])
                let valueB = Int(b.rgba[offset + channel])
                let delta = abs(valueA - valueB)
                if delta > 0 { different = true }
                deltaTotal += UInt64(delta)
                deltaMaximum = max(deltaMaximum, delta)
                // Uniformity includes spatial coverage/alpha, not only visible RGB.
                // A monochrome glow on transparent pixels is not a blank interior.
                minimum[0][channel] = min(minimum[0][channel], valueA)
                maximum[0][channel] = max(maximum[0][channel], valueA)
                minimum[1][channel] = min(minimum[1][channel], valueB)
                maximum[1][channel] = max(maximum[1][channel], valueB)
            }
            if different { changed += 1 }
        }
    }
    var evidence: [String: Any] = [
        "status": changed > 0 ? "pass" : "fail",
        "reason": changed > 0 ? "pixels_changed" : "unchanged",
        "width": a.width, "height": a.height,
        "normalization": "sRGB premultiplied RGBA8, identical-size decoded pixels, no rescaling",
        "crop": ["x": marginX, "y": marginY, "width": cropWidth, "height": cropHeight],
        "crop_policy": "10 percent each horizontal edge; 15 percent each vertical edge",
        "compared_pixels": compared, "changed_pixels": changed,
        "changed_fraction": Double(changed) / Double(compared),
        "channel_delta_total": deltaTotal,
        "mean_absolute_channel_delta": Double(deltaTotal) / Double(compared * 4),
        "maximum_channel_delta": deltaMaximum,
        "first_visible_pixels": visible[0], "second_visible_pixels": visible[1],
        "evidence_scope": "rendered_window_interior_pixel_change_only",
        "limitation": "Background visible through glass may change pixels; differences do not prove particle motion, physical audio sync, or audible silence."
    ]
    for index in 0..<2 {
        if visible[index] == 0 {
            evidence["status"] = "fail"
            evidence["reason"] = "transparent_image"
            evidence["error"] = "Image \(index + 1) has a fully transparent interior"
            return evidence
        }
        if minimum[index] == maximum[index] {
            evidence["status"] = "fail"
            evidence["reason"] = "blank_image"
            evidence["error"] = "Image \(index + 1) has a uniform-color interior"
            return evidence
        }
    }
    if changed == 0 { evidence["error"] = "Interior pixels are unchanged; no rendered-frame change observed" }
    return evidence
}

func readImage(_ path: String) throws -> CGImage {
    var info = stat()
    guard stat(path, &info) == 0, (info.st_mode & S_IFMT) == S_IFREG,
          info.st_size > 0, info.st_size <= maximumFileBytes else {
        throw Problem("invalid_image_file", "Image must be a readable regular file of at most 128 MiB: \(path)")
    }
    let options = [kCGImageSourceShouldCache: false] as CFDictionary
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, options),
          CGImageSourceGetCount(source) == 1,
          let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [String: Any],
          let width = properties[kCGImagePropertyPixelWidth as String] as? NSNumber,
          let height = properties[kCGImagePropertyPixelHeight as String] as? NSNumber else {
        throw Problem("image_decode_failed", "Expected one decodable still image: \(path)")
    }
    try validateDimensions(width.intValue, height.intValue)
    guard let image = CGImageSourceCreateImageAtIndex(source, 0, options) else {
        throw Problem("image_decode_failed", "Cannot decode image pixels: \(path)")
    }
    try validateDimensions(image.width, image.height)
    return image
}

func pngData(_ image: CGImage) throws -> Data {
    let data = NSMutableData()
    guard let destination = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil) else {
        throw Problem("png_encode_failed", "Cannot create PNG encoder")
    }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else {
        throw Problem("png_encode_failed", "Cannot encode captured PNG")
    }
    return data as Data
}

struct Identity: Equatable {
    let executable: String
    let startedSeconds: UInt64
    let startedMicroseconds: UInt64
}

func ownedAmbien(_ pid: pid_t) throws -> Identity {
    var info = proc_bsdinfo()
    let size = Int32(MemoryLayout<proc_bsdinfo>.size)
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size,
          info.pbi_uid == geteuid(), info.pbi_ruid == getuid() else {
        throw Problem("candidate_identity_failed", "PID must be a live process owned by the current user")
    }
    // sys/proc_info.h defines the non-importable macro as 4 * MAXPATHLEN.
    var path = [CChar](repeating: 0, count: 4 * Int(PATH_MAX))
    let count = path.withUnsafeMutableBytes { proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }
    guard count > 0 else {
        throw Problem("candidate_identity_failed", "Cannot read candidate executable path")
    }
    let executable = path.withUnsafeBufferPointer { String(cString: $0.baseAddress!) }
    guard executable.hasPrefix("/"), URL(fileURLWithPath: executable).lastPathComponent == "ambien" else {
        throw Problem("candidate_identity_failed", "Capture is restricted to an owned ambien executable")
    }
    return Identity(executable: executable, startedSeconds: info.pbi_start_tvsec,
                    startedMicroseconds: info.pbi_start_tvusec)
}

func ownedWindow(_ id: CGWindowID, pid: pid_t) throws {
    guard let entries = CGWindowListCopyWindowInfo([.optionIncludingWindow], id) as? [[String: Any]],
          let entry = entries.first,
          (entry[kCGWindowNumber as String] as? NSNumber)?.uint32Value == id,
          (entry[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == pid,
          (entry[kCGWindowLayer as String] as? NSNumber)?.intValue == 0 else {
        throw Problem("window_identity_failed", "Requested normal window is not owned by candidate PID")
    }
}

// Callback results are synchronized; keep the main RunLoop alive for native SDK callbacks.
final class CallbackResult<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var result: Result<T, Error>?
    func complete(_ value: T?, _ error: Error?) {
        lock.lock()
        defer { lock.unlock() }
        if let error { result = .failure(error) }
        else if let value { result = .success(value) }
        else { result = .failure(Problem("capture_api_failed", "SDK returned neither result nor error")) }
    }
    func wait() throws -> T {
        let deadline = DispatchTime.now().uptimeNanoseconds + 15_000_000_000
        while true {
            lock.lock()
            let current = result
            lock.unlock()
            if let current { return try current.get() }
            guard DispatchTime.now().uptimeNanoseconds < deadline else {
                throw Problem("capture_timeout", "ScreenCaptureKit did not complete within 15 seconds")
            }
            RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01))
        }
    }
}

func privateDirectory(_ output: String) throws -> (Int32, String) {
    let url = URL(fileURLWithPath: output)
    guard output.hasPrefix("/"), url.pathExtension.lowercased() == "png",
          url.standardizedFileURL.path == output else {
        throw Problem("invalid_output", "Output must be an absolute, normalized PNG path")
    }
    let parent = url.deletingLastPathComponent().path
    let fd = open(parent, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
    guard fd >= 0 else {
        throw Problem("private_output_required", "Output parent must exist and not be a symlink: errno=\(errno)")
    }
    var info = stat()
    guard fstat(fd, &info) == 0, info.st_uid == geteuid(),
          (info.st_mode & 0o077) == 0 else {
        close(fd)
        throw Problem("private_output_required", "Output parent must be user-owned with no group/other access (0700)")
    }
    let name = url.lastPathComponent
    var existing = stat()
    let exists = fstatat(fd, name, &existing, AT_SYMLINK_NOFOLLOW)
    guard exists != 0, errno == ENOENT else {
        close(fd)
        throw Problem("output_exists", "Refusing to overwrite an existing output path")
    }
    return (fd, name)
}

func savePrivatePNG(_ data: Data, directory: Int32, name: String) throws {
    let fd = openat(directory, name, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
    guard fd >= 0 else { throw Problem("output_create_failed", "Cannot create private PNG: errno=\(errno)") }
    var completed = false
    defer {
        close(fd)
        if !completed { unlinkat(directory, name, 0) }
    }
    guard fchmod(fd, 0o600) == 0 else {
        throw Problem("output_write_failed", "Cannot set private PNG permissions: errno=\(errno)")
    }
    try data.withUnsafeBytes { buffer in
        var written = 0
        while written < buffer.count {
            let count = Darwin.write(fd, buffer.baseAddress!.advanced(by: written), buffer.count - written)
            if count < 0 && errno == EINTR { continue }
            guard count > 0 else { throw Problem("output_write_failed", "PNG write failed: errno=\(errno)") }
            written += count
        }
    }
    guard fsync(fd) == 0 else { throw Problem("output_write_failed", "PNG fsync failed: errno=\(errno)") }
    completed = true
}

func capture(pid: pid_t, windowID: CGWindowID, output: String) throws -> [String: Any] {
    // Must precede SCShareableContent, ScreenshotManager, and any permission-request API.
    guard CGPreflightScreenCaptureAccess() else {
        throw Problem("screen_recording_permission_required",
                      "Screen Recording is not authorized. User must prepare permission externally; no capture or permission request was attempted.",
                      status: "blocked")
    }
    guard #available(macOS 14.2, *) else {
        throw Problem("unsupported_macos", "Window capture requires macOS 14.2 or later", status: "blocked")
    }
    let identity = try ownedAmbien(pid)
    try ownedWindow(windowID, pid: pid)
    let (directory, name) = try privateDirectory(output)
    defer { close(directory) }
    let contentResult = CallbackResult<SCShareableContent>()
    SCShareableContent.getExcludingDesktopWindows(true, onScreenWindowsOnly: true) {
        contentResult.complete($0, $1)
    }
    let content = try contentResult.wait()
    guard let window = content.windows.first(where: { $0.windowID == windowID }),
          window.owningApplication?.processID == pid, window.windowLayer == 0,
          window.isOnScreen else {
        throw Problem("window_identity_failed", "ScreenCaptureKit did not expose the requested owned on-screen window")
    }
    guard try ownedAmbien(pid) == identity else {
        throw Problem("candidate_identity_changed", "Candidate process identity changed during capture preparation")
    }
    try ownedWindow(windowID, pid: pid)
    // Never construct a display/application filter: only this single owned window.
    let filter = SCContentFilter(desktopIndependentWindow: window)
    let scale = Double(filter.pointPixelScale)
    let width = ceil(Double(filter.contentRect.width) * scale)
    let height = ceil(Double(filter.contentRect.height) * scale)
    guard scale.isFinite, scale > 0, width.isFinite, height.isFinite,
          width >= 16, height >= 16, width <= Double(maximumDimension), height <= Double(maximumDimension) else {
        throw Problem("image_dimensions_out_of_bounds", "Window native capture dimensions exceed checker limits")
    }
    try validateDimensions(Int(width), Int(height))
    let configuration = SCStreamConfiguration()
    configuration.width = Int(width)
    configuration.height = Int(height)
    configuration.showsCursor = false
    configuration.ignoreShadowsSingleWindow = true
    configuration.includeChildWindows = false
    configuration.capturesAudio = false
    let imageResult = CallbackResult<CGImage>()
    // Recheck immediately before invoking capture; never invoke it after a false preflight.
    guard CGPreflightScreenCaptureAccess() else {
        throw Problem("screen_recording_permission_required", "Screen Recording authorization became unavailable",
                      status: "blocked")
    }
    SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration) {
        imageResult.complete($0, $1)
    }
    let image = try imageResult.wait()
    try validateDimensions(image.width, image.height)
    guard try ownedAmbien(pid) == identity else {
        throw Problem("candidate_identity_changed", "Candidate process identity changed while capturing")
    }
    try ownedWindow(windowID, pid: pid)
    try savePrivatePNG(pngData(image), directory: directory, name: name)
    return ["status": "pass", "operation": "capture", "reason": "owned_window_png_saved",
            "pid": pid, "window_id": windowID, "executable": identity.executable,
            "output": output, "width": image.width, "height": image.height,
            "window_only": true, "capture_method": "ScreenCaptureKit desktopIndependentWindow",
            "screen_recording_authorized": true, "file_mode": "0600",
            "evidence_scope": "actual_owned_window_capture_not_app_acceptance"]
}

let usage = """
/usr/bin/swift scripts/native-screenshot.swift capture --pid PID --window-id CGWINDOWID --output ABSOLUTE_PNG
/usr/bin/swift scripts/native-screenshot.swift compare IMAGE1 IMAGE2
/usr/bin/swift scripts/native-screenshot.swift --self-test
/usr/bin/swift scripts/native-screenshot.swift --help
"""

func run(_ arguments: [String]) throws -> [String: Any] {
    if arguments == ["--help"] {
        return ["status": "pass", "operation": "help", "reason": "usage",
                "usage": usage, "exit_codes": ["pass": 0, "fail": 1, "blocked": 77],
                "capture_safety": "No prompts; only owned ambien window; existing user-owned private (0700) output parent; new 0600 PNG; no overwrite",
                "compare": "Decoded sRGB RGBA8 interior pixel comparison; unchanged/blank/transparent/mismatched images fail",
                "limits": ["maximum_dimension": maximumDimension, "maximum_pixels": maximumPixels,
                           "maximum_image_file_bytes": maximumFileBytes],
                "self_test_scope": "Synthetic checker verification, not app acceptance"]
    }
    if arguments == ["--self-test"] { return try selfTest() }
    if arguments.first == "compare" {
        guard arguments.count == 3, !arguments[1].hasPrefix("--"), !arguments[2].hasPrefix("--") else {
            throw Problem("invalid_arguments", "compare requires exactly two image paths")
        }
        var result = try compareImages(readImage(arguments[1]), readImage(arguments[2]))
        result["operation"] = "compare"
        result["first_image"] = URL(fileURLWithPath: arguments[1]).standardizedFileURL.path
        result["second_image"] = URL(fileURLWithPath: arguments[2]).standardizedFileURL.path
        return result
    }
    if arguments.first == "capture" {
        guard arguments.count == 7 else {
            throw Problem("invalid_arguments", "capture requires exactly --pid, --window-id, and --output")
        }
        var values: [String: String] = [:]
        for index in stride(from: 1, to: arguments.count, by: 2) {
            let flag = arguments[index]
            guard ["--pid", "--window-id", "--output"].contains(flag), values[flag] == nil else {
                throw Problem("invalid_arguments", "Unknown or repeated capture option: \(flag)")
            }
            values[flag] = arguments[index + 1]
        }
        guard let pidText = values["--pid"], pidText.allSatisfy({ $0.isASCII && $0.isNumber }),
              let pid = Int32(pidText), pid > 0,
              let windowText = values["--window-id"], windowText.allSatisfy({ $0.isASCII && $0.isNumber }),
              let window = UInt32(windowText), window > 0,
              let output = values["--output"], output.hasPrefix("/"),
              URL(fileURLWithPath: output).pathExtension.lowercased() == "png" else {
            throw Problem("invalid_arguments", "PID/window ID must be positive integers and output an absolute PNG path")
        }
        return try capture(pid: pid, windowID: window, output: output)
    }
    throw Problem("invalid_arguments", "Unknown or missing command; use --help")
}

var response: [String: Any]
do {
    response = try run(Array(CommandLine.arguments.dropFirst()))
} catch let problem as Problem {
    response = ["status": problem.status, "reason": problem.reason, "error": problem.description]
} catch {
    let nativeError = error as NSError
    // SCError.h defines user-declined authorization as SCStreamErrorDomain / -3801.
    let denied = nativeError.domain == SCStreamErrorDomain && nativeError.code == -3801
    response = ["status": denied ? "blocked" : "fail",
                "reason": denied ? "screen_recording_permission_required" : "operation_failed",
                "error": nativeError.localizedDescription,
                "error_domain": nativeError.domain, "error_code": nativeError.code]
}
response["schema_version"] = 1
do {
    let data = try JSONSerialization.data(withJSONObject: response, options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([10]))
} catch {
    FileHandle.standardOutput.write(Data("{\"status\":\"fail\",\"reason\":\"json_encode_failed\",\"error\":\"Cannot serialize evidence\",\"schema_version\":1}\n".utf8))
    exit(1)
}
exit(response["status"] as? String == "pass" ? 0 : (response["status"] as? String == "blocked" ? 77 : 1))
