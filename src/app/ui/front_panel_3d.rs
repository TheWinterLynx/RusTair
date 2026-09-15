use std::collections::HashMap;
use std::num::NonZeroU64;

use eframe::{
    egui,
    egui_wgpu::wgpu::util::DeviceExt as _,
    egui_wgpu::{self, wgpu},
};

use super::super::RusTairApp;
use crate::embedded_assets;

const GLB_PATH: &str = "assets/panels/altair-3d/runtime/Altair8800_1975.glb";
const BINDINGS_PATH: &str = "assets/panels/altair-3d/runtime/bindings.json";
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
const VERTEX_STRIDE: wgpu::BufferAddress = 44;
const CAMERA_UNIFORM_BYTES: u64 = 64;
const LED_COUNT: usize = 36;
const LED_UNIFORM_BYTES: u64 = LED_COUNT as u64 * 16;
const NO_LED_INDEX: u32 = u32::MAX;
const LED_VISIBLE_THRESHOLD: f32 = 0.025;
const LED_CORE_STEEPNESS: f32 = 7.0;

const LED_IDS: [&str; LED_COUNT] = [
    "INTE", "PROT", "MEMR", "INP", "M1", "OUT", "HLTA", "STACK", "WO", "INT", "WAIT", "HLDA",
    "A15", "A14", "A13", "A12", "A11", "A10", "A09", "A08", "A07", "A06", "A05", "A04", "A03",
    "A02", "A01", "A00", "D7", "D6", "D5", "D4", "D3", "D2", "D1", "D0",
];

const MODEL_SHADER: &str = r#"
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
    let key = max(dot(n, normalize(vec3<f32>(0.38, 0.72, 0.58))), 0.0);
    let fill = max(dot(n, normalize(vec3<f32>(-0.72, 0.22, 0.48))), 0.0);
    let back = max(dot(n, normalize(vec3<f32>(0.18, -0.32, -0.93))), 0.0);
    let sky = 0.5 + 0.5 * n.y;

    // The GLB deliberately carries very dark period-correct paint values. A
    // pure multiplicative Lambert term made those values collapse to almost
    // black in the emulator. Lift the viewing exposure while retaining a dark
    // charcoal front panel and the blue enclosure separation.
    let base = pow(max(input.color.rgb, vec3<f32>(0.018)), vec3<f32>(0.72));
    let light = 1.08 + 0.68 * key + 0.38 * fill + 0.32 * back + 0.12 * sky;
    var rgb = min(base * light + vec3<f32>(0.028), vec3<f32>(1.0));

    // Lamps are emissive presentation driven from the exact same electrical
    // duty snapshot as the classic 2D panel. They are not point lights and do
    // not feed any state back into the emulated machine.
    if input.led_index < 36u {
        let intensity = led_state.values[input.led_index].x;
        let emission = vec3<f32>(1.0, 0.035, 0.012) * (2.6 * intensity);
        rgb = min(rgb + emission, vec3<f32>(1.0));
    }

    return vec4<f32>(rgb, input.color.a);
}
"#;

const PRESENT_SHADER: &str = r#"
@group(0) @binding(0)
var source_tex: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

struct QuadOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> QuadOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );

    var out: QuadOut;
    out.clip_position = vec4<f32>(positions[vertex_index], 0.0, 1.0);
    out.uv = uvs[vertex_index];
    return out;
}

@fragment
fn fs_main(input: QuadOut) -> @location(0) vec4<f32> {
    return textureSample(source_tex, source_sampler, input.uv);
}
"#;

