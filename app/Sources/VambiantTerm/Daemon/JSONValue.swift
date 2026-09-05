// A plain JSON tree for values whose shape is defined on the Rust side
// (the config document, defaults). Dotted-path lookup mirrors the keys the
// daemon accepts in `config.set`.

import Foundation

indirect enum JSONValue: Codable, Equatable, Hashable, Sendable {
    case object([String: JSONValue])
    case array([JSONValue])
    case string(String)
    case number(Double)
    case bool(Bool)
    case null

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self = .null
        } else if let b = try? c.decode(Bool.self) {
            self = .bool(b)
        } else if let n = try? c.decode(Double.self) {
            self = .number(n)
        } else if let s = try? c.decode(String.self) {
            self = .string(s)
        } else if let a = try? c.decode([JSONValue].self) {
            self = .array(a)
        } else if let o = try? c.decode([String: JSONValue].self) {
            self = .object(o)
        } else {
            throw DecodingError.dataCorruptedError(in: c, debugDescription: "not JSON")
        }
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case let .object(o): try c.encode(o)
        case let .array(a): try c.encode(a)
        case let .string(s): try c.encode(s)
        case let .number(n):
            if n == n.rounded(), abs(n) < 1e15 {
                try c.encode(Int64(n))
            } else {
                try c.encode(n)
            }
        case let .bool(b): try c.encode(b)
        case .null: try c.encodeNil()
        }
    }

    /// `font.size` → the nested value, or nil.
    subscript(path path: String) -> JSONValue? {
        var current = self
        for part in path.split(separator: ".") {
            guard case let .object(o) = current, let next = o[String(part)] else { return nil }
            current = next
        }
        return current
    }

    var stringValue: String? {
        if case let .string(s) = self {
            return s
        }
        return nil
    }

    var doubleValue: Double? {
        if case let .number(n) = self {
            return n
        }
        return nil
    }

    var boolValue: Bool? {
        if case let .bool(b) = self {
            return b
        }
        return nil
    }

    var stringArray: [String]? {
        if case let .array(a) = self {
            return a.compactMap(\.stringValue)
        }
        return nil
    }

    /// Human-readable, for "reset to default" hints.
    var display: String {
        switch self {
        case let .string(s): return s
        case let .number(n): return n == n.rounded() ? String(Int64(n)) : String(n)
        case let .bool(b): return b ? "on" : "off"
        case let .array(a): return a.map(\.display).joined(separator: ", ")
        case .object: return "{…}"
        case .null: return "—"
        }
    }

    /// Decodes into a concrete type by round-tripping through JSON.
    func decode<T: Decodable>(_ type: T.Type) throws -> T {
        try JSONDecoder().decode(type, from: JSONEncoder().encode(self))
    }
}
