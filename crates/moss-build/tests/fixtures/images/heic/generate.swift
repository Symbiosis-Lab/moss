// Writes the HEIC test fixtures through ImageIO: tiny, generated, never a photo.
// Run on a Mac: swiftc generate.swift -o /tmp/gen && /tmp/gen .
import Foundation
import CoreGraphics
import ImageIO

let out = CommandLine.arguments[1]

// w×h, top-left quadrant red, top-right green, bottom-left blue, bottom-right white.
func quadrants(_ w: Int, _ h: Int, alpha: CGFloat = 1) -> CGImage {
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    // CoreGraphics' origin is bottom-left.
    let hw = w / 2, hh = h / 2
    ctx.setFillColor(red: 1, green: 0, blue: 0, alpha: alpha); ctx.fill(CGRect(x: 0, y: hh, width: hw, height: h - hh))
    ctx.setFillColor(red: 0, green: 1, blue: 0, alpha: alpha); ctx.fill(CGRect(x: hw, y: hh, width: w - hw, height: h - hh))
    ctx.setFillColor(red: 0, green: 0, blue: 1, alpha: alpha); ctx.fill(CGRect(x: 0, y: 0, width: hw, height: hh))
    ctx.setFillColor(red: 1, green: 1, blue: 1, alpha: alpha); ctx.fill(CGRect(x: hw, y: 0, width: w - hw, height: hh))
    return ctx.makeImage()!
}

func write(_ name: String, _ image: CGImage, orientation: Int? = nil) {
    let url = URL(fileURLWithPath: out).appendingPathComponent(name) as CFURL
    let dest = CGImageDestinationCreateWithURL(url, "public.heic" as CFString, 1, nil)!
    var props: [CFString: Any] = [kCGImageDestinationLossyCompressionQuality: 0.9]
    if let o = orientation { props[kCGImagePropertyOrientation] = o }
    CGImageDestinationAddImage(dest, image, props as CFDictionary)
    precondition(CGImageDestinationFinalize(dest), "could not write \(name)")
}

write("quadrants-64x48.heic", quadrants(64, 48))
write("quadrants-64x48-orient6.heic", quadrants(64, 48), orientation: 6)
write("translucent-32x32.heic", quadrants(32, 32, alpha: 0.5))
write("tiled-1100x700.heic", quadrants(1100, 700))
