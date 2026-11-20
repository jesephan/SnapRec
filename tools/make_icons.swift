// Renders SnapRec's bird icons into assets/.
//   swiftc tools/make_icons.swift -o /tmp/mkicons && /tmp/mkicons assets
import AppKit

let outDir = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "assets"
try? FileManager.default.createDirectory(atPath: outDir, withIntermediateDirectories: true)

// Bird designed on a 100x100 canvas, y pointing down, facing right.
func birdParts() -> [CGPath] {
    let body = CGPath(ellipseIn: CGRect(x: 20, y: 38, width: 56, height: 40), transform: nil)
    let head = CGPath(ellipseIn: CGRect(x: 55, y: 22, width: 30, height: 30), transform: nil)
    let beak = CGMutablePath()
    beak.addLines(between: [CGPoint(x: 82, y: 31), CGPoint(x: 98, y: 38), CGPoint(x: 82, y: 44)])
    beak.closeSubpath()
    let tail = CGMutablePath()
    tail.addLines(between: [CGPoint(x: 30, y: 52), CGPoint(x: 2, y: 38), CGPoint(x: 8, y: 54),
                            CGPoint(x: 4, y: 66), CGPoint(x: 30, y: 68)])
    tail.closeSubpath()
    return [tail, body, head, beak]
}

func fillBird(_ ctx: CGContext) {
    ctx.beginTransparencyLayer(auxiliaryInfo: nil)
    for part in birdParts() { ctx.addPath(part); ctx.fillPath() }
    ctx.endTransparencyLayer()
}

func wing() -> CGPath {
    let p = CGMutablePath()
    p.move(to: CGPoint(x: 30, y: 52))
    p.addCurve(to: CGPoint(x: 64, y: 50), control1: CGPoint(x: 40, y: 38), control2: CGPoint(x: 56, y: 40))
    p.addCurve(to: CGPoint(x: 30, y: 52), control1: CGPoint(x: 58, y: 70), control2: CGPoint(x: 38, y: 70))
    p.closeSubpath()
    return p
}

func legs() -> CGPath {
    let p = CGMutablePath()
    p.move(to: CGPoint(x: 42, y: 76)); p.addLine(to: CGPoint(x: 40, y: 92))
    p.move(to: CGPoint(x: 34, y: 92)); p.addLine(to: CGPoint(x: 46, y: 92))
    p.move(to: CGPoint(x: 56, y: 76)); p.addLine(to: CGPoint(x: 57, y: 92))
    p.move(to: CGPoint(x: 51, y: 92)); p.addLine(to: CGPoint(x: 63, y: 92))
    return p
}

let eye = CGRect(x: 69, y: 30, width: 7, height: 7)

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

func rgb(_ hex: UInt32, _ a: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255,
            blue: CGFloat(hex & 0xff) / 255, alpha: a)
}

// App icon (1024)
save(render(size: 1024) { ctx in
    let bg = CGPath(roundedRect: CGRect(x: 100, y: 100, width: 824, height: 824),
                    cornerWidth: 185, cornerHeight: 185, transform: nil)
    ctx.addPath(bg); ctx.clip()
    let grad = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!,
                          colors: [rgb(0x6EC6FF), rgb(0x1E6FD9)] as CFArray, locations: [0, 1])!
    ctx.drawLinearGradient(grad, start: CGPoint(x: 512, y: 100), end: CGPoint(x: 512, y: 924), options: [])
    // bird: 100 units -> 640 px
    ctx.translateBy(x: 192, y: 190); ctx.scaleBy(x: 6.4, y: 6.4)
    ctx.setShadow(offset: CGSize(width: 0, height: 2), blur: 6, color: rgb(0x0A2A5A, 0.35))
    ctx.setStrokeColor(rgb(0x3A2A1A)); ctx.setLineWidth(3); ctx.setLineCap(.round)
    ctx.addPath(legs()); ctx.strokePath()
    ctx.setFillColor(rgb(0xFF9F1C)); fillBird(ctx)
    ctx.setShadow(offset: .zero, blur: 0, color: nil)
    ctx.setFillColor(rgb(0xFFD23F))
    ctx.addLines(between: [CGPoint(x: 82, y: 31), CGPoint(x: 98, y: 38), CGPoint(x: 82, y: 44)]); ctx.fillPath()
    ctx.setFillColor(rgb(0xE06C00)); ctx.addPath(wing()); ctx.fillPath()
    ctx.setFillColor(rgb(0xFFFFFF)); ctx.fillEllipse(in: eye.insetBy(dx: -1.5, dy: -1.5))
    ctx.setFillColor(rgb(0x111111)); ctx.fillEllipse(in: eye.insetBy(dx: 1, dy: 1))
}, "icon_1024.png")

// Menu bar icons (44px = 22pt @2x)
func trayIcon(_ color: CGColor) -> CGImage {
    render(size: 44) { ctx in
        ctx.translateBy(x: 1, y: 2); ctx.scaleBy(x: 0.42, y: 0.42)
        ctx.setFillColor(color); fillBird(ctx)
        ctx.setStrokeColor(color); ctx.setLineWidth(6); ctx.setLineCap(.round)
        ctx.addPath(legs()); ctx.strokePath()
        ctx.setBlendMode(.clear); ctx.fillEllipse(in: eye.insetBy(dx: -1, dy: -1))
        ctx.addPath(wing()); ctx.setLineWidth(4); ctx.strokePath()
    }
}
save(trayIcon(rgb(0x000000)), "tray.png")
save(trayIcon(rgb(0xFF3B30)), "tray_rec.png")
