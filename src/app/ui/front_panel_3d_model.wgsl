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
fn fs_main(input: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    var n = normalize(input.world_normal);
    if !front_facing {
        n = -n;
    }

    let key = max(dot(n, normalize(vec3<f32>(0.42, 0.68, 0.60))), 0.0);
    let fill = max(dot(n, normalize(vec3<f32>(-0.70, 0.28, 0.58))), 0.0);
    let back = max(dot(n, normalize(vec3<f32>(0.14, 0.46, -0.88))), 0.0);
    let hemisphere = 0.5 + 0.5 * n.y;

    // Match the neutral Blender material-preview balance more closely. The GLB
    // baseColorFactor is already in linear space; keep it intact and illuminate
    // it rather than applying an artistic gamma lift or a hard clamp.
    let base = clamp(input.color.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let diffuse_light = 1.10 + 0.42 * hemisphere + 0.52 * key + 0.20 * fill + 0.10 * back;

    // Blender's neutral preview keeps even very dark painted surfaces readable
    // because they reflect the studio environment. Approximate that environment
    // with two broad neutral lobes so black paint, metal toggles and the blue
    // enclosure retain shape without tinting their authored base colours.
    let studio_a = pow(
        max(dot(n, normalize(vec3<f32>(0.15, 0.30, 0.94))), 0.0),
        6.0,
    );
    let studio_b = pow(
        max(dot(n, normalize(vec3<f32>(-0.48, 0.22, 0.85))), 0.0),
        4.0,
    );
    let studio_reflection = vec3<f32>(0.034) * studio_a + vec3<f32>(0.018) * studio_b;

    var rgb = base * diffuse_light + studio_reflection;

    // Lamps remain emissive presentation driven by the same electrical duty
    // snapshot as the classic 2D panel. They are not point lights and feed no
    // state back into the emulated machine.
    if input.led_index < 36u {
        let intensity = led_state.values[input.led_index].x;
        rgb += vec3<f32>(2.4, 0.040, 0.015) * intensity;
    }

    return vec4<f32>(rgb, input.color.a);
}
