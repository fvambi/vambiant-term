// Colours. The default is `vambiant-dark` from docs/09 verbatim; theme
// files and system light/dark following arrive with vt-config. Cell
// colours cross the boundary as `[kind, a, b, c]` (0 default, 1 indexed,
// 2 rgb) and are resolved here, once per cell per frame.

import simd

struct RGBA: Equatable, Sendable {
    var r: Float, g: Float, b: Float, a: Float

    init(r: Float, g: Float, b: Float, a: Float = 1) {
        self.r = r
        self.g = g
        self.b = b
        self.a = a
    }

    init(hex: UInt32) {
        self.init(
            r: Float((hex >> 16) & 0xFF) / 255,
            g: Float((hex >> 8) & 0xFF) / 255,
            b: Float(hex & 0xFF) / 255
        )
    }

    init(r8: UInt8, g8: UInt8, b8: UInt8) {
        self.init(r: Float(r8) / 255, g: Float(g8) / 255, b: Float(b8) / 255)
    }

    /// `#RRGGBB`, case-insensitive; nil when malformed.
    init?(hexString: String) {
        let digits = hexString.hasPrefix("#") ? String(hexString.dropFirst()) : hexString
        guard digits.count == 6, let n = UInt32(digits, radix: 16) else { return nil }
        self.init(hex: n)
    }

    var simd: SIMD4<Float> {
        SIMD4(r, g, b, a)
    }

    func scaled(_ k: Float) -> RGBA {
        RGBA(r: r * k, g: g * k, b: b * k, a: a)
    }
}

struct Theme: Sendable {
    let name: String
    let background: RGBA
    let foreground: RGBA
    let cursor: RGBA
    let selection: RGBA
    /// 0–15: normal then bright ANSI colours; 16–255 the xterm cube and ramp.
    let palette: [RGBA]

    static let vambiantDark = Theme(
        name: "vambiant-dark",
        background: RGBA(hex: 0x0D0F12),
        foreground: RGBA(hex: 0xD8DEE9),
        cursor: RGBA(hex: 0x7AA2F7),
        selection: RGBA(hex: 0x2A2F3A),
        ansi: [
            0x1A1D23, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF,
            0x4B5263, 0xFF7B86, 0xA9D977, 0xF0D18A, 0x79C0FF, 0xD7A3FF, 0x6FD3DE, 0xE6E9EF,
        ]
    )

    init(name: String, background: RGBA, foreground: RGBA, cursor: RGBA, selection: RGBA, ansi: [UInt32]) {
        precondition(ansi.count == 16)
        self.name = name
        self.background = background
        self.foreground = foreground
        self.cursor = cursor
        self.selection = selection
        var p = ansi.map(RGBA.init(hex:))
        // xterm 6×6×6 cube then the 24-step grey ramp; identical in every
        // terminal, so not themeable.
        let steps: [UInt8] = [0, 95, 135, 175, 215, 255]
        for r in 0 ..< 6 {
            for g in 0 ..< 6 {
                for b in 0 ..< 6 {
                    p.append(RGBA(r8: steps[r], g8: steps[g], b8: steps[b]))
                }
            }
        }
        for i in 0 ..< 24 {
            let v = UInt8(8 + i * 10)
            p.append(RGBA(r8: v, g8: v, b8: v))
        }
        palette = p
    }

    /// From a `themes/*.toml` file as the daemon serves it. Nil when a
    /// colour does not parse — the daemon warns about those separately.
    init?(file: ThemeFile) {
        let all = [file.background, file.foreground, file.cursor, file.selection] + file.normal.ordered + file.bright.ordered
        let parsed = all.compactMap(RGBA.init(hexString:))
        guard parsed.count == all.count else { return nil }
        self.init(
            name: file.name,
            background: parsed[0],
            foreground: parsed[1],
            cursor: parsed[2],
            selection: parsed[3],
            ansi: parsed[4...].map { c in
                UInt32(c.r * 255 + 0.5) << 16 | UInt32(c.g * 255 + 0.5) << 8 | UInt32(c.b * 255 + 0.5)
            }
        )
    }

    /// Resolves a wire colour; `isForeground` picks the default.
    func resolve(_ c: (UInt8, UInt8, UInt8, UInt8), isForeground: Bool) -> RGBA {
        switch c.0 {
        case 1: return palette[Int(c.1)]
        case 2: return RGBA(r8: c.1, g8: c.2, b8: c.3)
        default: return isForeground ? foreground : background
        }
    }
}