#[derive(Clone, Copy, Debug)]
struct CameraState {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    pan: [f32; 3],
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.10,
            zoom: 1.0,
            pan: [0.0; 3],
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Panel3dUiState {
    open: bool,
    camera: CameraState,
}

impl Default for Panel3dUiState {
    fn default() -> Self {
        Self {
            open: false,
            camera: CameraState::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Panel3dSnapshot {
    leds: [f32; LED_COUNT],
}

fn ui_state_id() -> egui::Id {
    egui::Id::new("rustair-altair-3d-front-panel-state")
}

fn load_ui_state(ctx: &egui::Context) -> Panel3dUiState {
    ctx.data(|data| {
        data.get_temp::<Panel3dUiState>(ui_state_id())
            .unwrap_or_default()
    })
}

fn store_ui_state(ctx: &egui::Context, state: Panel3dUiState) {
    ctx.data_mut(|data| data.insert_temp(ui_state_id(), state));
}

fn led_optical_intensity(electrical: f32, powered: bool, brightness: f32) -> f32 {
    if !powered {
        return 0.0;
    }
    let electrical = electrical.clamp(0.0, 1.0);
    if electrical < LED_VISIBLE_THRESHOLD {
        return 0.0;
    }
    let x = ((electrical - LED_VISIBLE_THRESHOLD) / (1.0 - LED_VISIBLE_THRESHOLD)).clamp(0.0, 1.0);
    let normalization = 1.0 - (-LED_CORE_STEEPNESS).exp();
    let response = (1.0 - (-LED_CORE_STEEPNESS * x).exp()) / normalization;
    (response * brightness).clamp(0.0, 1.0)
}

fn panel_snapshot(app: &RusTairApp) -> Panel3dSnapshot {
    let panel = app.machine.front_panel_state();
    let lamps = panel.lamps;
    let (brightness, _) = super::persistence::led_visual_settings();
    let mut leds = [0.0; LED_COUNT];
    let mut set_led = |index: usize, electrical: f32| {
        leds[index] = led_optical_intensity(electrical, panel.powered, brightness);
    };

    set_led(0, lamps.inte);
    set_led(1, lamps.prot);
    set_led(2, lamps.memr);
    set_led(3, lamps.inp);
    set_led(4, lamps.m1);
    set_led(5, lamps.out);
    set_led(6, lamps.hlta);
    set_led(7, lamps.stack);
    set_led(8, lamps.wo);
    set_led(9, lamps.int_ack);
    set_led(10, lamps.wait);
    set_led(11, lamps.hlda);

    for binding_bit in 0..16 {
        set_led(12 + binding_bit, lamps.address[15 - binding_bit]);
    }
    for binding_bit in 0..8 {
        set_led(28 + binding_bit, lamps.data[7 - binding_bit]);
    }

    Panel3dSnapshot { leds }
}

pub(super) fn open(ctx: &egui::Context) {
    let mut state = load_ui_state(ctx);
    state.open = true;
    store_ui_state(ctx, state);
    ctx.request_repaint();
}

pub(super) fn install(cc: &eframe::CreationContext<'_>) {
    let Some(render_state) = cc.wgpu_render_state.as_ref() else {
        return;
    };

    render_state
        .renderer
        .write()
        .callback_resources
        .insert(Altair3dRenderResources::new(render_state.target_format));
}

pub(super) fn show_viewport(app: &RusTairApp, parent_ctx: &egui::Context) {
    let mut state = load_ui_state(parent_ctx);
    if !state.open {
        return;
    }

    let snapshot = panel_snapshot(app);
    let mut close_requested = false;
    parent_ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("rustair-altair-3d-front-panel"),
        egui::ViewportBuilder::default()
            .with_title("RusTair — Interactive 3D Altair 8800")
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([720.0, 480.0])
            .with_resizable(true),
        |viewport_ctx, _class| {
            egui::CentralPanel::default().show(viewport_ctx, |ui| {
                draw_viewport_contents(ui, &mut state, snapshot);
            });
            close_requested = viewport_ctx.input(|input| input.viewport().close_requested());
        },
    );

    if close_requested {
        state.open = false;
    }
    store_ui_state(parent_ctx, state);
}

fn draw_viewport_contents(
    ui: &mut egui::Ui,
    state: &mut Panel3dUiState,
    snapshot: Panel3dSnapshot,
) {
    ui.horizontal(|ui| {
        ui.label("LMB orbit · RMB/MMB pan · wheel dolly");
        ui.separator();
        ui.add(
            egui::Slider::new(&mut state.camera.zoom, 0.08..=6.0)
                .text("Distance")
                .step_by(0.01),
        );
        if ui.button("Reset camera").clicked() {
            state.camera = CameraState::default();
        }
    });
    ui.small(
        "Native inspection viewport · live front-panel LEDs share the classic panel hardware duty snapshot; switches are the next connection checkpoint.",
    );
    ui.separator();

    let available = ui.available_size();
    let canvas_size = egui::vec2(available.x.max(320.0), available.y.max(240.0));
    let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::drag());

    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = ui.input(|input| input.pointer.delta());
        state.camera.yaw = wrap_angle(state.camera.yaw - delta.x * 0.008);
        state.camera.pitch = wrap_angle(state.camera.pitch + delta.y * 0.008);
        ui.ctx().request_repaint();
    }

    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let delta = ui.input(|input| input.pointer.delta());
        let (_, right, up) = camera_basis(state.camera);
        let scale = 0.003 * state.camera.zoom;
        state.camera.pan = add3(
            state.camera.pan,
            add3(scale3(right, -delta.x * scale), scale3(up, delta.y * scale)),
        );
        ui.ctx().request_repaint();
    }

    if response.hovered() {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll.abs() > f32::EPSILON {
            state.camera.zoom = (state.camera.zoom * (-scroll * 0.0025).exp()).clamp(0.08, 6.0);
            ui.ctx().request_repaint();
        }
    }

    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_rgb(36, 39, 45));

    ui.painter().add(egui_wgpu::Callback::new_paint_callback(
        rect,
        Altair3dCallback {
            camera: state.camera,
            leds: snapshot.leds,
            size_points: [rect.width(), rect.height()],
        },
    ));
}

#[derive(Clone, Copy)]
struct Altair3dCallback {
    camera: CameraState,
    leds: [f32; LED_COUNT],
    size_points: [f32; 2],
}

impl egui_wgpu::CallbackTrait for Altair3dCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(resources) = callback_resources.get_mut::<Altair3dRenderResources>() else {
            return Vec::new();
        };

        let width = (self.size_points[0] * screen_descriptor.pixels_per_point)
            .round()
            .max(1.0) as u32;
        let height = (self.size_points[1] * screen_descriptor.pixels_per_point)
            .round()
            .max(1.0) as u32;

        resources.prepare(
            device,
            queue,
            encoder,
            [width, height],
            self.camera,
            self.leds,
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(resources) = callback_resources.get::<Altair3dRenderResources>() else {
            return;
        };
        resources.paint(render_pass);
    }
}

struct Altair3dRenderResources {
    target_format: wgpu::TextureFormat,
    loaded: Option<Result<LoadedRenderer, String>>,
}

impl Altair3dRenderResources {
    fn new(target_format: wgpu::TextureFormat) -> Self {
        Self {
            target_format,
            loaded: None,
        }
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        camera: CameraState,
        leds: [f32; LED_COUNT],
    ) {
        if self.loaded.is_none() {
            self.loaded = Some(LoadedRenderer::new(device, self.target_format));
        }

        if let Some(Ok(renderer)) = self.loaded.as_mut() {
            renderer.prepare(device, queue, encoder, size, camera, leds);
        }
    }

    fn paint(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        if let Some(Ok(renderer)) = self.loaded.as_ref() {
            renderer.paint(render_pass);
        }
    }
}

struct LoadedRenderer {
    model_pipeline: wgpu::RenderPipeline,
    present_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    led_buffer: wgpu::Buffer,
    model_bind_group: wgpu::BindGroup,
    present_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    center: [f32; 3],
    radius: f32,
    target: Option<OffscreenTarget>,
}

