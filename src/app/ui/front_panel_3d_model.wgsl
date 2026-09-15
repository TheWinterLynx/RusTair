struct Camera {
    view_proj: mat4x4<f32>,
};

struct LedState {
    values: array<vec4<f32>, 36>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(0) @binding(1)
var<uniform> led_state: LedState;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) led_index: u32,
};

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) led_index: u32,
};

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip_position = camera.view_proj * vec4<f32>(input.position, 1.0);
    out.world_normal = input.normal;
    out.color = input.color;
    out.led_index = input.led_index;
    return out;
}

@fragment
fn fs_main(input: VertexOut) -> @location(0) vec4<f32> {
    let n = normalize(input.world_normal);
    let key = max(dot(n, normalize(vec3<f32>(0.42, 0.68, 0.60))), 0.0);
    let fill = max(dot(n, normalize(vec3<f32>(-0.70, 0.28, 0.58))), 0.0);
    let back = max(dot(n, normalize(vec3<f32>(0.14, 0.46, -0.88))), 0.0);
    let hemisphere = 0.5 + 0.5 * n.y;

    // Preserve the GLB baseColorFactor instead of applying an artistic gamma
    // lift. This keeps the period blue, charcoal panel and grey/white plastics
    // close to the neutral material-preview appearance of the same GLB in
    // Blender while still giving enough shape to the enclosure and internals.
    let base = clamp(input.color.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let light = 0.72 + 0.30 * hemisphere + 0.38 * key + 0.14 * fill + 0.08 * back;
    var linear_rgb = base * light;

    // Lamps remain emissive presentation driven by the same electrical duty
    // snapshot as the classic 2D panel. Emission is added before the gentle
    // highlight roll-off so fully lit LEDs glow without clipping the lens to a
    // flat primary colour.
    if input.led_index < 36u {
        let intensity = led_state.values[input.led_index].x;
        linear_rgb += vec3<f32>(2.8, 0.045, 0.018) * intensity;
    }

    // Soft highlight compression only; unlike the previous hard min(..., 1.0)
    // this keeps the lower logo strip and pale internal parts from blowing out.
    let rgb = linear_rgb / (vec3<f32>(1.0) + 0.35 * linear_rgb);
    return vec4<f32>(rgb, input.color.a);
}
