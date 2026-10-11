//! GPU compute path (wgpu, WGSL). First kernel: the separable box blur that the guided filter
//! is built from. CPU stays the reference; `box_blur` returns `None` when no adapter exists, and
//! callers fall back to `adaptive_exposure::box_blur`.
//!
//! The blur uses a running sum: O(1) per pixel, independent of the radius.

use std::sync::OnceLock;

const SHADER: &str = r#"
struct Params { w: u32, h: u32, r: u32, dir: u32 };
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst: array<f32>;

// One invocation per row (dir 0) or per column (dir 1), sliding a running sum: O(1) per pixel,
// so the cost does not grow with the radius.
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let r = i32(p.r);
    let inv = 1.0 / f32(2 * r + 1);
    if (p.dir == 0u) {
        if (id.x >= p.h) {
            return;
        }
        let row = id.x * p.w;
        let last = i32(p.w) - 1;
        var sum = 0.0;
        for (var k = -r; k <= r; k = k + 1) {
            sum = sum + src[row + u32(clamp(k, 0, last))];
        }
        for (var x = 0; x < i32(p.w); x = x + 1) {
            dst[row + u32(x)] = sum * inv;
            sum = sum + src[row + u32(clamp(x + r + 1, 0, last))] - src[row + u32(clamp(x - r, 0, last))];
        }
    } else {
        if (id.x >= p.w) {
            return;
        }
        let col = id.x;
        let last = i32(p.h) - 1;
        var sum = 0.0;
        for (var k = -r; k <= r; k = k + 1) {
            sum = sum + src[u32(clamp(k, 0, last)) * p.w + col];
        }
        for (var y = 0; y < i32(p.h); y = y + 1) {
            dst[u32(y) * p.w + col] = sum * inv;
            sum = sum + src[u32(clamp(y + r + 1, 0, last)) * p.w + col] - src[u32(clamp(y - r, 0, last)) * p.w + col];
        }
    }
}
"#;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Largest buffer the device accepts, in bytes. Bigger images use the CPU.
    max_buffer: u64,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

static GPU: OnceLock<Option<Gpu>> = OnceLock::new();

fn gpu() -> Option<&'static Gpu> {
    GPU.get_or_init(|| pollster::block_on(init())).as_ref()
}

