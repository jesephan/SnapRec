// Renders SnapRec's black & white panda icons into assets/.
//   swiftc tools/make_icons.swift -o /tmp/mkicons && /tmp/mkicons assets
//
//   icon_1024.png          app icon
//   tray.png               menu bar icon (idle)
//   tray_rec_0..7.png      menu bar animation while recording
import AppKit

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "assets"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

let black = CGColor(gray: 0.05, alpha: 1)
let white = CGColor(gray: 1, alpha: 1)

// Panda face designed on a 100x100 canvas, y pointing down.
let head = CGRect(x: 9, y: 20, width: 82, height: 74)
let ears = [CGRect(x: 6, y: 6, width: 30, height: 30), CGRect(x: 64, y: 6, width: 30, height: 30)]
let eyeCenters = [CGPoint(x: 33, y: 54), CGPoint(x: 67, y: 54)]

func ellipse(center c: CGPoint, w: CGFloat, h: CGFloat, angle: CGFloat) -> CGPath {
    var t = CGAffineTransform(translationX: c.x, y: c.y).rotated(by: angle)
    return CGPath(ellipseIn: CGRect(x: -w / 2, y: -h / 2, width: w, height: h), transform: &t)
}

struct PandaStyle {
    var outline: CGFloat      // head outline width (canvas units)
    var eyeOpen: CGFloat      // 1 = open, 0 = closed (blink)
    var details: Bool         // mouth + eye shine (only at large sizes)
}

func drawPanda(_ ctx: CGContext, _ s: PandaStyle) {
    // ears
    ctx.setFillColor(black)
    for e in ears { ctx.fillEllipse(in: e) }
    // head
    ctx.setFillColor(white)
    ctx.fillEllipse(in: head)
    ctx.setStrokeColor(black)
    ctx.setLineWidth(s.outline)
    ctx.strokeEllipse(in: head.insetBy(dx: s.outline / 2, dy: s.outline / 2))
    // eye patches, tilted like a panda's
    ctx.setFillColor(black)
    for (i, c) in eyeCenters.enumerated() {
        ctx.addPath(ellipse(center: c, w: 20, h: 27, angle: (i == 0 ? 1 : -1) * 0.55))
        ctx.fillPath()
    }
    // eyes
    for (i, c) in eyeCenters.enumerated() {
        let ec = CGPoint(x: c.x + (i == 0 ? 2 : -2), y: c.y - 2)
        if s.eyeOpen > 0.05 {
            let h = 11 * s.eyeOpen
            ctx.setFillColor(white)
            ctx.fillEllipse(in: CGRect(x: ec.x - 5.5, y: ec.y - h / 2, width: 11, height: h))
            if s.details {
                ctx.setFillColor(black)
                ctx.fillEllipse(in: CGRect(x: ec.x - 3, y: ec.y - min(3, h / 2), width: 6, height: min(6, h)))
                ctx.setFillColor(white)
                ctx.fillEllipse(in: CGRect(x: ec.x - 0.5, y: ec.y - 2.5, width: 2.2, height: 2.2))
            }
        } else {
            ctx.setStrokeColor(white)
            ctx.setLineWidth(3)
            ctx.setLineCap(.round)
            ctx.move(to: CGPoint(x: ec.x - 5, y: ec.y + 1))
            ctx.addQuadCurve(to: CGPoint(x: ec.x + 5, y: ec.y + 1), control: CGPoint(x: ec.x, y: ec.y + 4))
            ctx.strokePath()
        }
    }
    // nose
    ctx.setFillColor(black)
    let nose = CGMutablePath()
    nose.move(to: CGPoint(x: 42, y: 69))
    nose.addQuadCurve(to: CGPoint(x: 58, y: 69), control: CGPoint(x: 50, y: 65))
    nose.addQuadCurve(to: CGPoint(x: 50, y: 78), control: CGPoint(x: 57, y: 76))
    nose.addQuadCurve(to: CGPoint(x: 42, y: 69), control: CGPoint(x: 43, y: 76))
    ctx.addPath(nose); ctx.fillPath()
    if s.details {
        ctx.setStrokeColor(black)
        ctx.setLineWidth(2.2)
        ctx.setLineCap(.round)
        ctx.move(to: CGPoint(x: 50, y: 78)); ctx.addLine(to: CGPoint(x: 50, y: 82))
        ctx.move(to: CGPoint(x: 42, y: 81))
        ctx.addQuadCurve(to: CGPoint(x: 50, y: 82), control: CGPoint(x: 46, y: 86))
        ctx.addQuadCurve(to: CGPoint(x: 58, y: 81), control: CGPoint(x: 54, y: 86))
        ctx.strokePath()
    }
}

