import AppKit
import CoreGraphics
import Foundation
import ImageIO
import ScreenCaptureKit

private func fail(_ message: String) -> Never {
    FileHandle.standardError.write(Data("window probe: \(message)\n".utf8))
    exit(1)
}

private func emit(_ value: [String: Any]) {
    guard
        let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
        let json = String(data: data, encoding: .utf8)
    else {
        fail("could not encode result")
    }

    print(json)
}

if CommandLine.arguments.count == 4,
   CommandLine.arguments[1] == "capture",
   let windowId = UInt32(CommandLine.arguments[2])
{
    _ = NSApplication.shared
    let outputUrl = URL(fileURLWithPath: CommandLine.arguments[3])
    Task {
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(
                false,
                onScreenWindowsOnly: true
            )
            guard let targetWindow = content.windows.first(where: {
                $0.windowID == CGWindowID(windowId)
            }) else {
                fail("ScreenCaptureKit could not find on-screen window \(windowId)")
            }

            let configuration = SCStreamConfiguration()
            configuration.width = Int(targetWindow.frame.width)
            configuration.height = Int(targetWindow.frame.height)
            configuration.showsCursor = false
            configuration.ignoreShadowsSingleWindow = true

            let filter = SCContentFilter(desktopIndependentWindow: targetWindow)
            let image = try await SCScreenshotManager.captureImage(
                contentFilter: filter,
                configuration: configuration
            )
            guard let destination = CGImageDestinationCreateWithURL(
                outputUrl as CFURL,
                "public.png" as CFString,
                1,
                nil
            ) else {
                fail("could not create PNG destination")
            }

            CGImageDestinationAddImage(destination, image, nil)
            guard CGImageDestinationFinalize(destination) else {
                fail("could not finalize PNG")
            }

            emit(["width": image.width, "height": image.height])
            exit(0)
        } catch {
            fail("ScreenCaptureKit failed for window \(windowId): \(error)")
        }
    }

    RunLoop.main.run()
    fail("ScreenCaptureKit run loop ended unexpectedly")
}

guard CommandLine.arguments.count == 2,
      let requestedPid = Int32(CommandLine.arguments[1]),
      requestedPid > 0
else {
    fail("usage: window_probe <pid>")
}

guard let windowInfo = CGWindowListCopyWindowInfo(
    [.optionOnScreenOnly, .excludeDesktopElements],
    kCGNullWindowID
) as? [[CFString: Any]] else {
    fail("CGWindowListCopyWindowInfo returned no window list")
}

let candidates: [[String: Any]] = windowInfo.compactMap { window in
    guard
        (window[kCGWindowOwnerPID] as? NSNumber)?.int32Value == requestedPid,
        (window[kCGWindowIsOnscreen] as? NSNumber)?.boolValue == true,
        (window[kCGWindowLayer] as? NSNumber)?.intValue == 0,
        (window[kCGWindowAlpha] as? NSNumber)?.doubleValue ?? 0 > 0,
        let windowId = (window[kCGWindowNumber] as? NSNumber)?.uint32Value,
        window[kCGWindowBounds] != nil,
        let bounds = CGRect(
            dictionaryRepresentation: window[kCGWindowBounds] as! CFDictionary
        ),
        bounds.width >= 320,
        bounds.height >= 240
    else {
        return nil
    }

    return [
        "id": windowId,
        "name": window[kCGWindowName] as? String ?? "",
        "ownerName": window[kCGWindowOwnerName] as? String ?? "",
        "x": bounds.origin.x,
        "y": bounds.origin.y,
        "width": bounds.width,
        "height": bounds.height,
        "area": bounds.width * bounds.height,
    ]
}

let mainWindow = candidates.max {
    ($0["area"] as? Double ?? 0) < ($1["area"] as? Double ?? 0)
}
let encodedWindow = mainWindow.map { $0 as Any } ?? NSNull()
let sessionInfo = CGSessionCopyCurrentDictionary() as? [String: Any]
let screenLocked = (sessionInfo?["CGSSessionScreenIsLocked"] as? NSNumber)?.boolValue ?? false

emit([
    "pid": requestedPid,
    "screenCaptureAccess": CGPreflightScreenCaptureAccess(),
    "screenLocked": screenLocked,
    "window": encodedWindow,
])
