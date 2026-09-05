// Glyph rasterisation into one texture. Every glyph is drawn into a
// fixed-size slot (one or two cells wide at the current backing scale),
// so packing is a grid, not a shelf — cheaper and never fragments. Colour
// glyphs (Apple Color Emoji) keep their colour; monochrome glyphs are
// stored white and tinted in the fragment shader.

import CoreGraphics
import CoreText
import CVambiantTerm
import Metal
import simd

struct GlyphKey: Hashable {
    let scalar: UInt32
    let style: FontStyle
    let wide: Bool
}

struct GlyphRect {
    /// x0, y0, x1, y1 in texture coordinates.
    let uv: SIMD4<Float>
    let isColor: Bool
}

@MainActor
final class GlyphAtlas {
    let texture: MTLTexture
    private let fonts: FontSet
    private let scale: CGFloat
    private let slotWidth: Int
    private let slotHeight: Int
    private let columns: Int
    private let rows: Int
    private var next = 0
    private var glyphs: [GlyphKey: GlyphRect?] = [:]
    /// Bumped whenever the atlas is cleared; a frame built across a bump
    /// must be rebuilt.
    private(set) var generation = 0

    static let size = 2048

    init(device: MTLDevice, fonts: FontSet, scale: CGFloat) throws {
        self.fonts = fonts
        self.scale = scale
        slotWidth = Int(ceil(fonts.metrics.width * scale))
        slotHeight = Int(ceil(fonts.metrics.height * scale))
        columns = Self.size / slotWidth
        rows = Self.size / slotHeight
        let desc = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm, width: Self.size, height: Self.size, mipmapped: false
        )
        desc.usage = .shaderRead
        guard let texture = device.makeTexture(descriptor: desc) else {
            throw RendererError(message: "cannot allocate the \(Self.size)² glyph atlas")
        }
        self.texture = texture
    }

    var slotsInUse: Int {
        next
    }

    /// Looks up or rasterises. `nil` means "draw nothing" (blank glyph).
    func glyph(for key: GlyphKey) -> GlyphRect? {
        if let cached = glyphs[key] {
            return cached
        }
        let rect = rasterise(key)
        glyphs[key] = .some(rect)
        return rect
    }

    private func rasterise(_ key: GlyphKey) -> GlyphRect? {
        guard let scalar = Unicode.Scalar(key.scalar), scalar != " " else { return nil }
        let cells = key.wide ? 2 : 1
        if next + cells > columns * rows || (next % columns) + cells > columns {
            if next + cells > columns * rows {
                glyphs.removeAll(keepingCapacity: true)
                generation += 1
                next = 0
            } else {
                next += columns - (next % columns) // wide glyph wraps to the next row
            }
        }
        let width = slotWidth * cells
        let height = slotHeight
        guard let ctx = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
        ) else { return nil }
        ctx.setAllowsFontSmoothing(true)
        ctx.setShouldSmoothFonts(false)
        ctx.setAllowsAntialiasing(true)
        ctx.setShouldAntialias(true)
        ctx.scaleBy(x: scale, y: scale)
        let font = fonts.font(for: key.style)
        let attrs: [CFString: Any] = [
            kCTFontAttributeName: font,
            kCTForegroundColorAttributeName: CGColor(gray: 1, alpha: 1),
        ]
        let string = CFAttributedStringCreate(nil, String(scalar) as CFString, attrs as CFDictionary)!
        let line = CTLineCreateWithAttributedString(string)
        var isColor = false
        for run in CTLineGetGlyphRuns(line) as! [CTRun] {
            let runAttrs = CTRunGetAttributes(run) as NSDictionary
            if let runFont = runAttrs[kCTFontAttributeName] {
                let f = runFont as! CTFont
                if CTFontGetSymbolicTraits(f).contains(.colorGlyphsTrait) {
                    isColor = true
                }
            }
        }
        ctx.textPosition = CGPoint(x: 0, y: fonts.metrics.baseline)
        CTLineDraw(line, ctx)
        guard let data = ctx.data else { return nil }
        let slot = next
        next += cells
        let x = (slot % columns) * slotWidth
        let y = (slot / columns) * slotHeight
        texture.replace(
            region: MTLRegionMake2D(x, y, width, height), mipmapLevel: 0,
            withBytes: data, bytesPerRow: ctx.bytesPerRow
        )
        // A CGBitmapContext stores its first row at the top of the image
        // (only its coordinate origin is bottom-left), so rows upload to
        // the texture as-is and the quad samples top-down.
        let s = Float(Self.size)
        let uv = SIMD4<Float>(Float(x) / s, Float(y) / s, Float(x + width) / s, Float(y + height) / s)
        return GlyphRect(uv: uv, isColor: isColor)
    }
}
