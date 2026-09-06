import CVambiantTerm
import Metal
import Testing
@testable import VambiantTerm

struct ThemeTests {
    @Test func paletteHas256Entries() {
        #expect(Theme.vambiantDark.palette.count == 256)
        #expect(Theme.vambiantDark.palette[16] == RGBA(r8: 0, g8: 0, b8: 0))
        #expect(Theme.vambiantDark.palette[231] == RGBA(r8: 255, g8: 255, b8: 255))
        #expect(Theme.vambiantDark.palette[255] == RGBA(r8: 238, g8: 238, b8: 238))
    }

    @Test func wireColoursResolve() {
        let t = Theme.vambiantDark
        #expect(t.resolve((0, 0, 0, 0), isForeground: true) == t.foreground)
        #expect(t.resolve((0, 0, 0, 0), isForeground: false) == t.background)
        #expect(t.resolve((1, 1, 0, 0), isForeground: true) == RGBA(hex: 0xE06C75))
        #expect(t.resolve((2, 10, 20, 30), isForeground: true) == RGBA(r8: 10, g8: 20, b8: 30))
    }
}

struct FontTests {
    @Test func metricsAreIntegralAndPositive() {
        let f = FontSet()
        #expect(f.metrics.width > 0)
        #expect(f.metrics.height > f.metrics.width)
        #expect(f.metrics.width == f.metrics.width.rounded())
        #expect(f.metrics.height == f.metrics.height.rounded())
    }

    @Test func unknownFamiliesFallBackToMenlo() {
        let f = FontSet(families: ["No Such Font 12345"])
        #expect(f.familyName == "Menlo")
    }

    @Test func boldVariantIsCached() {
        let f = FontSet()
        let a = f.font(for: FontStyle(bold: true))
        let b = f.font(for: FontStyle(bold: true))
        #expect(a === b)
    }
}

struct InstanceLayoutTests {
    @Test func instanceStrideMatchesTheShaderStruct() {
        #expect(MemoryLayout<CellInstance>.stride == 80)
        #expect(MemoryLayout<CellInstance>.offset(of: \.uv) == 16)
        #expect(MemoryLayout<CellInstance>.offset(of: \.fg) == 32)
        #expect(MemoryLayout<CellInstance>.offset(of: \.bg) == 48)
        #expect(MemoryLayout<CellInstance>.offset(of: \.flags) == 64)
    }
}

@MainActor struct RendererTests {
    /// A synthetic 3×2 grid through the real build path (needs a GPU).
    @Test func buildsOneBackgroundPerCellAndGlyphsForText() throws {
        guard let device = MTLCreateSystemDefaultDevice() else { return }
        let r = try GridRenderer(device: device, fonts: FontSet(), theme: .vambiantDark, scale: 2)
        var cells = [VtCell](repeating: VtCell(ch: 32, fg: (0, 0, 0, 0), bg: (0, 0, 0, 0), attrs: 0, reserved: 0), count: 6)
        cells[0].ch = UInt32(UInt8(ascii: "h"))
        cells[1].ch = UInt32(UInt8(ascii: "i"))
        cells[1].attrs = Attrs.underline
        cells[3].ch = 0x4E16 // 世, wide
        cells[3].attrs = Attrs.wide
        cells[4].attrs = Attrs.wideSpacer
        let ok: Bool = cells.withUnsafeBufferPointer { buf in
            let view = VtGridView(
                cells: buf.baseAddress,
                cols: 3,
                rows: 2,
                cursor_row: 1,
                cursor_col: 2,
                cursor_visible: true,
                seq: 1,
                disconnected: false,
                top: 0,
                total: 2
            )
            return r.build(view, origin: .zero, focused: true)
        }
        #expect(ok)
        #expect(r.atlas.slotsInUse == 4, "h, i and a two-slot wide glyph")
    }

    @Test func atlasMarksEmojiAsColour() throws {
        guard let device = MTLCreateSystemDefaultDevice() else { return }
        let atlas = try GlyphAtlas(device: device, fonts: FontSet(), scale: 2)
        let g = atlas.glyph(for: GlyphKey(scalar: 0x1F600, style: FontStyle(), wide: true))
        #expect(g?.isColor == true)
        let a = atlas.glyph(for: GlyphKey(scalar: UInt32(UInt8(ascii: "a")), style: FontStyle(), wide: false))
        #expect(a?.isColor == false)
        #expect(atlas.glyph(for: GlyphKey(scalar: 32, style: FontStyle(), wide: false)) == nil)
    }
}
