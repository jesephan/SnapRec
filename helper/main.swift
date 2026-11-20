// snaprec-helper: ScreenCaptureKit-based recorder + region selector for SnapRec.
//
//   snaprec-helper list-displays
//   snaprec-helper select
//   snaprec-helper record --out FILE [--display ID] [--rect x,y,w,h] [--audio]
//                         [--no-cursor] [--fps N] [--half] [--exclude-bundle ID]
//
// `record` streams H.264/HEVC + AAC straight to disk (fragmented, crash-safe)
// until it reads "q" or EOF on stdin, or gets SIGINT/SIGTERM.
import AppKit
import AVFoundation
import CoreMedia
import Foundation
import ScreenCaptureKit

setvbuf(stdout, nil, _IOLBF, 0)

func fail(_ msg: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

func printJSON(_ obj: Any) {
    if let d = try? JSONSerialization.data(withJSONObject: obj),
       let s = String(data: d, encoding: .utf8) { print(s) }
}

func displayID(of screen: NSScreen) -> UInt32 {
    (screen.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value ?? 0
}

// MARK: - list-displays

func listDisplays() {
    _ = NSApplication.shared
    let mainID = CGMainDisplayID()
    for s in NSScreen.screens {
        let id = displayID(of: s)
        printJSON(["id": id, "name": s.localizedName, "width": Int(s.frame.width),
                   "height": Int(s.frame.height), "main": id == mainID])
    }
    exit(0)
}

// MARK: - select (drag a rectangle on any screen)

final class OverlayWindow: NSWindow {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { true }
}

final class SelectView: NSView {
    let display: UInt32
    var start: NSPoint?
    var current: NSPoint?
    init(frame: NSRect, display: UInt32) { self.display = display; super.init(frame: frame) }
    required init?(coder: NSCoder) { fatalError() }
    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func resetCursorRects() { addCursorRect(bounds, cursor: .crosshair) }

    var selection: NSRect? {
        guard let a = start, let b = current else { return nil }
        return NSRect(x: min(a.x, b.x), y: min(a.y, b.y), width: abs(a.x - b.x), height: abs(a.y - b.y))
    }
    func point(_ e: NSEvent) -> NSPoint {
        let p = convert(e.locationInWindow, from: nil)
        return NSPoint(x: min(max(p.x, 0), bounds.width), y: min(max(p.y, 0), bounds.height))
    }
    override func mouseDown(with e: NSEvent) { start = point(e); current = start; needsDisplay = true }
    override func mouseDragged(with e: NSEvent) { current = point(e); needsDisplay = true }
    override func mouseUp(with e: NSEvent) {
        current = point(e)
        if let r = selection, r.width >= 8, r.height >= 8 {
            printJSON(["display": display, "x": Int(r.minX.rounded()), "y": Int(r.minY.rounded()),
                       "w": Int(r.width.rounded()), "h": Int(r.height.rounded())])
            exit(0)
        }
        start = nil; current = nil; needsDisplay = true
    }
    override func keyDown(with e: NSEvent) { if e.keyCode == 53 { exit(2) } } // Esc

    override func draw(_ dirtyRect: NSRect) {
        NSColor(white: 0, alpha: 0.35).setFill()
        bounds.fill()
        let attrs: [NSAttributedString.Key: Any] = [
            .font: NSFont.systemFont(ofSize: 15, weight: .semibold),
            .foregroundColor: NSColor.white,
            .backgroundColor: NSColor(white: 0, alpha: 0.6),
        ]
        if let r = selection {
            NSColor.clear.setFill()
            r.fill(using: .copy)
            NSColor.systemRed.setStroke()
            let p = NSBezierPath(rect: r.insetBy(dx: -1, dy: -1)); p.lineWidth = 2; p.stroke()
            let label = " \(Int(r.width)) × \(Int(r.height)) " as NSString
            label.draw(at: NSPoint(x: r.minX, y: max(r.minY - 24, 2)), withAttributes: attrs)
        } else {
            let hint = "  Drag to select the area to record — Esc to cancel  " as NSString
            let size = hint.size(withAttributes: attrs)
            hint.draw(at: NSPoint(x: (bounds.width - size.width) / 2, y: bounds.height / 2 - size.height / 2),
                      withAttributes: attrs)
        }
    }
}

var overlayWindows: [NSWindow] = []

func selectRegion() {
    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    for screen in NSScreen.screens {
        let w = OverlayWindow(contentRect: screen.frame, styleMask: .borderless, backing: .buffered, defer: false)
        w.setFrame(screen.frame, display: false)
        w.isOpaque = false
        w.backgroundColor = .clear
        w.level = .screenSaver
        w.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
        let v = SelectView(frame: NSRect(origin: .zero, size: screen.frame.size), display: displayID(of: screen))
        w.contentView = v
        w.makeKeyAndOrderFront(nil)
        w.makeFirstResponder(v)
        overlayWindows.append(w)
    }
    app.activate(ignoringOtherApps: true)
    NSCursor.crosshair.set()
    app.run()
}

// MARK: - record

final class Recorder: NSObject, SCStreamOutput, SCStreamDelegate {
    let writer: AVAssetWriter
    let videoIn: AVAssetWriterInput
    let audioIn: AVAssetWriterInput?
    let queue = DispatchQueue(label: "snaprec.capture")
    var stream: SCStream?
    var started = false
    var finished = false

    init(out: URL, width: Int, height: Int, fps: Int, audio: Bool) throws {
        try? FileManager.default.removeItem(at: out)
        writer = try AVAssetWriter(outputURL: out, fileType: .mp4)
        writer.movieFragmentInterval = CMTime(seconds: 5, preferredTimescale: 600)
        let useHEVC = width > 4096 || height > 2304
        let bitrate = min(max(width * height * fps / 10, 2_000_000), 50_000_000)
        var compression: [String: Any] = [
            AVVideoAverageBitRateKey: bitrate,
            AVVideoExpectedSourceFrameRateKey: fps,
            AVVideoMaxKeyFrameIntervalKey: fps * 2,
        ]
        if !useHEVC { compression[AVVideoProfileLevelKey] = AVVideoProfileLevelH264HighAutoLevel }
        videoIn = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: useHEVC ? AVVideoCodecType.hevc : AVVideoCodecType.h264,
            AVVideoWidthKey: width,
            AVVideoHeightKey: height,
            AVVideoCompressionPropertiesKey: compression,
        ])
        videoIn.expectsMediaDataInRealTime = true
        writer.add(videoIn)
        if audio {
            let a = AVAssetWriterInput(mediaType: .audio, outputSettings: [
                AVFormatIDKey: kAudioFormatMPEG4AAC,
                AVSampleRateKey: 48000,
                AVNumberOfChannelsKey: 2,
                AVEncoderBitRateKey: 192_000,
            ])
            a.expectsMediaDataInRealTime = true
            writer.add(a)
            audioIn = a
        } else {
            audioIn = nil
        }
    }

    func stream(_ stream: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard sb.isValid, !finished else { return }
        switch type {
        case .screen:
            guard let atts = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false)
                    as? [[SCStreamFrameInfo: Any]],
                  let raw = atts.first?[.status] as? Int,
                  SCFrameStatus(rawValue: raw) == .complete else { return }
            if !started {
                guard writer.startWriting() else {
                    fail("Writer failed: \(writer.error?.localizedDescription ?? "unknown")")
                }
                writer.startSession(atSourceTime: sb.presentationTimeStamp)
                started = true
                print("started")
            }
            if videoIn.isReadyForMoreMediaData { videoIn.append(sb) }
        case .audio:
            if started, let a = audioIn, a.isReadyForMoreMediaData { a.append(sb) }
        @unknown default:
            break
        }
    }

    func stream(_ stream: SCStream, didStopWithError error: Error) {
        FileHandle.standardError.write("Capture stopped: \(error.localizedDescription)\n".data(using: .utf8)!)
        Task { await self.stop(); exit(3) }
    }

    func stop() async {
        if let s = stream { try? await s.stopCapture() }
        queue.sync { finished = true }
        guard started else {
            fail("No frames were captured (check Screen Recording permission).", 4)
        }
        videoIn.markAsFinished()
        audioIn?.markAsFinished()
        await writer.finishWriting()
        if writer.status != .completed {
            fail("Finalizing failed: \(writer.error?.localizedDescription ?? "unknown")", 5)
        }
    }
}

