// Self-screenshot for diagnostics: reads a BGRA drawable texture back and
// writes a PNG. Only reachable through VAMBIANT_TERM_SCREENSHOT.

import CoreGraphics
import Foundation
import ImageIO
@preconcurrency import Metal
import UniformTypeIdentifiers

enum Screenshot {
    private nonisolated(unsafe) static var written = false

    static func write(_ texture: MTLTexture, to path: String) {
        guard !written else { return }
        written = true
        let w = texture.width
        let h = texture.height
        var bytes = [UInt8](repeating: 0, count: w * h * 4)
        texture.getBytes(&bytes, bytesPerRow: w * 4, from: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0)
        let data = Data(bytes)
        guard let provider = CGDataProvider(data: data as CFData),
              let image = CGImage(
                  width: w, height: h, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: w * 4,
                  space: CGColorSpace(name: CGColorSpace.sRGB)!,
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent
              ),
              let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil)
        else {
            NSLog("screenshot: cannot encode %dx%d texture", w, h)
            return
        }
        CGImageDestinationAddImage(dest, image, nil)
        CGImageDestinationFinalize(dest)
        NSLog("screenshot: wrote %@", path)
    }
}