impl LoadedRenderer {
    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Result<Self, String> {
        let mesh = load_static_mesh()?;
        let vertex_bytes = encode_vertices(&mesh.vertices);
        let index_bytes = encode_indices(&mesh.indices);

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Altair 8800 3D vertex buffer"),
            contents: &vertex_bytes,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Altair 8800 3D index buffer"),
            contents: &index_bytes,
            usage: wgpu::BufferUsages::INDEX,
        });

        let model_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Altair 8800 3D model state layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(CAMERA_UNIFORM_BYTES),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(LED_UNIFORM_BYTES),
                        },
                        count: None,
                    },
                ],
            });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Altair 8800 3D camera buffer"),
            size: CAMERA_UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let led_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Altair 8800 3D LED buffer"),
            size: LED_UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Altair 8800 3D model state bind group"),
            layout: &model_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: led_buffer.as_entire_binding(),
                },
            ],
        });

        let model_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Altair 8800 3D model shader"),
            source: wgpu::ShaderSource::Wgsl(MODEL_SHADER.into()),
        });
        let model_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Altair 8800 3D model pipeline layout"),
                bind_group_layouts: &[&model_bind_group_layout],
                push_constant_ranges: &[],
            });

        const ATTRIBUTES: [wgpu::VertexAttribute; 4] = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 40,
                shader_location: 3,
            },
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: VERTEX_STRIDE,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        };

        let model_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Altair 8800 3D model pipeline"),
            layout: Some(&model_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &model_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[vertex_layout],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &model_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OFFSCREEN_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let present_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Altair 8800 3D present layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });
        let present_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Altair 8800 3D present pipeline layout"),
                bind_group_layouts: &[&present_bind_group_layout],
                push_constant_ranges: &[],
            });
        let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Altair 8800 3D present shader"),
            source: wgpu::ShaderSource::Wgsl(PRESENT_SHADER.into()),
        });
        let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Altair 8800 3D present pipeline"),
            layout: Some(&present_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &present_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &present_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Altair 8800 3D present sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(Self {
            model_pipeline,
            present_pipeline,
            camera_buffer,
            led_buffer,
            model_bind_group,
            present_bind_group_layout,
            sampler,
            vertex_buffer,
            index_buffer,
            index_count: u32::try_from(mesh.indices.len())
                .map_err(|_| "Altair 3D index count exceeds u32".to_owned())?,
            center: mesh.center,
            radius: mesh.radius,
            target: None,
        })
    }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: [u32; 2],
        camera: CameraState,
        leds: [f32; LED_COUNT],
    ) {
        let resize = self
            .target
            .as_ref()
            .is_none_or(|target| target.size != size);
        if resize {
            self.target = Some(OffscreenTarget::new(
                device,
                &self.present_bind_group_layout,
                &self.sampler,
                size,
            ));
        }

        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let view_proj = camera_matrix(self.center, self.radius, aspect, camera);
        queue.write_buffer(&self.camera_buffer, 0, &encode_mat4_uniform(view_proj));
        queue.write_buffer(&self.led_buffer, 0, &encode_led_uniform(leds));

        let Some(target) = self.target.as_ref() else {
            return;
        };

        let color_attachment = Some(wgpu::RenderPassColorAttachment {
            view: &target.color_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.105,
                    g: 0.115,
                    b: 0.135,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        });
        let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
            view: &target.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        };

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Altair 8800 3D offscreen pass"),
            color_attachments: &[color_attachment],
            depth_stencil_attachment: Some(depth_attachment),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        render_pass.set_pipeline(&self.model_pipeline);
        render_pass.set_bind_group(0, &self.model_bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..self.index_count, 0, 0..1);
    }

    fn paint(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        let Some(target) = self.target.as_ref() else {
            return;
        };
        render_pass.set_pipeline(&self.present_pipeline);
        render_pass.set_bind_group(0, &target.present_bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

struct OffscreenTarget {
    size: [u32; 2],
    _color: wgpu::Texture,
    _depth: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    present_bind_group: wgpu::BindGroup,
}

impl OffscreenTarget {
    fn new(
        device: &wgpu::Device,
        present_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        size: [u32; 2],
    ) -> Self {
        let extent = wgpu::Extent3d {
            width: size[0].max(1),
            height: size[1].max(1),
            depth_or_array_layers: 1,
        };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Altair 8800 3D offscreen color"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OFFSCREEN_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Altair 8800 3D offscreen depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        let present_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Altair 8800 3D present bind group"),
            layout: present_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });

        Self {
            size,
            _color: color,
            _depth: depth,
            color_view,
            depth_view,
            present_bind_group,
        }
    }
}

#[derive(Clone, Copy)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
    led_index: u32,
}

struct StaticMesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    center: [f32; 3],
    radius: f32,
}

fn load_led_material_bindings() -> Result<HashMap<String, u32>, String> {
    let bytes = embedded_assets::get(BINDINGS_PATH)
        .ok_or_else(|| format!("missing embedded runtime asset: {BINDINGS_PATH}"))?;
    let mut parser = JsonParser::new(bytes);
    let root = parser.parse()?;
    let leds = json_array(root.require("leds")?)?;
    if leds.len() != LED_COUNT {
        return Err(format!(
            "Altair 3D bindings define {} LEDs; expected {LED_COUNT}",
            leds.len()
        ));
    }

    let mut materials = HashMap::with_capacity(LED_COUNT);
    for (index, led) in leds.iter().enumerate() {
        let id = json_string(led.require("id")?)?;
        if id != LED_IDS[index] {
            return Err(format!(
                "Altair 3D LED binding {index} is {id:?}; expected {:?}",
                LED_IDS[index]
            ));
        }
        let material = json_string(led.require("material")?)?.to_owned();
        if materials.insert(material.clone(), index as u32).is_some() {
            return Err(format!(
                "Altair 3D LED material {material:?} is bound more than once"
            ));
        }
    }
    Ok(materials)
}

