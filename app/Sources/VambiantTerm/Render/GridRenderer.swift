// Turns one locked grid view into two instance buffers and encodes them.
// Runs on the main actor; the grid lock is held only while instances are
// built, never across GPU work.

import AppKit
import CVambiantTerm
@preconcurrency import Metal
import QuartzCore
import simd

struct RendererError: Error, CustomStringConvertible {
    let message: String
    var description: String {
        message
    }
}

/// Must match `Instance` in Shaders.swift byte for byte (stride 80).
struct CellInstance {
    var origin: SIMD2<Float>
    var size: SIMD2<Float>
    var uv: SIMD4<Float>
    var fg: SIMD4<Float>
    var bg: SIMD4<Float>
    var flags: UInt32
    var pad0: UInt32 = 0
    var pad1: UInt32 = 0
    var pad2: UInt32 = 0

    static let hasGlyph: UInt32 = 1
    static let isColor: UInt32 = 2
}

struct Uniforms {
    var viewport: SIMD2<Float>
}

enum CursorStyle: String, Sendable {
    case block, bar, underline
}

/// Everything drawn over the cells besides the cells themselves.
struct GridOverlays {
    var decorations: [BlockDecoration] = []
    var matches: [MatchDecoration] = []
    var selection: [SelectionSpan] = []
    /// The hovered link, underlined.
    var links: [SelectionSpan] = []
}

@MainActor
final class GridRenderer {
    let device: MTLDevice
    let queue: MTLCommandQueue
    private(set) var fonts: FontSet
    var theme: Theme
    var cursorStyle: CursorStyle = .block
    var blockChrome = BlockChrome()
    /// `[font] bold_is_bright`: bold text in the 0–7 palette uses 8–15.
    var boldIsBright = false
    private(set) var scale: CGFloat
    private(set) var atlas: GlyphAtlas
    private let bgPipeline: MTLRenderPipelineState
    private let glyphPipeline: MTLRenderPipelineState
    private let sampler: MTLSamplerState
    private var bg: [CellInstance] = []
    private var glyphs: [CellInstance] = []
    private var bgBuffer: MTLBuffer?
    private var glyphBuffer: MTLBuffer?

    init(device: MTLDevice, fonts: FontSet, theme: Theme, scale: CGFloat) throws {
        self.device = device
        self.fonts = fonts
        self.theme = theme
        self.scale = scale
        guard let queue = device.makeCommandQueue() else {
            throw RendererError(message: "cannot create a Metal command queue")
        }
        self.queue = queue
        let library: MTLLibrary
        do {
            library = try device.makeLibrary(source: Shaders.source, options: nil)
        } catch {
            throw RendererError(message: "shader compilation failed: \(error)")
        }
        func pipeline(_ vertex: String, _ fragment: String, blend: Bool) throws -> MTLRenderPipelineState {
            let d = MTLRenderPipelineDescriptor()
            d.vertexFunction = library.makeFunction(name: vertex)
            d.fragmentFunction = library.makeFunction(name: fragment)
            let c = d.colorAttachments[0]!
            c.pixelFormat = .bgra8Unorm
            c.isBlendingEnabled = blend
            c.rgbBlendOperation = .add
            c.alphaBlendOperation = .add
            c.sourceRGBBlendFactor = .one
            c.sourceAlphaBlendFactor = .one
            c.destinationRGBBlendFactor = .oneMinusSourceAlpha
            c.destinationAlphaBlendFactor = .oneMinusSourceAlpha
            return try device.makeRenderPipelineState(descriptor: d)
        }
        bgPipeline = try pipeline("bg_vertex", "bg_fragment", blend: false)
        glyphPipeline = try pipeline("glyph_vertex", "glyph_fragment", blend: true)
        let sd = MTLSamplerDescriptor()
        sd.minFilter = .nearest
        sd.magFilter = .nearest
        guard let sampler = device.makeSamplerState(descriptor: sd) else {
            throw RendererError(message: "cannot create the atlas sampler")
        }
        self.sampler = sampler
        atlas = try GlyphAtlas(device: device, fonts: fonts, scale: scale)
    }

    /// `[font]` changed: new metrics, new atlas.
    func setFonts(_ newFonts: FontSet) throws {
        fonts = newFonts
        atlas = try GlyphAtlas(device: device, fonts: newFonts, scale: scale)
    }

    /// Backing scale changed: glyphs must be re-rasterised.
    func setScale(_ newScale: CGFloat) throws {
        guard newScale != scale else { return }
        scale = newScale
        atlas = try GlyphAtlas(device: device, fonts: fonts, scale: newScale)
    }

