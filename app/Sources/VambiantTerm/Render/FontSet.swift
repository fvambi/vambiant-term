// Fonts and cell metrics. Defaults follow docs/09 `[font]`: the family
// cascade "Berkeley Mono" → "SF Mono" → "Menlo", 13 pt, line height 1.2.
// CoreText's own cascade list handles glyphs the primary font lacks, so
// per-run font fallback is not reimplemented here.

import CoreText
import Foundation

struct FontStyle: Hashable, Sendable {
    var bold = false
    var italic = false
}

struct CellMetrics: Equatable, Sendable {
    /// Advance of one cell in points.
    let width: CGFloat
    /// Cell height in points including the line-height multiplier.
    let height: CGFloat
    /// Baseline distance from the cell bottom, in points.
    let baseline: CGFloat
    let underlinePosition: CGFloat
    let underlineThickness: CGFloat
}

final class FontSet: @unchecked Sendable {
    let regular: CTFont
    let size: CGFloat
    let lineHeight: CGFloat
    let metrics: CellMetrics
    private var variants: [FontStyle: CTFont] = [:]
    private let lock = NSLock()

    static let defaultFamilies = ["Berkeley Mono", "SF Mono", "Menlo"]

    init(families: [String] = FontSet.defaultFamilies, size: CGFloat = 13, lineHeight: CGFloat = 1.2) {
        self.size = size
        self.lineHeight = lineHeight
        let regular = Self.firstInstalled(families, size: size)
        self.regular = regular
        let ascent = CTFontGetAscent(regular)
        let descent = CTFontGetDescent(regular)
        let leading = CTFontGetLeading(regular)
        var glyph = CGGlyph(0)
        var advance = CGSize.zero
        var ch = UniChar(0x4D) // "M"
        CTFontGetGlyphsForCharacters(regular, &ch, &glyph, 1)
        CTFontGetAdvancesForGlyphs(regular, .horizontal, &glyph, &advance, 1)
        let natural = ascent + descent + leading
        let height = ceil(natural * lineHeight)
        // Centre the natural line box in the taller cell so the extra
        // leading is split above and below, as Ghostty does.
        let baseline = floor((height - natural) / 2 + descent)
        metrics = CellMetrics(
            width: ceil(advance.width),
            height: height,
            baseline: baseline,
            underlinePosition: max(1, -CTFontGetUnderlinePosition(regular)),
            underlineThickness: max(1, CTFontGetUnderlineThickness(regular))
        )
    }

    /// The first family CoreText can actually find; Menlo ships with macOS
    /// and is the guaranteed end of the cascade.
    private static func firstInstalled(_ families: [String], size: CGFloat) -> CTFont {
        for family in families {
            let attrs: [CFString: Any] = [kCTFontFamilyNameAttribute: family]
            let desc = CTFontDescriptorCreateWithAttributes(attrs as CFDictionary)
            let font = CTFontCreateWithFontDescriptor(desc, size, nil)
            let name = CTFontCopyFamilyName(font) as String
            if name == family {
                return font
            }
        }
        return CTFontCreateWithName("Menlo" as CFString, size, nil)
    }

    func font(for style: FontStyle) -> CTFont {
        if !style.bold, !style.italic {
            return regular
        }
        lock.lock()
        defer { lock.unlock() }
        if let f = variants[style] {
            return f
        }
        var traits: CTFontSymbolicTraits = []
        if style.bold {
            traits.insert(.boldTrait)
        }
        if style.italic {
            traits.insert(.italicTrait)
        }
        // A family without the trait keeps the regular face rather than
        // synthesising one; the atlas notes bold as a colour tweak then.
        let f = CTFontCreateCopyWithSymbolicTraits(regular, size, nil, traits, traits) ?? regular
        variants[style] = f
        return f
    }

    var familyName: String {
        CTFontCopyFamilyName(regular) as String
    }
}