fn load_static_mesh() -> Result<StaticMesh, String> {
    let glb = embedded_assets::get(GLB_PATH)
        .ok_or_else(|| format!("missing embedded runtime asset: {GLB_PATH}"))?;
    let (root, bin) = parse_glb(glb)?;
    let led_materials = load_led_material_bindings()?;

    let scene_index = root.get("scene").map(json_usize).transpose()?.unwrap_or(0);
    let scenes = json_array(root.require("scenes")?)?;
    let scene = scenes
        .get(scene_index)
        .ok_or_else(|| format!("glTF scene index {scene_index} is out of range"))?;
    let roots = json_array(scene.require("nodes")?)?;

    let mut builder = MeshBuilder::default();
    for node in roots {
        append_node(
            &root,
            bin,
            json_usize(node)?,
            Mat4::identity(),
            &led_materials,
            &mut builder,
        )?;
    }
    builder.finish()
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    min: [f32; 3],
    max: [f32; 3],
    have_bounds: bool,
}

impl MeshBuilder {
    fn include_point(&mut self, point: [f32; 3]) {
        if !self.have_bounds {
            self.min = point;
            self.max = point;
            self.have_bounds = true;
            return;
        }
        for axis in 0..3 {
            self.min[axis] = self.min[axis].min(point[axis]);
            self.max[axis] = self.max[axis].max(point[axis]);
        }
    }

    fn finish(self) -> Result<StaticMesh, String> {
        if !self.have_bounds || self.vertices.is_empty() || self.indices.is_empty() {
            return Err("embedded Altair glTF contains no renderable triangle geometry".into());
        }

        let center = [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ];
        let half = [
            (self.max[0] - self.min[0]) * 0.5,
            (self.max[1] - self.min[1]) * 0.5,
            (self.max[2] - self.min[2]) * 0.5,
        ];
        let radius = length3(half).max(0.01);

        Ok(StaticMesh {
            vertices: self.vertices,
            indices: self.indices,
            center,
            radius,
        })
    }
}

fn append_node(
    root: &JsonValue,
    bin: &[u8],
    node_index: usize,
    parent_transform: Mat4,
    led_materials: &HashMap<String, u32>,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    let nodes = json_array(root.require("nodes")?)?;
    let node = nodes
        .get(node_index)
        .ok_or_else(|| format!("glTF node index {node_index} is out of range"))?;
    let world = parent_transform.mul(node_transform(node)?);

    if let Some(mesh_value) = node.get("mesh") {
        append_mesh(
            root,
            bin,
            json_usize(mesh_value)?,
            world,
            led_materials,
            builder,
        )?;
    }

    if let Some(children) = node.get("children") {
        for child in json_array(children)? {
            append_node(root, bin, json_usize(child)?, world, led_materials, builder)?;
        }
    }
    Ok(())
}

fn append_mesh(
    root: &JsonValue,
    bin: &[u8],
    mesh_index: usize,
    transform: Mat4,
    led_materials: &HashMap<String, u32>,
    builder: &mut MeshBuilder,
) -> Result<(), String> {
    let meshes = json_array(root.require("meshes")?)?;
    let mesh = meshes
        .get(mesh_index)
        .ok_or_else(|| format!("glTF mesh index {mesh_index} is out of range"))?;
    let primitives = json_array(mesh.require("primitives")?)?;

    for primitive in primitives {
        let mode = primitive
            .get("mode")
            .map(json_usize)
            .transpose()?
            .unwrap_or(4);
        if mode != 4 {
            return Err(format!(
                "Altair runtime glTF contains unsupported primitive mode {mode}"
            ));
        }

        let attributes = primitive.require("attributes")?;
        let position_accessor = json_usize(attributes.require("POSITION")?)?;
        let normal_accessor = json_usize(attributes.require("NORMAL")?)?;
        let positions = read_vec3_f32_accessor(root, bin, position_accessor)?;
        let normals = read_vec3_f32_accessor(root, bin, normal_accessor)?;
        if positions.len() != normals.len() {
            return Err(format!(
                "glTF primitive has {} positions but {} normals",
                positions.len(),
                normals.len()
            ));
        }

        let material_index = primitive.get("material").map(json_usize).transpose()?;
        let color = material_index
            .map(|material| material_base_color(root, material))
            .transpose()?
            .unwrap_or([1.0; 4]);
        let led_index = material_index
            .map(|material| material_name(root, material))
            .transpose()?
            .flatten()
            .and_then(|name| led_materials.get(name).copied())
            .unwrap_or(NO_LED_INDEX);

        let base = u32::try_from(builder.vertices.len())
            .map_err(|_| "Altair 3D vertex count exceeds u32".to_owned())?;
        for (position, normal) in positions.into_iter().zip(normals) {
            let world_position = transform.transform_point(position);
            let world_normal = normalize3(transform.transform_vector(normal));
            builder.include_point(world_position);
            builder.vertices.push(Vertex {
                position: world_position,
                normal: world_normal,
                color,
                led_index,
            });
        }

        let index_accessor = json_usize(primitive.require("indices")?)?;
        for index in read_indices_accessor(root, bin, index_accessor)? {
            builder.indices.push(
                base.checked_add(index)
                    .ok_or_else(|| "Altair 3D index overflow".to_owned())?,
            );
        }
    }

    Ok(())
}

fn material(root: &JsonValue, material_index: usize) -> Result<&JsonValue, String> {
    let materials = json_array(root.require("materials")?)?;
    materials
        .get(material_index)
        .ok_or_else(|| format!("glTF material index {material_index} is out of range"))
}

fn material_name(root: &JsonValue, material_index: usize) -> Result<Option<&str>, String> {
    material(root, material_index)?
        .get("name")
        .map(json_string)
        .transpose()
}