func render(size: Int, draw: (CGContext) -> Void) -> CGImage {
    let ctx = CGContext(data: nil, width: size, height: size, bitsPerComponent: 8, bytesPerRow: 0,
                        space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    // flip to y-down
    ctx.translateBy(x: 0, y: CGFloat(size)); ctx.scaleBy(x: 1, y: -1)
    draw(ctx)
    return ctx.makeImage()!
}

func save(_ img: CGImage, _ name: String) {
    let url = URL(fileURLWithPath: outDir).appendingPathComponent(name)
    let dest = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, img, nil)
    CGImageDestinationFinalize(dest)
    print("wrote \(url.path)")
}

// App icon (1024): big panda face on a light rounded square
save(render(size: 1024) { ctx in
    let bg = CGPath(roundedRect: CGRect(x: 100, y: 100, width: 824, height: 824),
                    cornerWidth: 185, cornerHeight: 185, transform: nil)
    ctx.saveGState()
    ctx.addPath(bg); ctx.clip()
    let grad = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
                          colors: [CGColor(gray: 1, alpha: 1), CGColor(gray: 0.84, alpha: 1)] as CFArray,
                          locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 512, y: 100), end: CGPoint(x: 512, y: 924), options: [])
    // panda: 100 units -> 760 px, filling the tile
    ctx.translateBy(x: 132, y: 122); ctx.scaleBy(x: 7.6, y: 7.6)
    ctx.setShadow(offset: CGSize(width: 0, height: 1.5), blur: 4, color: CGColor(gray: 0, alpha: 0.35))
    ctx.beginTransparencyLayer(auxiliaryInfo: nil)
    drawPanda(ctx, PandaStyle(outline: 3, eyeOpen: 1, details: true))
    ctx.endTransparencyLayer()
    ctx.restoreGState()
}, "icon_1024.png")

// Menu bar icons (44px = 22pt @2x). The panda fills the whole icon.
func trayIcon(eyeOpen: CGFloat, dot: (radius: CGFloat, alpha: CGFloat)?) -> CGImage {
    render(size: 44) { ctx in
        ctx.scaleBy(x: 0.44, y: 0.44)
        drawPanda(ctx, PandaStyle(outline: 7, eyeOpen: eyeOpen, details: false))
        if let d = dot {
            // pulsing red "recording" dot in the bottom-right corner
            let c = CGPoint(x: 84, y: 84)
            ctx.setFillColor(white)
            ctx.fillEllipse(in: CGRect(x: c.x - d.radius - 4, y: c.y - d.radius - 4,
                                       width: (d.radius + 4) * 2, height: (d.radius + 4) * 2))
            ctx.setFillColor(CGColor(srgbRed: 1, green: 0.23, blue: 0.19, alpha: d.alpha))
            ctx.fillEllipse(in: CGRect(x: c.x - d.radius, y: c.y - d.radius, width: d.radius * 2, height: d.radius * 2))
        }
    }
}
save(trayIcon(eyeOpen: 1, dot: nil), "tray.png")

// Recording animation: the red dot pulses and the panda blinks once per loop.
let frames = 8
for i in 0..<frames {
    let phase = CGFloat(i) / CGFloat(frames)
    let pulse = (1 - cos(phase * 2 * .pi)) / 2           // 0 → 1 → 0
    let eye: CGFloat = i == 4 ? 0 : (i == 3 || i == 5 ? 0.45 : 1)
    save(trayIcon(eyeOpen: eye, dot: (radius: 9 + 5 * pulse, alpha: 0.55 + 0.45 * pulse)), "tray_rec_\(i).png")
}
