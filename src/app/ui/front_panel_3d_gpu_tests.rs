//! Opt-in real GPU regression: cargo test --lib gpu_color_and_material_render -- --ignored
//! Set RUSTAIR_3D_CAPTURE_DIR to also save the actual emulator render for inspection.
use super::*;
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn read_present(
    renderer: &LoadedRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) -> Vec<u8> {
    let extent = wgpu::Extent3d {
        width: size[0],
        height: size[1],
        depth_or_array_layers: 1,
    };
    let output = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("3D test output"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = output.create_view(&Default::default());
    let stride = (size[0] * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();
        renderer.paint(&mut pass);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &output,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size[1]),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
    device.poll(wgpu::PollType::Wait).unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let mut bytes = Vec::new();
    for row in mapped.chunks_exact(stride as usize) {
        bytes.extend_from_slice(&row[..size[0] as usize * 4]);
    }
    bytes
}

#[test]
#[ignore = "requires a working Vulkan/Metal/DX12 adapter; exercises real render and readback"]
fn gpu_color_and_material_render() {
    let instance = wgpu::Instance::default();
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPU adapter required");
    let (device, queue) =
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let size = [1024, 640];
    let mut reference: Option<Vec<u8>> = None;
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut renderer = LoadedRenderer::new(&device, format).unwrap();
        let switch_states = switch_runtime::rest_switch_states(&renderer.switch_runtime);
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer.prepare(
            &device,
            &queue,
            &mut encoder,
            size,
            CameraState {
                zoom: 0.65,
                ..CameraState::default()
            },
            [0.0; LED_COUNT],
            switch_states,
        );
        queue.submit([encoder.finish()]);
        let frame = read_present(&renderer, &device, &queue, format, size);
        if let Some(ref other) = reference {
            let worst = frame
                .iter()
                .zip(other)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(worst <= 2, "sRGB and UNORM presentations differ by {worst}");
        } else {
            if let Ok(dir) = std::env::var("RUSTAIR_3D_CAPTURE_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                image::save_buffer(
                    std::path::Path::new(&dir).join("altair-render.png"),
                    &frame,
                    size[0],
                    size[1],
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            reference = Some(frame);
        }
        // Known linear 18% gray must reach either surface as ~118/255, not 46
        // (missing sRGB conversion) or ~181 (double conversion).
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &renderer.target.as_ref().unwrap().color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.18,
                            g: 0.18,
                            b: 0.18,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        queue.submit([encoder.finish()]);
        let gray = read_present(&renderer, &device, &queue, format, size);
        for value in &gray[..3] {
            assert!((117..=119).contains(value), "18% gray encoded as {value}");
        }
    }
}
