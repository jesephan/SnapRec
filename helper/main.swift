// snaprec-helper: ScreenCaptureKit-based recorder + region selector for SnapRec.
//
//   snaprec-helper list-displays
//   snaprec-helper select
//   snaprec-helper choose-format [--kind video|gif] [--container mp4|webm]
//                         [--quality low|medium|high|best] [--gif-fps N] [--no-ffmpeg]
//                         [--dir FOLDER] [--name FILENAME]
//   snaprec-helper gif --in VIDEO --out FILE.gif [--fps N] [--width W]
//   snaprec-helper record --out FILE [--display ID] [--rect x,y,w,h] [--audio]
//                         [--no-cursor] [--fps N] [--half] [--exclude-bundle ID]
//
// `record` streams H.264/HEVC + AAC straight to disk
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

func describe(_ error: Error?) -> String {
    guard let e = error as NSError? else { return "unknown" }
    var s = "\(e.localizedDescription) (\(e.domain) \(e.code))"
    if let u = e.userInfo[NSUnderlyingErrorKey] as? NSError { s += " ← \(u.domain) \(u.code)" }
    return s
}

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
        // No movieFragmentInterval: when the screen is still, ScreenCaptureKit sends
        // no frames, and a fragment boundary with no video makes finishWriting fail
        // (-11800 / -16341), leaving only the first fragment (~5 s) playable.
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
                    fail("Writer failed: \(describe(writer.error))")
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
        // End at "now" (same host clock as the capture timestamps) so a still
        // screen at the end isn't cut off at the last changed frame.
        writer.endSession(atSourceTime: CMClockGetTime(CMClockGetHostTimeClock()))
        videoIn.markAsFinished()
        audioIn?.markAsFinished()
        await writer.finishWriting()
        if writer.status != .completed {
            fail("Finalizing failed: \(describe(writer.error))", 5)
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

// MARK: - choose-format ("Record as…" prompt shown before every recording)
//
// Prints {"format":"mp4"|"webm","path":…} or {"format":"gif","quality":"…","fps":N,"path":…}.
// If the file already exists the user must confirm replacing it.
// Exits with code 2 if cancelled.

final class FormatPrompt: NSObject {
    let kind = NSSegmentedControl(labels: ["Video", "GIF"], trackingMode: .selectOne, target: nil, action: nil)
    let container = NSPopUpButton(frame: .zero, pullsDown: false)
    let quality = NSPopUpButton(frame: .zero, pullsDown: false)
    let fps = NSPopUpButton(frame: .zero, pullsDown: false)
    let videoBox = NSView()
    let gifBox = NSView()
    let nameField = NSTextField(string: "")
    let extLabel = NSTextField(labelWithString: ".mp4")
    let dirLabel = NSTextField(labelWithString: "")
    var dir = URL(fileURLWithPath: NSHomeDirectory())

    static let qualities = [("low", "Low — 640 px, smallest file"), ("medium", "Medium — 960 px"),
                            ("high", "High — 1280 px"), ("best", "Best — original size")]
    static let fpsChoices = [5, 10, 12, 15, 20, 30]

    func label(_ text: String, _ frame: NSRect) -> NSTextField {
        let l = NSTextField(labelWithString: text)
        l.alignment = .right
        l.frame = frame
        return l
    }

    func build(kindDefault: String, containerDefault: String, qualityDefault: String,
               fpsDefault: Int, hasFFmpeg: Bool, dir: URL, name: String) -> NSView {
        let width: CGFloat = 340
        let root = NSView(frame: NSRect(x: 0, y: 0, width: width, height: 192))

        // File name + folder
        self.dir = dir
        root.addSubview(label("File name:", NSRect(x: 0, y: 166, width: 110, height: 20)))
        nameField.stringValue = name
        nameField.placeholderString = "Recording"
        nameField.frame = NSRect(x: 118, y: 164, width: width - 118 - 48, height: 24)
        root.addSubview(nameField)
        extLabel.frame = NSRect(x: width - 44, y: 166, width: 44, height: 20)
        extLabel.textColor = .secondaryLabelColor
        root.addSubview(extLabel)
        root.addSubview(label("Save to:", NSRect(x: 0, y: 134, width: 110, height: 20)))
        dirLabel.frame = NSRect(x: 118, y: 134, width: width - 118 - 90, height: 20)
        dirLabel.lineBreakMode = .byTruncatingMiddle
        dirLabel.textColor = .secondaryLabelColor
        root.addSubview(dirLabel)
        let choose = NSButton(title: "Choose…", target: self, action: #selector(chooseDir))
        choose.bezelStyle = .rounded
        choose.frame = NSRect(x: width - 86, y: 128, width: 86, height: 30)
        root.addSubview(choose)
        updateDirLabel()

        kind.frame = NSRect(x: 0, y: 88, width: width, height: 26)
        kind.segmentStyle = .rounded
        for i in 0..<2 { kind.setWidth((width - 6) / 2, forSegment: i) }
        kind.target = self
        kind.action = #selector(kindChanged)
        root.addSubview(kind)

        // Video: container
        for box in [videoBox, gifBox] { box.frame = NSRect(x: 0, y: 0, width: width, height: 80); root.addSubview(box) }
        videoBox.addSubview(label("Format:", NSRect(x: 0, y: 48, width: 110, height: 20)))
        container.addItems(withTitles: ["MP4 (H.264)", "WebM (VP9)"])
        container.frame = NSRect(x: 118, y: 44, width: width - 118, height: 26)
        container.selectItem(at: containerDefault == "webm" ? 1 : 0)
        container.target = self
        container.action = #selector(updateExt)
        videoBox.addSubview(container)

        // GIF: quality + fps
        gifBox.addSubview(label("Quality:", NSRect(x: 0, y: 48, width: 110, height: 20)))
        quality.addItems(withTitles: FormatPrompt.qualities.map { $0.1 })
        quality.frame = NSRect(x: 118, y: 44, width: width - 118, height: 26)
        quality.selectItem(at: FormatPrompt.qualities.firstIndex { $0.0 == qualityDefault } ?? 1)
        gifBox.addSubview(quality)
        gifBox.addSubview(label("Frames per second:", NSRect(x: 0, y: 12, width: 110, height: 20)))
        fps.addItems(withTitles: FormatPrompt.fpsChoices.map { "\($0) fps" })
        fps.frame = NSRect(x: 118, y: 8, width: 110, height: 26)
        let nearest = FormatPrompt.fpsChoices.enumerated().min { abs($0.1 - fpsDefault) < abs($1.1 - fpsDefault) }
        fps.selectItem(at: nearest?.offset ?? 1)
        gifBox.addSubview(fps)

        if !hasFFmpeg {
            // WebM and GIF are converted with ffmpeg after recording.
            container.autoenablesItems = false
            container.selectItem(at: 0)
            container.item(at: 1)?.isEnabled = false
            // GIF is converted natively, only WebM needs ffmpeg.
            let note = NSTextField(labelWithString: "WebM needs ffmpeg:  brew install ffmpeg")
            note.font = .systemFont(ofSize: NSFont.smallSystemFontSize)
            note.textColor = .secondaryLabelColor
            note.frame = NSRect(x: 0, y: 12, width: width, height: 18)
            videoBox.addSubview(note)
        }

        kind.selectedSegment = kindDefault == "gif" ? 1 : 0
        kindChanged()
        return root
    }

    @objc func kindChanged() {
        let gif = kind.selectedSegment == 1
        videoBox.isHidden = gif
        gifBox.isHidden = !gif
        updateExt()
    }

    var ext: String {
        kind.selectedSegment == 1 ? "gif" : (container.indexOfSelectedItem == 1 ? "webm" : "mp4")
    }

    @objc func updateExt() { extLabel.stringValue = "." + ext }

    func updateDirLabel() {
        dirLabel.stringValue = (dir.path as NSString).abbreviatingWithTildeInPath
        dirLabel.toolTip = dir.path
    }

    @objc func chooseDir() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = "Choose"
        panel.message = "Choose where to save the recording"
        panel.directoryURL = dir
        if panel.runModal() == .OK, let url = panel.url {
            dir = url
            updateDirLabel()
        }
    }

    /// File name without the extension, with characters macOS can't use removed.
    var baseName: String {
        var n = nameField.stringValue.trimmingCharacters(in: .whitespacesAndNewlines)
        n = n.replacingOccurrences(of: "/", with: "-").replacingOccurrences(of: ":", with: "-")
        for e in [".mp4", ".webm", ".gif", ".mov"] where n.lowercased().hasSuffix(e) {
            n = String(n.dropLast(e.count))
        }
        while n.hasPrefix(".") { n.removeFirst() }
        return n.isEmpty ? "Recording" : n
    }

    var path: URL { dir.appendingPathComponent(baseName + "." + ext) }

    func result() -> [String: Any] {
        if kind.selectedSegment == 1 {
            return ["format": "gif",
                    "quality": FormatPrompt.qualities[max(0, quality.indexOfSelectedItem)].0,
                    "fps": FormatPrompt.fpsChoices[max(0, fps.indexOfSelectedItem)],
                    "path": path.path]
        }
        return ["format": ext, "path": path.path]
    }
}

func chooseFormat(_ args: [String]) {
    var kindDefault = "video", containerDefault = "mp4", qualityDefault = "medium"
    var fpsDefault = 10, hasFFmpeg = true
    var dir = FileManager.default.urls(for: .moviesDirectory, in: .userDomainMask).first
        ?? URL(fileURLWithPath: NSHomeDirectory())
    var name = "Recording"
    var i = 0
    func next() -> String { i += 1; return i < args.count ? args[i] : "" }
    while i < args.count {
        switch args[i] {
        case "--kind": kindDefault = next()
        case "--container": containerDefault = next()
        case "--quality": qualityDefault = next()
        case "--gif-fps": fpsDefault = Int(next()) ?? 10
        case "--no-ffmpeg": hasFFmpeg = false
        case "--dir": dir = URL(fileURLWithPath: next(), isDirectory: true)
        case "--name": name = next()
        default: break
        }
        i += 1
    }

    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)
    let prompt = FormatPrompt()
    let accessory = prompt.build(kindDefault: kindDefault, containerDefault: containerDefault,
                                 qualityDefault: qualityDefault, fpsDefault: fpsDefault,
                                 hasFFmpeg: hasFFmpeg, dir: dir, name: name)
    app.activate(ignoringOtherApps: true)
    while true {
        let alert = NSAlert()
        alert.messageText = "Record as…"
        alert.informativeText = "Choose Video or GIF, a file name and where to save it."
        alert.accessoryView = accessory
        alert.addButton(withTitle: "Start Recording")
        alert.addButton(withTitle: "Cancel")
        alert.window.level = .floating
        alert.window.initialFirstResponder = prompt.nameField
        guard alert.runModal() == .alertFirstButtonReturn else {
            // Still report the folder so a changed save location is remembered.
            printJSON(["cancelled": true, "dir": prompt.dir.path])
            exit(2)
        }
        alert.accessoryView = nil
        accessory.removeFromSuperview()

        let target = prompt.path
        if FileManager.default.fileExists(atPath: target.path) {
            let confirm = NSAlert()
            confirm.alertStyle = .warning
            confirm.messageText = "“\(target.lastPathComponent)” already exists. Do you want to replace it?"
            confirm.informativeText = "A file with the same name already exists in “\(target.deletingLastPathComponent().lastPathComponent)”. Replacing it will overwrite its current contents when recording starts."
            confirm.addButton(withTitle: "Replace")
            confirm.addButton(withTitle: "Cancel")
            confirm.buttons[0].hasDestructiveAction = true
            confirm.window.level = .floating
            if confirm.runModal() != .alertFirstButtonReturn { continue } // back to the form
        }
        break
    }
    printJSON(prompt.result())
    exit(0)
}