    var cellSize: CGSize {
        CGSize(width: fonts.metrics.width * scale, height: fonts.metrics.height * scale)
    }

    /// The grid's font as AppKit sees it, for chrome drawn with text views.
    var nsFont: NSFont {
        NSFont(name: CTFontCopyPostScriptName(fonts.regular) as String, size: fonts.size)
            ?? NSFont.monospacedSystemFont(ofSize: fonts.size, weight: .regular)
    }

    /// Builds the instance lists for `view` with the cell grid starting at
    /// `origin` (device pixels). Returns false if the atlas was cleared
    /// mid-build and the caller should run it again.
    func build(
        _ view: VtGridView, origin: CGPoint, focused: Bool, cursorOn: Bool = true, overlays: GridOverlays = GridOverlays()
    ) -> Bool {
        let generation = atlas.generation
        bg.removeAll(keepingCapacity: true)
        glyphs.removeAll(keepingCapacity: true)
        guard let cells = view.cells else { return true }
        let (selectedRows, failedRows) = tintedRows(overlays.decorations)
        let boldRows = Self.boldRows(overlays.decorations)
        let tint = theme.background.mixed(with: theme.selection, 0.35)
        let failedTint = theme.background.mixed(with: theme.palette[1], 0.08)
        let cols = Int(view.cols)
        let rows = Int(view.rows)
        let cw = Float(cellSize.width)
        let ch = Float(cellSize.height)
        let ox = Float(origin.x)
        let oy = Float(origin.y)
        let underlineY = ch - Float(fonts.metrics.baseline * scale) + Float(fonts.metrics.underlinePosition * scale)
        let decoration = Decoration(
            underlineY: underlineY,
            thickness: Float(max(1, (fonts.metrics.underlineThickness * scale).rounded()))
        )
        bg.reserveCapacity(cols * rows)
        glyphs.reserveCapacity(cols * rows)
        for r in 0 ..< rows {
            for c in 0 ..< cols {
                let cell = cells[r * cols + c]
                let attrs = cell.attrs
                if attrs & Attrs.wideSpacer != 0 {
                    continue
                }
                let wide = attrs & Attrs.wide != 0
                var fgWire = (cell.fg.0, cell.fg.1, cell.fg.2, cell.fg.3)
                if boldIsBright, attrs & Attrs.bold != 0, fgWire.0 == 1, fgWire.1 < 8 {
                    fgWire.1 += 8
                }
                var fg = theme.resolve(fgWire, isForeground: true)
                var bgc = theme.resolve((cell.bg.0, cell.bg.1, cell.bg.2, cell.bg.3), isForeground: false)
                if attrs & Attrs.inverse != 0 {
                    swap(&fg, &bgc)
                }
                if attrs & Attrs.dim != 0 {
                    fg = fg.scaled(0.6)
                }
                if cell.bg.0 == 0, attrs & Attrs.inverse == 0 {
                    if selectedRows.contains(r) {
                        bgc = tint
                    } else if failedRows.contains(r) {
                        bgc = failedTint
                    }
                }
                let isCursor = view.cursor_visible && Int(view.cursor_row) == r && Int(view.cursor_col) == c
                let solidCursor = isCursor && focused && cursorOn && cursorStyle == .block
                if solidCursor {
                    bgc = theme.cursor
                    fg = theme.background
                }
                let originPx = SIMD2<Float>(ox + Float(c) * cw, oy + Float(r) * ch)
                let size = SIMD2<Float>(wide ? cw * 2 : cw, ch)
                bg.append(CellInstance(origin: originPx, size: size, uv: .zero, fg: fg.simd, bg: bgc.simd, flags: 0))
                if isCursor, !focused {
                    appendHollowCursor(at: originPx, size: size)
                } else if isCursor, cursorOn, cursorStyle != .block {
                    appendThinCursor(at: originPx, size: size)
                }
                if attrs & Attrs.hidden != 0 {
                    continue
                }
                appendDecorations(attrs, at: originPx, size: size, fg: fg.simd, style: decoration)
                let style = FontStyle(bold: attrs & Attrs.bold != 0 || boldRows.contains(r), italic: attrs & Attrs.italic != 0)
                guard let g = atlas.glyph(for: GlyphKey(scalar: cell.ch, style: style, wide: wide)) else { continue }
                var flags = CellInstance.hasGlyph
                if g.isColor {
                    flags |= CellInstance.isColor
                }
                glyphs.append(CellInstance(origin: originPx, size: size, uv: g.uv, fg: fg.simd, bg: bgc.simd, flags: flags))
            }
        }
        appendOverlays(overlays, in: CellPlane(cells: cells, cols: cols, origin: SIMD2(ox, oy), cell: SIMD2(cw, ch)), rows: rows)
        return atlas.generation == generation
    }