var recorder: Recorder?
var signalSources: [DispatchSourceSignal] = []
var stopping = false

func stopAndExit() {
    DispatchQueue.main.async {
        if stopping { return }
        stopping = true
        Task {
            await recorder?.stop()
            print("saved")
            exit(0)
        }
    }
}

func record(_ args: [String]) {
    var out: String?
    var display: UInt32?
    var rect: CGRect?
    var audio = false
    var cursor = true
    var fps = 30
    var half = false
    var excludeBundle: String?
    var i = 0
    func next() -> String { i += 1; return i < args.count ? args[i] : "" }
    while i < args.count {
        switch args[i] {
        case "--out": out = next()
        case "--display": display = UInt32(next())
        case "--rect":
            let p = next().split(separator: ",").compactMap { Double($0) }
            if p.count == 4 { rect = CGRect(x: p[0], y: p[1], width: p[2], height: p[3]) }
        case "--audio": audio = true
        case "--no-cursor": cursor = false
        case "--fps": fps = max(1, min(60, Int(next()) ?? 30))
        case "--half": half = true
        case "--exclude-bundle": excludeBundle = next()
        default: fail("Unknown argument \(args[i])")
        }
        i += 1
    }
    guard let outPath = out else { fail("--out is required") }
    _ = NSApplication.shared
    Task { [display, rect, audio, cursor, fps, half, excludeBundle] in
        do {
            let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
            let wanted = display ?? CGMainDisplayID()
            guard let d = content.displays.first(where: { $0.displayID == wanted }) ?? content.displays.first else {
                fail("No display found")
            }
            let excluded = content.applications.filter { $0.bundleIdentifier == excludeBundle }
            let filter = SCContentFilter(display: d, excludingApplications: excluded, exceptingWindows: [])

            let scale = NSScreen.screens.first(where: { displayID(of: $0) == d.displayID })?.backingScaleFactor ?? 2
            let area = (rect ?? CGRect(x: 0, y: 0, width: d.width, height: d.height))
                .intersection(CGRect(x: 0, y: 0, width: d.width, height: d.height))
            guard area.width >= 2, area.height >= 2 else { fail("Selected area is empty") }
            let factor = half ? scale / 2 : scale
            let w = (Int(area.width * factor) / 2) * 2
            let h = (Int(area.height * factor) / 2) * 2

            let cfg = SCStreamConfiguration()
            cfg.width = w
            cfg.height = h
            if rect != nil { cfg.sourceRect = area }
            cfg.showsCursor = cursor
            cfg.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
            cfg.queueDepth = 6
            cfg.pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
            if audio {
                cfg.capturesAudio = true
                cfg.sampleRate = 48000
                cfg.channelCount = 2
                cfg.excludesCurrentProcessAudio = true
            }

            let rec = try Recorder(out: URL(fileURLWithPath: outPath), width: w, height: h, fps: fps, audio: audio)
            let stream = SCStream(filter: filter, configuration: cfg, delegate: rec)
            try stream.addStreamOutput(rec, type: .screen, sampleHandlerQueue: rec.queue)
            if audio { try stream.addStreamOutput(rec, type: .audio, sampleHandlerQueue: rec.queue) }
            rec.stream = stream
            recorder = rec
            try await stream.startCapture()
        } catch {
            fail("Could not start recording: \(error.localizedDescription)", 6)
        }
    }

    // Stop on "q" / EOF from the parent app
    Thread.detachNewThread {
        while let line = readLine() {
            if line.contains("q") { break }
        }
        stopAndExit()
    }
    for sig in [SIGINT, SIGTERM] {
        signal(sig, SIG_IGN)
        let src = DispatchSource.makeSignalSource(signal: sig, queue: .main)
        src.setEventHandler { stopAndExit() }
        src.resume()
        signalSources.append(src)
    }
    dispatchMain()
}

// MARK: - main

let argv = CommandLine.arguments
guard argv.count >= 2 else { fail("usage: snaprec-helper list-displays | select | record --out FILE ...") }
switch argv[1] {
case "list-displays": listDisplays()
case "select": selectRegion()
case "record": record(Array(argv.dropFirst(2)))
default: fail("unknown command \(argv[1])")
}
