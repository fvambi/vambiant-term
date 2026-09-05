// Metal shaders as source: the Command Line Tools ship no `metal`
// compiler, so the library is built at runtime with
// `makeLibrary(source:)` (docs/10 §2). Two instanced passes over the same
// instance layout: background quads, then glyph quads sampling the atlas.

enum Shaders {
    static let source = """
    #include <metal_stdlib>
    using namespace metal;

    struct Instance {
        float2 origin;
        float2 size;
        float4 uv;
        float4 fg;
        float4 bg;
        uint   flags;
        uint   pad0;
        uint   pad1;
        uint   pad2;
    };

    struct Uniforms {
        float2 viewport;
    };

    struct VOut {
        float4 position [[position]];
        float2 uv;
        float4 color;
        uint   flags [[flat]];
    };

    constant float2 corners[6] = {
        float2(0, 0), float2(1, 0), float2(0, 1),
        float2(0, 1), float2(1, 0), float2(1, 1)
    };

    static inline float4 to_clip(float2 p, float2 viewport) {
        return float4(p.x / viewport.x * 2.0 - 1.0, 1.0 - p.y / viewport.y * 2.0, 0.0, 1.0);
    }

    vertex VOut bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                          const device Instance* inst [[buffer(0)]],
                          constant Uniforms& u [[buffer(1)]]) {
        Instance i = inst[iid];
        float2 c = corners[vid];
        VOut o;
        o.position = to_clip(i.origin + c * i.size, u.viewport);
        o.uv = c;
        o.color = i.bg;
        o.flags = i.flags;
        return o;
    }

    fragment float4 bg_fragment(VOut in [[stage_in]]) {
        return in.color;
    }

    vertex VOut glyph_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                             const device Instance* inst [[buffer(0)]],
                             constant Uniforms& u [[buffer(1)]]) {
        Instance i = inst[iid];
        float2 c = corners[vid];
        float2 size = (i.flags & 1u) ? i.size : float2(0.0);
        VOut o;
        o.position = to_clip(i.origin + c * size, u.viewport);
        o.uv = mix(i.uv.xy, i.uv.zw, c);
        o.color = i.fg;
        o.flags = i.flags;
        return o;
    }

    fragment float4 glyph_fragment(VOut in [[stage_in]],
                                   texture2d<float> atlas [[texture(0)]],
                                   sampler s [[sampler(0)]]) {
        float4 t = atlas.sample(s, in.uv);
        if (in.flags & 2u) {
            return t * in.color.a;
        }
        return float4(in.color.rgb * t.a, t.a) * in.color.a;
    }
    """
}