// MARK: - gif (native MP4 -> animated GIF, used when ffmpeg isn't installed)

func makeGif(_ args: [String]) {
    var input = "", output = "", fps = 10, maxWidth = 0
    var i = 0
    func next() -> String { i += 1; return i < args.count ? args[i] : "" }
    while i < args.count {
        switch args[i] {
        case "--in": input = next()
        case "--out": output = next()
        case "--fps": fps = max(1, min(30, Int(next()) ?? 10))
        case "--width": maxWidth = max(0, Int(next()) ?? 0)
        default: break
        }
        i += 1
    }
    guard !input.isEmpty, !output.isEmpty else { fail("gif: --in and --out are required") }

    let asset = AVURLAsset(url: URL(fileURLWithPath: input))
    let duration = CMTimeGetSeconds(asset.duration)
    guard duration.isFinite, duration > 0 else { fail("gif: could not read video duration") }
    let gen = AVAssetImageGenerator(asset: asset)
    gen.appliesPreferredTrackTransform = true
    gen.requestedTimeToleranceBefore = .zero
    gen.requestedTimeToleranceAfter = CMTime(value: 1, timescale: CMTimeScale(fps * 2))
    if maxWidth > 0 { gen.maximumSize = CGSize(width: maxWidth, height: 0) }

    let frameCount = max(1, Int((duration * Double(fps)).rounded(.down)))
    let outURL = URL(fileURLWithPath: output)
    try? FileManager.default.removeItem(at: outURL)
    guard let dest = CGImageDestinationCreateWithURL(outURL as CFURL, "com.compuserve.gif" as CFString, frameCount, nil)
    else { fail("gif: cannot create \(output)") }
    CGImageDestinationSetProperties(dest, [
        kCGImagePropertyGIFDictionary as String: [kCGImagePropertyGIFLoopCount as String: 0],
    ] as CFDictionary)
    let delay = 1.0 / Double(fps)
    let frameProps = [kCGImagePropertyGIFDictionary as String: [
        kCGImagePropertyGIFDelayTime as String: delay,
        kCGImagePropertyGIFUnclampedDelayTime as String: delay,
    ]] as CFDictionary

    // ImageIO expects exactly `frameCount` images: reuse the previous frame if one
    // can't be read, and pad the end if the first frames were unreadable.
    var added = 0, written = 0
    var last: CGImage?
    for f in 0..<frameCount {
        let t = CMTime(seconds: Double(f) * delay, preferredTimescale: 600)
        if let img = try? gen.copyCGImage(at: t, actualTime: nil) { last = img; added += 1 }
        guard let img = last else { continue }
        CGImageDestinationAddImage(dest, img, frameProps)
        written += 1
    }
    while let img = last, written < frameCount {
        CGImageDestinationAddImage(dest, img, frameProps)
        written += 1
    }
    guard added > 0, CGImageDestinationFinalize(dest) else {
        try? FileManager.default.removeItem(at: outURL)
        fail("gif: no frames could be converted")
    }
    exit(0)
}

// MARK: - main

let argv = CommandLine.arguments
guard argv.count >= 2 else { fail("usage: snaprec-helper list-displays | select | choose-format | record --out FILE ...") }
switch argv[1] {
case "list-displays": listDisplays()
case "select": selectRegion()
case "choose-format": chooseFormat(Array(argv.dropFirst(2)))
case "gif": makeGif(Array(argv.dropFirst(2)))
case "record": record(Array(argv.dropFirst(2)))
default: fail("unknown command \(argv[1])")
}
