struct Camera {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
};
struct LedState { values: array<vec4<f32>, 36>, };
struct SwitchTransform {
    pivot_angle: vec4<f32>,
    axis_radius: vec4<f32>,
};
struct SwitchState { values: array<SwitchTransform, 25>, };
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> led_state: LedState;
@group(0) @binding(2) var<uniform> switch_state: SwitchState;

const SWITCH_SEAT_DEPTH_M: f32 = 0.0032;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) led_index: u32,
    @location(4) metallic_roughness: vec2<f32>,
    @location(5) switch_index: u32,
};
struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) led_index: u32,
    @location(3) world_position: vec3<f32>,
    @location(4) metallic_roughness: vec2<f32>,
};

fn rotate_axis(value: vec3<f32>, axis: vec3<f32>, angle: f32) -> vec3<f32> {
    let sine = sin(angle);
    let cosine = cos(angle);
    return value * cosine
        + cross(axis, value) * sine
        + axis * dot(axis, value) * (1.0 - cosine);
}

@vertex
fn vs_main(input: VertexIn) -> VertexOut {
    var position = input.position;
    var normal = input.normal;
    if input.switch_index < 25u {
        let lever_state = switch_state.values[input.switch_index];
        let pivot = lever_state.pivot_angle.xyz;
        let axis = normalize(lever_state.axis_radius.xyz);
        let angle = lever_state.pivot_angle.w;
        let seat_offset = vec3<f32>(0.0, 0.0, -SWITCH_SEAT_DEPTH_M);
        position = pivot + rotate_axis(position - pivot, axis, angle) + seat_offset;
        normal = normalize(rotate_axis(normal, axis, angle));
    }

    var out: VertexOut;
    out.clip_position = camera.view_proj * vec4<f32>(position, 1.0);
    out.world_normal = normal;
    out.world_position = position;
    out.color = input.color;
    out.led_index = input.led_index;
    out.metallic_roughness = input.metallic_roughness;
    return out;
}
fn fresnel(f0: vec3<f32>, cosine: f32) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - cosine, 5.0);
}
// Metallic/roughness GGX BRDF. Metal reflects the environment instead of being
// painted with baseColor * diffuse light (which blew the nickel parts white).
fn light_brdf(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, base: vec3<f32>,
              metal: f32, rough: f32, energy: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let nl = max(dot(n,l), 0.0);
    let nv = max(dot(n,v), 0.0001);
    let nh = max(dot(n,h), 0.0);
    let vh = max(dot(v,h), 0.0);
    let a2 = pow(rough, 4.0);
    let d = a2 / max(3.14159265 * pow(nh * nh * (a2 - 1.0) + 1.0, 2.0), 0.000001);
    let k = pow(rough + 1.0, 2.0) / 8.0;
    let g = (nl / (nl * (1.0 - k) + k)) * (nv / (nv * (1.0 - k) + k));
    let f = fresnel(mix(vec3<f32>(0.04), base, metal), vh);
    let specular = d * g * f / max(4.0 * nl * nv, 0.0001);
    let diffuse = (vec3<f32>(1.0) - f) * (1.0 - metal) * base / 3.14159265;
    return (diffuse + specular) * nl * energy;
}
// Neutral procedural studio environment. Broad reflections remain visible on
// nickel, aluminium and dark paint without an external HDRI or baked lighting.
fn studio(reflected: vec3<f32>, rough: f32) -> vec3<f32> {
    let exponent = mix(100.0, 2.0, rough * rough);
    let key = pow(max(dot(reflected, normalize(vec3<f32>(0.45,0.65,0.60))), 0.0), exponent);
    let fill = pow(max(dot(reflected, normalize(vec3<f32>(-0.65,0.30,0.70))), 0.0), exponent);
    let room = 0.18 + 0.22 * (reflected.y * 0.5 + 0.5);
    return vec3<f32>(room + 1.6 * key + 0.9 * fill);
}
@fragment
fn fs_main(input: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    var n = normalize(input.world_normal);
    if !front_facing { n = -n; }
    let v = normalize(camera.eye.xyz - input.world_position);
    let base = max(input.color.rgb, vec3<f32>(0.0));
    let metal = clamp(input.metallic_roughness.x, 0.0, 1.0);
    let rough = clamp(input.metallic_roughness.y, 0.045, 1.0);
    let f0 = mix(vec3<f32>(0.04), base, metal);
    let nv = max(dot(n,v), 0.0);
    let f = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - nv, 5.0);
    let ambient = (vec3<f32>(1.0) - f) * (1.0 - metal) * base * (0.65 + 0.25 * (n.y * 0.5 + 0.5));
    var rgb = ambient + studio(reflect(-v,n), rough) * f;
    rgb += light_brdf(n,v,normalize(vec3<f32>(0.42,0.68,0.60)),base,metal,rough,2.0);
    rgb += light_brdf(n,v,normalize(vec3<f32>(-0.70,0.28,0.58)),base,metal,rough,0.8);
    if input.led_index < 36u {
        rgb += vec3<f32>(2.4,0.040,0.015) * led_state.values[input.led_index].x;
    }
    // Compress highlights in linear light before the sRGB render attachment.
    // This preview is not Blender's AgX/path-traced studio, but uses the GLB's
    // authored material factors and retains highlight detail instead of clipping.
    rgb = rgb / (vec3<f32>(1.0) + rgb);
    return vec4<f32>(rgb, input.color.a);
}
