// Turns one locked grid view into two instance buffers and encodes them.
// Runs on the main actor; the grid lock is held only while instances are
// built, never across GPU work.

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

@MainActor
final class GridRenderer {
    let device: MTLDevice
    let queue: MTLCommandQueue
    let fonts: FontSet
    var theme: Theme
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

    /// Backing scale changed: glyphs must be re-rasterised.
    func setScale(_ newScale: CGFloat) throws {
        guard newScale != scale else { return }
        scale = newScale
        atlas = try GlyphAtlas(device: device, fonts: fonts, scale: newScale)
    }

    var cellSize: CGSize {
        CGSize(width: fonts.metrics.width * scale, height: fonts.metrics.height * scale)
    }

    /// Builds the instance lists for `view` with the cell grid starting at
    /// `origin` (device pixels). Returns false if the atlas was cleared
    /// mid-build and the caller should run it again.
    func build(_ view: VtGridView, origin: CGPoint, focused: Bool) -> Bool {
        let generation = atlas.generation
        bg.removeAll(keepingCapacity: true)
        glyphs.removeAll(keepingCapacity: true)
        guard let cells = view.cells else { return true }
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
                var fg = theme.resolve((cell.fg.0, cell.fg.1, cell.fg.2, cell.fg.3), isForeground: true)
                var bgc = theme.resolve((cell.bg.0, cell.bg.1, cell.bg.2, cell.bg.3), isForeground: false)
                if attrs & Attrs.inverse != 0 {
                    swap(&fg, &bgc)
                }
                if attrs & Attrs.dim != 0 {
                    fg = fg.scaled(0.6)
                }
                let isCursor = view.cursor_visible && Int(view.cursor_row) == r && Int(view.cursor_col) == c
                if isCursor, focused {
                    bgc = theme.cursor
                    fg = theme.background
                }
                let originPx = SIMD2<Float>(ox + Float(c) * cw, oy + Float(r) * ch)
                let size = SIMD2<Float>(wide ? cw * 2 : cw, ch)
                bg.append(CellInstance(origin: originPx, size: size, uv: .zero, fg: fg.simd, bg: bgc.simd, flags: 0))
                if isCursor, !focused {
                    appendHollowCursor(at: originPx, size: size)
                }
                if attrs & Attrs.hidden != 0 {
                    continue
                }
                appendDecorations(attrs, at: originPx, size: size, fg: fg.simd, style: decoration)
                let style = FontStyle(bold: attrs & Attrs.bold != 0, italic: attrs & Attrs.italic != 0)
                guard let g = atlas.glyph(for: GlyphKey(scalar: cell.ch, style: style, wide: wide)) else { continue }
                var flags = CellInstance.hasGlyph
                if g.isColor {
                    flags |= CellInstance.isColor
                }
                glyphs.append(CellInstance(origin: originPx, size: size, uv: g.uv, fg: fg.simd, bg: bgc.simd, flags: flags))
            }
        }
        return atlas.generation == generation
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
