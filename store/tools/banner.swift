// Exits 1 when a phone screenshot has a notification banner at the top
// (another app's notification would end up in the store image), 0 if not.
//
//   swift store/tools/banner.swift shot.png
import CoreGraphics
import Foundation
import ImageIO

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { exit(2) }
let width = image.width, height = image.height
var pixels = [UInt8](repeating: 0, count: width * height * 4)
let context = CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))

func luminance(_ x: Int, _ y: Int) -> Double {
    let i = (y * width + x) * 4
    return 0.299 * Double(pixels[i]) + 0.587 * Double(pixels[i + 1]) + 0.114 * Double(pixels[i + 2])
}

// The screens' own background along both edges, just under the status bar,
// is near white; a banner's gray material covers it.
let scale = Double(width) / 1170
var dark = 0
for y in stride(from: 200, through: 300, by: 20) {
    for x in [36, 1134] where luminance(Int(Double(x) * scale), Int(Double(y) * scale)) < 215 { dark += 1 }
}
exit(dark >= 6 ? 1 : 0)
