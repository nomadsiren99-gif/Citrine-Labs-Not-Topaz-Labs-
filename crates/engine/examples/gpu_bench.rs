//! Gate 7 timing: CPU versus GPU guided filter on a full-size photo's luma.
//! Usage: cargo run -p engine --release --example gpu_bench -- <image.jpg>

use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("image path");
    let img = image::open(path).unwrap().to_luma32f();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let luma: Vec<f32> = img.as_raw().iter().map(|v| v.max(1e-4).log2()).collect();
    println!("{w}x{h} ({:.1} MP)", (w * h) as f32 / 1e6);
    for radius in [8usize, 30, 100] {
        let t = Instant::now();
        let cpu = engine::guided::guided_filter(&luma, &luma, w, h, radius, 0.01);
        let cpu_ms = t.elapsed().as_secs_f64() * 1e3;
        // First GPU call includes device setup; time a second call for steady-state.
        let _ = engine::gpu::guided_filter(&luma, &luma, w, h, radius, 0.01);
        let t = Instant::now();
        let gpu = engine::gpu::guided_filter(&luma, &luma, w, h, radius, 0.01);
        let gpu_ms = t.elapsed().as_secs_f64() * 1e3;
        let worst = gpu.as_ref().map(|g| g.iter().zip(&cpu).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max));
        println!("radius {radius:>3}: CPU {cpu_ms:8.1} ms   GPU {gpu_ms:8.1} ms   max diff {worst:?}");
    }
}
