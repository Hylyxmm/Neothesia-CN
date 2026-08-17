struct ViewUniform {
    transform: mat4x4<f32>,
    size: vec2<f32>,
    scale: f32,
}

@group(0) @binding(0)
var<uniform> view_uniform: ViewUniform;

struct Vertex {
    @location(0) position: vec2<f32>,
}

struct LightInstance {
    @location(1) q_position: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>,
    // x: fade_h  — vertical fade height (logical px) fading upward from the quad's BOTTOM edge;
    //              0 disables the fade (plain rectangle with 1px edge AA instead).
    // y: mode    — 0 = rectangle, 1 = soft radial disc (sparkle particles).
    // z: edge    — horizontal feather fraction (0..1) of the width, feathering both side edges.
    // w: unused
    @location(4) params: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,

    @location(0) local_px: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) params: vec4<f32>,
}

@vertex
fn vs_main(vertex: Vertex, light: LightInstance) -> VertexOutput {
    var quad_position = light.q_position * view_uniform.scale;
    var quad_size = light.size * view_uniform.scale;

    var i_transform: mat4x4<f32> = mat4x4<f32>(
        vec4<f32>(quad_size.x, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, quad_size.y, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(quad_position, 0.0, 1.0),
    );

    var out: VertexOutput;
    out.position = view_uniform.transform * i_transform * vec4<f32>(vertex.position, 0.0, 1.0);
    // Local coordinates in logical px within the quad, for per-pixel fades in the fragment stage.
    out.local_px = vec2<f32>(
        vertex.position.x * light.size.x,
        vertex.position.y * light.size.y,
    );
    out.size = light.size;
    out.color = light.color;
    out.params = light.params;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let fade_h = in.params.x;
    let mode = in.params.y;
    let edge = in.params.z;

    var alpha = 1.0;

    if (mode > 0.5) {
        // Soft radial disc: fully opaque in the inner 20%, smooth falloff to the rim.
        let r = min(in.size.x, in.size.y) * 0.5;
        let d = distance(in.local_px, in.size * 0.5);
        alpha = 1.0 - smoothstep(r * 0.2, r, d);
    } else {
        // Horizontal feather on both side edges.
        let edge_px = in.size.x * edge;
        if (edge_px > 0.0) {
            let dx = min(in.local_px.x, in.size.x - in.local_px.x);
            alpha *= smoothstep(0.0, edge_px, dx);
        }

        if (fade_h > 0.0) {
            // Per-pixel vertical fade: brightest at the bottom edge, smoothly reaching zero
            // `fade_h` px above it — no visible banding, unlike stacked translucent quads.
            let from_bottom = in.size.y - in.local_px.y;
            alpha *= 1.0 - smoothstep(0.0, fade_h, from_bottom);
        } else {
            // Plain rectangle: 1px anti-aliased top/bottom edges.
            let dy = min(in.local_px.y, in.size.y - in.local_px.y);
            alpha *= smoothstep(0.0, 1.0, dy);
        }
    }

    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