    /// Block chrome, find highlights and the text selection, in that order.
    private func appendOverlays(_ overlays: GridOverlays, in plane: CellPlane, rows: Int) {
        for d in overlays.decorations {
            appendBlockChrome(d, cells: plane.cells, cols: plane.cols, origin: plane.origin, cell: plane.cell)
        }
        appendMatches(overlays.matches, cols: plane.cols, rows: rows, origin: plane.origin, cell: plane.cell)
        appendSelection(overlays.selection, cols: plane.cols, rows: rows, origin: plane.origin, cell: plane.cell)
        appendLinks(overlays.links, cols: plane.cols, rows: rows, origin: plane.origin, cell: plane.cell)
    }

    /// A one-pixel-ish underline in the link colour along the bottom of the span.
    private func appendLinks(_ spans: [SelectionSpan], cols: Int, rows: Int, origin: SIMD2<Float>, cell: SIMD2<Float>) {
        let colour = theme.palette[4].simd
        let thickness = max(1, (cell.y / 12).rounded())
        for s in spans where s.row < rows && s.col < cols {
            let width = Float(min(s.len, cols - s.col)) * cell.x
            let o = SIMD2(origin.x + Float(s.col) * cell.x, origin.y + Float(s.row + 1) * cell.y - thickness)
            bg.append(CellInstance(origin: o, size: SIMD2(width, thickness), uv: .zero, fg: colour, bg: colour, flags: 0))
        }
    }

    /// Find highlights sit over the cell backgrounds and under the glyphs
    /// (the glyph pass runs after): yellow for matches, the current one
    /// stronger, clamped to the grid.
    private func appendMatches(_ matches: [MatchDecoration], cols: Int, rows: Int, origin: SIMD2<Float>, cell: SIMD2<Float>) {
        let hit = theme.background.mixed(with: theme.palette[3], 0.45).simd
        let now = theme.background.mixed(with: theme.palette[3], 0.85).simd
        for m in matches where m.row < rows && m.col < cols {
            let width = Float(min(m.len, cols - m.col)) * cell.x
            let o = SIMD2(origin.x + Float(m.col) * cell.x, origin.y + Float(m.row) * cell.y)
            let c = m.current ? now : hit
            bg.append(CellInstance(origin: o, size: SIMD2(width, cell.y), uv: .zero, fg: c, bg: c, flags: 0))
        }
    }

    /// The text selection, in the theme's selection colour, over the cells.
    private func appendSelection(_ spans: [SelectionSpan], cols: Int, rows: Int, origin: SIMD2<Float>, cell: SIMD2<Float>) {
        let colour = theme.background.mixed(with: theme.selection, 0.8).simd
        for s in spans where s.row < rows && s.col < cols {
            let width = Float(min(s.len, cols - s.col)) * cell.x
            let o = SIMD2(origin.x + Float(s.col) * cell.x, origin.y + Float(s.row) * cell.y)
            bg.append(CellInstance(origin: o, size: SIMD2(width, cell.y), uv: .zero, fg: colour, bg: colour, flags: 0))
        }
    }

    /// The grid as the chrome drawers see it: cells plus pixel geometry.
    private struct CellPlane {
        let cells: UnsafePointer<VtCell>
        let cols: Int
        let origin: SIMD2<Float>
        let cell: SIMD2<Float>
    }

    /// Draws `text` into blank cells of a row from column 0 (the context
    /// line of a block in Warp mode). Stops at the first non-blank cell:
    /// chrome never covers content.
    private func appendText(_ text: String, row: Int, colour: RGBA, in plane: CellPlane) {
        let style = FontStyle(bold: false, italic: false)
        for (c, scalar) in text.unicodeScalars.enumerated() {
            guard c < plane.cols else { return }
            let under = plane.cells[row * plane.cols + c]
            guard under.ch == 32 || under.ch == 0, under.bg.0 == 0 else { return }
            guard scalar != " ", let g = atlas.glyph(for: GlyphKey(scalar: scalar.value, style: style, wide: false)) else {
                continue
            }
            let o = SIMD2(plane.origin.x + Float(c) * plane.cell.x, plane.origin.y + Float(row) * plane.cell.y)
            var flags = CellInstance.hasGlyph
            if g.isColor {
                flags |= CellInstance.isColor
            }
            glyphs.append(CellInstance(origin: o, size: plane.cell, uv: g.uv, fg: colour.simd, bg: theme.background.simd, flags: flags))
        }
    }