fn material_base_color(root: &JsonValue, material_index: usize) -> Result<[f32; 4], String> {
    let material = material(root, material_index)?;
    let Some(pbr) = material.get("pbrMetallicRoughness") else {
        return Ok([1.0; 4]);
    };
    let Some(factor) = pbr.get("baseColorFactor") else {
        return Ok([1.0; 4]);
    };
    let values = json_array(factor)?;
    if values.len() != 4 {
        return Err("glTF baseColorFactor is not a vec4".into());
    }
    Ok([
        json_f32(&values[0])?,
        json_f32(&values[1])?,
        json_f32(&values[2])?,
        json_f32(&values[3])?,
    ])
}

fn node_transform(node: &JsonValue) -> Result<Mat4, String> {
    if let Some(matrix) = node.get("matrix") {
        let values = json_array(matrix)?;
        if values.len() != 16 {
            return Err("glTF node matrix does not contain 16 values".into());
        }
        let mut result = Mat4::identity();
        for column in 0..4 {
            for row in 0..4 {
                result.0[row][column] = json_f32(&values[column * 4 + row])?;
            }
        }
        return Ok(result);
    }

    let translation = read_optional_vec3(node.get("translation"), [0.0, 0.0, 0.0])?;
    let rotation = read_optional_vec4(node.get("rotation"), [0.0, 0.0, 0.0, 1.0])?;
    let scale = read_optional_vec3(node.get("scale"), [1.0, 1.0, 1.0])?;

    Ok(Mat4::translation(translation)
        .mul(Mat4::rotation_quaternion(rotation))
        .mul(Mat4::scale(scale)))
}

fn read_optional_vec3(value: Option<&JsonValue>, default: [f32; 3]) -> Result<[f32; 3], String> {
    let Some(value) = value else {
        return Ok(default);
    };
    let values = json_array(value)?;
    if values.len() != 3 {
        return Err("glTF vec3 has wrong length".into());
    }
    Ok([
        json_f32(&values[0])?,
        json_f32(&values[1])?,
        json_f32(&values[2])?,
    ])
}

fn read_optional_vec4(value: Option<&JsonValue>, default: [f32; 4]) -> Result<[f32; 4], String> {
    let Some(value) = value else {
        return Ok(default);
    };
    let values = json_array(value)?;
    if values.len() != 4 {
        return Err("glTF vec4 has wrong length".into());
    }
    Ok([
        json_f32(&values[0])?,
        json_f32(&values[1])?,
        json_f32(&values[2])?,
        json_f32(&values[3])?,
    ])
}

struct AccessorInfo {
    start: usize,
    stride: usize,
    count: usize,
    component_type: usize,
    value_type: String,
}

fn accessor_info(
    root: &JsonValue,
    bin_len: usize,
    accessor_index: usize,
) -> Result<AccessorInfo, String> {
    let accessors = json_array(root.require("accessors")?)?;
    let accessor = accessors
        .get(accessor_index)
        .ok_or_else(|| format!("glTF accessor index {accessor_index} is out of range"))?;
    let view_index = json_usize(accessor.require("bufferView")?)?;

    let views = json_array(root.require("bufferViews")?)?;
    let view = views
        .get(view_index)
        .ok_or_else(|| format!("glTF bufferView index {view_index} is out of range"))?;

    let view_offset = view
        .get("byteOffset")
        .map(json_usize)
        .transpose()?
        .unwrap_or(0);
    let accessor_offset = accessor
        .get("byteOffset")
        .map(json_usize)
        .transpose()?
        .unwrap_or(0);
    let start = view_offset
        .checked_add(accessor_offset)
        .ok_or_else(|| "glTF accessor byte offset overflow".to_owned())?;

    let component_type = json_usize(accessor.require("componentType")?)?;
    let value_type = json_string(accessor.require("type")?)?.to_owned();
    let count = json_usize(accessor.require("count")?)?;
    let element_size = component_size(component_type)?
        .checked_mul(type_components(&value_type)?)
        .ok_or_else(|| "glTF accessor element size overflow".to_owned())?;
    let stride = view
        .get("byteStride")
        .map(json_usize)
        .transpose()?
        .unwrap_or(element_size);
    if stride < element_size {
        return Err("glTF accessor stride is smaller than one element".into());
    }
    if count != 0 {
        let last = start
            .checked_add(
                stride
                    .checked_mul(count - 1)
                    .ok_or_else(|| "glTF accessor range overflow".to_owned())?,
            )
            .and_then(|value| value.checked_add(element_size))
            .ok_or_else(|| "glTF accessor range overflow".to_owned())?;
        if last > bin_len {
            return Err("glTF accessor points beyond BIN chunk".into());
        }
    }

    Ok(AccessorInfo {
        start,
        stride,
        count,
        component_type,
        value_type,
    })
}

fn read_vec3_f32_accessor(
    root: &JsonValue,
    bin: &[u8],
    accessor_index: usize,
) -> Result<Vec<[f32; 3]>, String> {
    let info = accessor_info(root, bin.len(), accessor_index)?;
    if info.component_type != 5126 || info.value_type != "VEC3" {
        return Err(format!("glTF accessor {accessor_index} is not FLOAT VEC3"));
    }

    let mut values = Vec::with_capacity(info.count);
    for item in 0..info.count {
        let offset = info.start + item * info.stride;
        values.push([
            read_f32_le(bin, offset)?,
            read_f32_le(bin, offset + 4)?,
            read_f32_le(bin, offset + 8)?,
        ]);
    }
    Ok(values)
}

fn read_indices_accessor(
    root: &JsonValue,
    bin: &[u8],
    accessor_index: usize,
) -> Result<Vec<u32>, String> {
    let info = accessor_info(root, bin.len(), accessor_index)?;
    if info.value_type != "SCALAR" {
        return Err(format!(
            "glTF index accessor {accessor_index} is not SCALAR"
        ));
    }

    let mut values = Vec::with_capacity(info.count);
    for item in 0..info.count {
        let offset = info.start + item * info.stride;
        let value = match info.component_type {
            5121 => *bin
                .get(offset)
                .ok_or_else(|| "glTF u8 index is truncated".to_owned())? as u32,
            5123 => u32::from(read_u16_le(bin, offset)?),
            5125 => read_u32_le(bin, offset)?,
            component => {
                return Err(format!("unsupported glTF index component type {component}"));
            }
        };
        values.push(value);
    }
    Ok(values)
}