async fn init() -> Option<Gpu> {
    // PHOTO_ENGINE_GPU=0 forces the CPU path, for debugging drivers or comparing results.
    if std::env::var("PHOTO_ENGINE_GPU").as_deref() == Ok("0") {
        return None;
    }
    // Any backend the platform offers: Metal, Vulkan, DirectX 12 or OpenGL. WGPU_BACKEND can narrow it.
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .ok()?;
    // A software renderer is slower than the CPU reference, so it is not used.
    if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
        return None;
    }
    // Ask for the adapter's own limits, so large photos fit in one storage buffer.
    let desc = wgpu::DeviceDescriptor { required_limits: adapter.limits(), ..Default::default() };
    let (device, queue) = adapter.request_device(&desc).await.ok()?;
    let max_buffer = device.limits().max_storage_buffer_binding_size as u64;

    let entry = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    };
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("box_blur"),
        entries: &[
            entry(0, wgpu::BufferBindingType::Uniform),
            entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
            entry(2, wgpu::BufferBindingType::Storage { read_only: false }),
        ],
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("box_blur"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("box_blur"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    Some(Gpu { device, queue, max_buffer, layout, pipeline })
}

fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Separable box blur with clamp-to-edge borders, same result as the CPU `box_blur` up to
/// float summation order. Returns `None` when no GPU adapter is available.
pub fn box_blur(src: &[f32], w: usize, h: usize, r: usize) -> Option<Vec<f32>> {
    let g = gpu()?;
    let n = w * h;
    let size = (n * 4) as u64;
    if size > g.max_buffer {
        return None;
    }
    let storage = |label: &str| {
        g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    };
    let buf_a = storage("a");
    let buf_b = storage("b");
    g.queue.write_buffer(&buf_a, 0, &to_bytes(src));

    let mut encoder = g.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    // Horizontal pass a -> b, then vertical pass b -> a.
    for (dir, input, output) in [(0u32, &buf_a, &buf_b), (1u32, &buf_b, &buf_a)] {
        let mut params = Vec::with_capacity(16);
        for v in [w as u32, h as u32, r as u32, dir] {
            params.extend_from_slice(&v.to_le_bytes());
        }
        let uniform = g.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        g.queue.write_buffer(&uniform, 0, &params);
        let bind = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &g.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: output.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
        pass.set_pipeline(&g.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        // Horizontal pass: one thread per row. Vertical pass: one thread per column.
        let threads = if dir == 0 { h as u32 } else { w as u32 };
        pass.dispatch_workgroups(threads.div_ceil(256), 1, 1);
    }

    let staging = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&buf_a, 0, &staging, 0, size);
    g.queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    g.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    rx.recv().ok()?.ok()?;
    let data = slice.get_mapped_range().ok()?;
    let out: Vec<f32> = data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    drop(data);
    staging.unmap();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_box_blur_matches_cpu() {
        let (w, h, r) = (37usize, 23usize, 3usize);
        let mut s: u32 = 4242;
        let src: Vec<f32> = (0..w * h)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 8) as f32 / (1u32 << 24) as f32
            })
            .collect();
        let Some(gpu_out) = box_blur(&src, w, h, r) else {
            eprintln!("no GPU adapter: skipping GPU parity test");
            return;
        };
        let cpu = crate::adaptive_exposure::box_blur(&src, w, h, r);
        let worst = gpu_out.iter().zip(&cpu).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "GPU and CPU differ by {worst}");
    }
}

/// Guided filter (He et al.) with all box blurs on the GPU. The per-pixel steps stay on the CPU,
/// which is cheap next to the blurs. Same algorithm as `guided::guided_filter`.
pub fn guided_filter(guidance: &[f32], src: &[f32], w: usize, h: usize, radius: usize, eps: f32) -> Option<Vec<f32>> {
    let mean_i = box_blur(guidance, w, h, radius)?;
    let mean_p = box_blur(src, w, h, radius)?;
    let ii: Vec<f32> = guidance.iter().map(|i| i * i).collect();
    let ip: Vec<f32> = guidance.iter().zip(src).map(|(i, p)| i * p).collect();
    let corr_i = box_blur(&ii, w, h, radius)?;
    let corr_ip = box_blur(&ip, w, h, radius)?;
    let n = w * h;
    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];
    for k in 0..n {
        let var_i = corr_i[k] - mean_i[k] * mean_i[k];
        let cov_ip = corr_ip[k] - mean_i[k] * mean_p[k];
        a[k] = cov_ip / (var_i + eps);
        b[k] = mean_p[k] - a[k] * mean_i[k];
    }
    let mean_a = box_blur(&a, w, h, radius)?;
    let mean_b = box_blur(&b, w, h, radius)?;
    Some((0..n).map(|k| mean_a[k] * guidance[k] + mean_b[k]).collect())
}

#[cfg(test)]
mod guided_parity_tests {
    use super::*;

    #[test]
    fn gpu_guided_filter_matches_cpu() {
        let (w, h, r) = (61usize, 47usize, 5usize);
        let mut s: u32 = 9001;
        let g: Vec<f32> = (0..w * h)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                -4.0 + 8.0 * (s >> 8) as f32 / (1u32 << 24) as f32
            })
            .collect();
        let Some(gpu_out) = guided_filter(&g, &g, w, h, r, 0.01) else {
            eprintln!("no GPU adapter: skipping GPU guided parity test");
            return;
        };
        let cpu = crate::guided::guided_filter(&g, &g, w, h, r, 0.01);
        let worst = gpu_out.iter().zip(&cpu).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-3, "GPU and CPU guided filter differ by {worst}");
    }
}