    /// Warp mode: the command line is bold, like Warp's block header.
    private static func boldRows(_ decorations: [BlockDecoration]) -> Set<Int> {
        var rows = Set<Int>()
        for d in decorations {
            if let r = d.commandRows {
                rows.formUnion(r)
            }
        }
        return rows
    }

    /// Rows a selected or failed block tints. Only default-background cells
    /// take the tint; cells that set their own colour keep it (chrome, not
    /// content).
    private func tintedRows(_ decorations: [BlockDecoration]) -> (selected: Set<Int>, failed: Set<Int>) {
        var selected = Set<Int>()
        var failed = Set<Int>()
        for d in decorations {
            if d.selected {
                selected.formUnion(d.firstRow ... d.lastRow)
            } else if d.status == .failed, blockChrome.failedTint {
                failed.formUnion(d.firstRow ... d.lastRow)
            }
        }
        return (selected, failed)
    }

    /// Gutter stripe in the left padding, a hairline above the header row,
    /// and the exit chip at the right end of the header when that space is
    /// blank (chrome never covers content).
    private func appendBlockChrome(
        _ d: BlockDecoration, cells: UnsafePointer<VtCell>, cols: Int, origin: SIMD2<Float>, cell: SIMD2<Float>
    ) {
        let colour: RGBA = switch d.status {
        case .ok: theme.palette[2].scaled(0.8)
        case .failed: theme.palette[1]
        case .unknown: theme.foreground.scaled(0.5)
        }
        let s = Float(scale)
        let stripeW = d.selected ? 4 * s : 2 * s
        let stripeX = max(0, origin.x - stripeW - 2 * s)
        let top = origin.y + Float(d.firstRow) * cell.y
        let height = Float(d.lastRow - d.firstRow + 1) * cell.y
        let stripe = (d.selected ? theme.selection : colour).simd
        bg.append(CellInstance(origin: SIMD2(stripeX, top), size: SIMD2(stripeW, height), uv: .zero, fg: stripe, bg: stripe, flags: 0))
        guard d.startsHere else { return }
        let width = origin.x * 2 + Float(cols) * cell.x
        if blockChrome.dividers {
            let hair = theme.background.mixed(with: theme.foreground, 0.18).simd
            bg.append(CellInstance(origin: SIMD2(0, top), size: SIMD2(width, max(1, s)), uv: .zero, fg: hair, bg: hair, flags: 0))
        }
        if d.bookmarked {
            // A tick in the right padding, in the cursor colour (the accent
            // that stays legible on light and dark themes): the bookmark
            // indicator Warp users look for at the edge.
            let mark = theme.cursor.simd
            let w = max(2 * s, min(origin.x - 2 * s, 4 * s))
            bg.append(CellInstance(
                origin: SIMD2(width - w - s, top + s), size: SIMD2(w, cell.y - 2 * s), uv: .zero, fg: mark, bg: mark, flags: 0
            ))
        }
        if let header = d.header, d.firstRow > 0 {
            let plane = CellPlane(cells: cells, cols: cols, origin: origin, cell: cell)
            appendText(header, row: d.firstRow - 1, colour: theme.foreground.scaled(0.55), in: plane)
        }
        guard let chip = d.chip else { return }
        let scalars = Array(chip.unicodeScalars)
        let startCol = cols - scalars.count - 1
        guard startCol > 0 else { return }
        for c in (startCol - 1) ..< cols where cells[d.firstRow * cols + c].ch != 32 || cells[d.firstRow * cols + c].bg.0 != 0 {
            return
        }
        let chipBg = theme.background.mixed(with: colour, 0.22).simd
        let chipOrigin = SIMD2(origin.x + Float(startCol - 1) * cell.x, top)
        bg.append(CellInstance(
            origin: chipOrigin, size: SIMD2(Float(scalars.count + 1) * cell.x, cell.y), uv: .zero, fg: chipBg, bg: chipBg, flags: 0
        ))
        for (i, scalar) in scalars.enumerated() {
            guard let g = atlas.glyph(for: GlyphKey(scalar: scalar.value, style: FontStyle(bold: true, italic: false), wide: false)) else {
                continue
            }
            let o = SIMD2(origin.x + Float(startCol + i) * cell.x, top)
            glyphs.append(CellInstance(origin: o, size: cell, uv: g.uv, fg: colour.simd, bg: chipBg, flags: CellInstance.hasGlyph))
        }
    }