fn component_size(component_type: usize) -> Result<usize, String> {
    match component_type {
        5120 | 5121 => Ok(1),
        5122 | 5123 => Ok(2),
        5125 | 5126 => Ok(4),
        other => Err(format!("unsupported glTF component type {other}")),
    }
}

fn type_components(value_type: &str) -> Result<usize, String> {
    match value_type {
        "SCALAR" => Ok(1),
        "VEC2" => Ok(2),
        "VEC3" => Ok(3),
        "VEC4" => Ok(4),
        "MAT2" => Ok(4),
        "MAT3" => Ok(9),
        "MAT4" => Ok(16),
        other => Err(format!("unsupported glTF accessor type {other}")),
    }
}

fn parse_glb(bytes: &[u8]) -> Result<(JsonValue, &[u8]), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"glTF" {
        return Err("Altair runtime model is not a GLB container".into());
    }
    let version = read_u32_le(bytes, 4)?;
    if version != 2 {
        return Err(format!("unsupported GLB version {version}"));
    }
    let declared_length = read_u32_le(bytes, 8)? as usize;
    if declared_length > bytes.len() {
        return Err("GLB declared length exceeds embedded asset length".into());
    }

    let mut offset = 12usize;
    let mut json_chunk = None;
    let mut bin_chunk = None;
    while offset + 8 <= declared_length {
        let length = read_u32_le(bytes, offset)? as usize;
        let kind = read_u32_le(bytes, offset + 4)?;
        offset += 8;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| "GLB chunk length overflow".to_owned())?;
        if end > declared_length {
            return Err("GLB chunk exceeds declared container length".into());
        }
        match kind {
            0x4E4F_534A => json_chunk = Some(&bytes[offset..end]),
            0x004E_4942 => bin_chunk = Some(&bytes[offset..end]),
            _ => {}
        }
        offset = end;
    }

    let json_bytes = json_chunk.ok_or_else(|| "GLB is missing JSON chunk".to_owned())?;
    let bin = bin_chunk.ok_or_else(|| "GLB is missing BIN chunk".to_owned())?;
    let mut parser = JsonParser::new(json_bytes);
    let root = parser.parse()?;
    Ok((root, bin))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| "binary u16 is truncated".to_owned())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| "binary u32 is truncated".to_owned())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_f32_le(bytes: &[u8], offset: usize) -> Result<f32, String> {
    Ok(f32::from_bits(read_u32_le(bytes, offset)?))
}

fn encode_vertices(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * VERTEX_STRIDE as usize);
    for vertex in vertices {
        for value in vertex
            .position
            .into_iter()
            .chain(vertex.normal)
            .chain(vertex.color)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&vertex.led_index.to_le_bytes());
    }
    bytes
}

fn encode_indices(indices: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(indices.len() * 4);
    for index in indices {
        bytes.extend_from_slice(&index.to_le_bytes());
    }
    bytes
}

fn encode_mat4_uniform(matrix: Mat4) -> [u8; CAMERA_UNIFORM_BYTES as usize] {
    let mut bytes = [0u8; CAMERA_UNIFORM_BYTES as usize];
    let mut cursor = 0usize;
    for column in 0..4 {
        for row in 0..4 {
            let value = matrix.0[row][column].to_le_bytes();
            bytes[cursor..cursor + 4].copy_from_slice(&value);
            cursor += 4;
        }
    }
    bytes
}

fn encode_led_uniform(leds: [f32; LED_COUNT]) -> Vec<u8> {
    let mut bytes = vec![0u8; LED_UNIFORM_BYTES as usize];
    for (index, intensity) in leds.into_iter().enumerate() {
        let offset = index * 16;
        bytes[offset..offset + 4].copy_from_slice(&intensity.to_le_bytes());
    }
    bytes
}

#[derive(Clone, Copy)]
struct Mat4([[f32; 4]; 4]);