    /// A one-pixel frame in the cursor colour for unfocused panes.
    private func appendHollowCursor(at origin: SIMD2<Float>, size: SIMD2<Float>) {
        let t = Float(max(1, scale))
        let cc = theme.cursor.simd
        let edges: [(SIMD2<Float>, SIMD2<Float>)] = [
            (origin, SIMD2(size.x, t)),
            (origin + SIMD2(0, size.y - t), SIMD2(size.x, t)),
            (origin, SIMD2(t, size.y)),
            (origin + SIMD2(size.x - t, 0), SIMD2(t, size.y)),
        ]
        for (o, s) in edges {
            bg.append(CellInstance(origin: o, size: s, uv: .zero, fg: cc, bg: cc, flags: 0))
        }
    }

    /// Bar or underline cursor: a thin rect in the cursor colour.
    private func appendThinCursor(at origin: SIMD2<Float>, size: SIMD2<Float>) {
        let t = Float(max(1, (2 * scale).rounded()))
        let cc = theme.cursor.simd
        let rect: (SIMD2<Float>, SIMD2<Float>) = cursorStyle == .bar
            ? (origin, SIMD2(t, size.y))
            : (origin + SIMD2(0, size.y - t), SIMD2(size.x, t))
        bg.append(CellInstance(origin: rect.0, size: rect.1, uv: .zero, fg: cc, bg: cc, flags: 0))
    }

    private struct Decoration {
        var underlineY: Float
        var thickness: Float
    }

    private func appendDecorations(
        _ attrs: UInt16, at origin: SIMD2<Float>, size: SIMD2<Float>, fg: SIMD4<Float>, style d: Decoration
    ) {
        if attrs & Attrs.underline != 0 {
            bg.append(CellInstance(
                origin: origin + SIMD2(0, d.underlineY), size: SIMD2(size.x, d.thickness), uv: .zero, fg: fg, bg: fg, flags: 0
            ))
        }
        if attrs & Attrs.strikeout != 0 {
            bg.append(CellInstance(
                origin: origin + SIMD2(0, size.y / 2), size: SIMD2(size.x, d.thickness), uv: .zero, fg: fg, bg: fg, flags: 0
            ))
        }
    }

    /// Encodes the instances built by `build` into `drawable` and commits.
    /// `onPresented` receives the presentation time on an arbitrary thread.
    func encode(
        into drawable: CAMetalDrawable,
        viewport: CGSize,
        onGPUDone: (@Sendable (CFTimeInterval) -> Void)? = nil,
        capture: (@Sendable (MTLTexture) -> Void)? = nil,
        onPresented: (@Sendable (CFTimeInterval) -> Void)? = nil
    ) {
        upload(&bgBuffer, bg)
        upload(&glyphBuffer, glyphs)
        guard let cmd = queue.makeCommandBuffer() else { return }
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = drawable.texture
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        let b = theme.background
        pass.colorAttachments[0].clearColor = MTLClearColor(red: Double(b.r), green: Double(b.g), blue: Double(b.b), alpha: 1)
        guard let enc = cmd.makeRenderCommandEncoder(descriptor: pass) else { return }
        var uniforms = Uniforms(viewport: SIMD2(Float(viewport.width), Float(viewport.height)))
        enc.setVertexBytes(&uniforms, length: MemoryLayout<Uniforms>.stride, index: 1)
        if let bgBuffer, !bg.isEmpty {
            enc.setRenderPipelineState(bgPipeline)
            enc.setVertexBuffer(bgBuffer, offset: 0, index: 0)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: bg.count)
        }
        if let glyphBuffer, !glyphs.isEmpty {
            enc.setRenderPipelineState(glyphPipeline)
            enc.setVertexBuffer(glyphBuffer, offset: 0, index: 0)
            enc.setFragmentTexture(atlas.texture, index: 0)
            enc.setFragmentSamplerState(sampler, index: 0)
            enc.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 6, instanceCount: glyphs.count)
        }
        enc.endEncoding()
        if let onPresented {
            drawable.addPresentedHandler { d in onPresented(d.presentedTime) }
        }
        if onGPUDone != nil || capture != nil {
            let texture = drawable.texture
            cmd.addCompletedHandler { c in
                capture?(texture)
                onGPUDone?(c.gpuEndTime)
            }
        }
        cmd.present(drawable)
        cmd.commit()
    }

    private func upload(_ buffer: inout MTLBuffer?, _ instances: [CellInstance]) {
        guard !instances.isEmpty else { return }
        let bytes = instances.count * MemoryLayout<CellInstance>.stride
        if buffer == nil || buffer!.length < bytes {
            buffer = device.makeBuffer(length: max(bytes, 64 * 1024), options: .storageModeShared)
        }
        instances.withUnsafeBytes { src in
            buffer!.contents().copyMemory(from: src.baseAddress!, byteCount: bytes)
        }
    }
}