impl Mat4 {
    fn identity() -> Self {
        Self([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn translation(value: [f32; 3]) -> Self {
        let mut result = Self::identity();
        result.0[0][3] = value[0];
        result.0[1][3] = value[1];
        result.0[2][3] = value[2];
        result
    }

    fn scale(value: [f32; 3]) -> Self {
        Self([
            [value[0], 0.0, 0.0, 0.0],
            [0.0, value[1], 0.0, 0.0],
            [0.0, 0.0, value[2], 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn rotation_quaternion(value: [f32; 4]) -> Self {
        let [x, y, z, w] = value;
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        let wx = w * x;
        let wy = w * y;
        let wz = w * z;
        Self([
            [1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy), 0.0],
            [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx), 0.0],
            [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy), 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    }

    fn mul(self, rhs: Self) -> Self {
        let mut out = [[0.0f32; 4]; 4];
        for (row, out_row) in out.iter_mut().enumerate() {
            for (column, cell) in out_row.iter_mut().enumerate() {
                *cell = (0..4).map(|k| self.0[row][k] * rhs.0[k][column]).sum();
            }
        }
        Self(out)
    }

    fn transform_point(self, point: [f32; 3]) -> [f32; 3] {
        [
            self.0[0][0] * point[0]
                + self.0[0][1] * point[1]
                + self.0[0][2] * point[2]
                + self.0[0][3],
            self.0[1][0] * point[0]
                + self.0[1][1] * point[1]
                + self.0[1][2] * point[2]
                + self.0[1][3],
            self.0[2][0] * point[0]
                + self.0[2][1] * point[1]
                + self.0[2][2] * point[2]
                + self.0[2][3],
        ]
    }

    fn transform_vector(self, vector: [f32; 3]) -> [f32; 3] {
        [
            self.0[0][0] * vector[0] + self.0[0][1] * vector[1] + self.0[0][2] * vector[2],
            self.0[1][0] * vector[0] + self.0[1][1] * vector[1] + self.0[1][2] * vector[2],
            self.0[2][0] * vector[0] + self.0[2][1] * vector[1] + self.0[2][2] * vector[2],
        ]
    }
}

fn camera_matrix(center: [f32; 3], radius: f32, aspect: f32, camera: CameraState) -> Mat4 {
    let fov_y = 42.0_f32.to_radians();
    let distance = (radius / (fov_y * 0.5).sin()) * camera.zoom;
    let target = add3(center, scale3(camera.pan, radius));
    let (direction, _, up) = camera_basis(camera);
    let eye = add3(target, scale3(direction, distance));

    let view = look_at_rh(eye, target, up);
    let near = (radius * 0.002).max(0.0001);
    let far = distance + radius * (8.0 + length3(camera.pan));
    let projection = perspective_rh_zo(fov_y, aspect.max(0.05), near, far);
    projection.mul(view)
}

fn camera_basis(camera: CameraState) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sin_yaw, cos_yaw) = camera.yaw.sin_cos();
    let (sin_pitch, cos_pitch) = camera.pitch.sin_cos();
    let direction = normalize3([sin_yaw * cos_pitch, sin_pitch, cos_yaw * cos_pitch]);
    let right = normalize3([cos_yaw, 0.0, -sin_yaw]);
    let up = normalize3(cross3(direction, right));
    (direction, right, up)
}

fn wrap_angle(value: f32) -> f32 {
    (value + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

fn look_at_rh(eye: [f32; 3], center: [f32; 3], up: [f32; 3]) -> Mat4 {
    let forward = normalize3(sub3(center, eye));
    let side = normalize3(cross3(forward, up));
    let corrected_up = cross3(side, forward);
    Mat4([
        [side[0], side[1], side[2], -dot3(side, eye)],
        [
            corrected_up[0],
            corrected_up[1],
            corrected_up[2],
            -dot3(corrected_up, eye),
        ],
        [-forward[0], -forward[1], -forward[2], dot3(forward, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

fn perspective_rh_zo(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    Mat4([
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), (far * near) / (near - far)],
        [0.0, 0.0, -1.0, 0.0],
    ])
}

fn add3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length3(value: [f32; 3]) -> f32 {
    dot3(value, value).sqrt()
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = length3(value);
    if length <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        [value[0] / length, value[1] / length, value[2] / length]
    }
}

#[derive(Clone, Debug)]
enum JsonValue {
    Null,
    Bool,
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

impl JsonValue {
    fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            Self::Object(values) => values.get(key),
            _ => None,
        }
    }

    fn require(&self, key: &str) -> Result<&JsonValue, String> {
        self.get(key)
            .ok_or_else(|| format!("JSON object is missing required key {key:?}"))
    }
}

fn json_array(value: &JsonValue) -> Result<&[JsonValue], String> {
    match value {
        JsonValue::Array(values) => Ok(values),
        _ => Err("JSON value is not an array".into()),
    }
}

fn json_usize(value: &JsonValue) -> Result<usize, String> {
    match value {
        JsonValue::Number(number) if number.is_finite() && *number >= 0.0 => {
            let integer = number.trunc();
            if integer == *number && integer <= usize::MAX as f64 {
                Ok(integer as usize)
            } else {
                Err("JSON number is not a non-negative integer".into())
            }
        }
        _ => Err("JSON value is not a non-negative integer".into()),
    }
}

fn json_f32(value: &JsonValue) -> Result<f32, String> {
    match value {
        JsonValue::Number(number) if number.is_finite() => Ok(*number as f32),
        _ => Err("JSON value is not a finite number".into()),
    }
}

fn json_string(value: &JsonValue) -> Result<&str, String> {
    match value {
        JsonValue::String(text) => Ok(text),
        _ => Err("JSON value is not a string".into()),
    }
}

struct JsonParser<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    fn parse(&mut self) -> Result<JsonValue, String> {
        let value = self.parse_value()?;
        self.skip_whitespace();
        if self.offset != self.input.len() {
            return Err(format!(
                "unexpected trailing JSON data at byte {}",
                self.offset
            ));
        }
        Ok(value)
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'n') => {
                self.expect_literal(b"null")?;
                Ok(JsonValue::Null)
            }
            Some(b't') => {
                self.expect_literal(b"true")?;
                Ok(JsonValue::Bool)
            }
            Some(b'f') => {
                self.expect_literal(b"false")?;
                Ok(JsonValue::Bool)
            }
            Some(b'"') => Ok(JsonValue::String(self.parse_string()?)),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(other) => Err(format!(
                "unexpected JSON byte 0x{other:02X} at offset {}",
                self.offset
            )),
            None => Err("unexpected end of JSON".into()),
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect_byte(b'[')?;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.consume_if(b']') {
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.parse_value()?);
            self.skip_whitespace();
            if self.consume_if(b']') {
                break;
            }
            self.expect_byte(b',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect_byte(b'{')?;
        let mut values = HashMap::new();
        self.skip_whitespace();
        if self.consume_if(b'}') {
            return Ok(JsonValue::Object(values));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect_byte(b':')?;
            let value = self.parse_value()?;
            values.insert(key, value);
            self.skip_whitespace();
            if self.consume_if(b'}') {
                break;
            }
            self.expect_byte(b',')?;
        }
        Ok(JsonValue::Object(values))
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect_byte(b'"')?;
        let mut bytes = Vec::new();
        loop {
            let byte = self
                .next()
                .ok_or_else(|| "unterminated JSON string".to_owned())?;
            match byte {
                b'"' => {
                    return String::from_utf8(bytes)
                        .map_err(|_| "JSON string contains invalid UTF-8".to_owned());
                }
                b'\\' => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| "unterminated JSON escape".to_owned())?;
                    match escaped {
                        b'"' | b'\\' | b'/' => bytes.push(escaped),
                        b'b' => bytes.push(0x08),
                        b'f' => bytes.push(0x0C),
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'u' => {
                            let mut code = self.parse_hex_u16()? as u32;
                            if (0xD800..=0xDBFF).contains(&code) {
                                self.expect_byte(b'\\')?;
                                self.expect_byte(b'u')?;
                                let low = self.parse_hex_u16()? as u32;
                                if !(0xDC00..=0xDFFF).contains(&low) {
                                    return Err("invalid JSON UTF-16 surrogate pair".into());
                                }
                                code = 0x1_0000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                            }
                            let character = char::from_u32(code)
                                .ok_or_else(|| "invalid JSON unicode escape".to_owned())?;
                            let mut encoded = [0u8; 4];
                            bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                        }
                        _ => return Err("unsupported JSON escape sequence".into()),
                    }
                }
                0x00..=0x1F => return Err("unescaped control byte in JSON string".into()),
                other => bytes.push(other),
            }
        }
    }

    fn parse_hex_u16(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self
                .next()
                .ok_or_else(|| "truncated JSON unicode escape".to_owned())?;
            let nibble = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return Err("invalid hex digit in JSON unicode escape".into()),
            };
            value = (value << 4) | u16::from(nibble);
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.offset;
        if self.consume_if(b'-') {}
        self.consume_digits();
        if self.consume_if(b'.') {
            self.consume_digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.offset += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.offset += 1;
            }
            self.consume_digits();
        }
        let text = std::str::from_utf8(&self.input[start..self.offset])
            .map_err(|_| "JSON number contains invalid UTF-8".to_owned())?;
        let number = text
            .parse::<f64>()
            .map_err(|_| format!("invalid JSON number {text:?}"))?;
        Ok(JsonValue::Number(number))
    }

    fn consume_digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.offset += 1;
        }
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Result<(), String> {
        if self.input.get(self.offset..self.offset + literal.len()) == Some(literal) {
            self.offset += literal.len();
            Ok(())
        } else {
            Err(format!("invalid JSON literal at byte {}", self.offset))
        }
    }

    fn expect_byte(&mut self, expected: u8) -> Result<(), String> {
        self.skip_whitespace();
        match self.next() {
            Some(actual) if actual == expected => Ok(()),
            Some(actual) => Err(format!(
                "expected JSON byte 0x{expected:02X}, got 0x{actual:02X} at byte {}",
                self.offset.saturating_sub(1)
            )),
            None => Err(format!(
                "expected JSON byte 0x{expected:02X}, got end of input"
            )),
        }
    }

    fn consume_if(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t' | 0)) {
            self.offset += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.offset).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.offset += 1;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_altair_glb_scene_metadata_is_parseable() {
        let glb = embedded_assets::get(GLB_PATH).unwrap();
        let (root, bin) = parse_glb(glb).expect("embedded Altair GLB must parse");
        assert!(bin.len() > 1_000_000);
        let nodes = json_array(root.require("nodes").unwrap()).unwrap();
        let meshes = json_array(root.require("meshes").unwrap()).unwrap();
        assert!(
            nodes.len() >= 620,
            "Altair GLB unexpectedly lost scene nodes"
        );
        assert!(meshes.len() >= 584, "Altair GLB unexpectedly lost meshes");
    }

    #[test]
    fn embedded_altair_bindings_define_expected_led_order() {
        let materials = load_led_material_bindings().expect("embedded LED bindings must parse");
        assert_eq!(materials.len(), LED_COUNT);
    }

    #[test]
    fn embedded_altair_glb_builds_expected_triangle_scene() {
        let mesh = load_static_mesh().expect("embedded Altair GLB must build a static scene");
        assert!(mesh.vertices.len() > 600_000);
        assert!(mesh.indices.len() > 1_000_000);
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(mesh.radius > 0.2 && mesh.radius < 1.0);

        let mut seen = [false; LED_COUNT];
        for vertex in &mesh.vertices {
            if let Ok(index) = usize::try_from(vertex.led_index) {
                if index < LED_COUNT {
                    seen[index] = true;
                }
            }
        }
        assert!(
            seen.into_iter().all(|present| present),
            "every bindings.json LED material must exist in the runtime GLB"
        );
    }

    #[test]
    fn free_camera_basis_remains_valid_through_poles() {
        for pitch in [
            0.0,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
            -std::f32::consts::FRAC_PI_2,
        ] {
            let camera = CameraState {
                pitch,
                yaw: 0.73,
                ..CameraState::default()
            };
            let (direction, right, up) = camera_basis(camera);
            assert!((length3(direction) - 1.0).abs() < 1.0e-5);
            assert!((length3(right) - 1.0).abs() < 1.0e-5);
            assert!((length3(up) - 1.0).abs() < 1.0e-5);
            assert!(dot3(direction, right).abs() < 1.0e-5);
            assert!(dot3(direction, up).abs() < 1.0e-5);
            assert!(dot3(right, up).abs() < 1.0e-5);
        }
    }

    #[test]
    fn json_parser_handles_unicode_and_numbers() {
        let mut parser = JsonParser::new(br#"{"name":"Altair \u2605","v":[-1,2.5e1,true,null]}"#);
        let value = parser.parse().unwrap();
        assert_eq!(
            json_string(value.require("name").unwrap()).unwrap(),
            "Altair ★"
        );
        let values = json_array(value.require("v").unwrap()).unwrap();
        assert_eq!(json_f32(&values[0]).unwrap(), -1.0);
        assert_eq!(json_f32(&values[1]).unwrap(), 25.0);
    }
}
