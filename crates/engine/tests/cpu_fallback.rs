//! With the GPU switched off, the guided filter must give exactly the CPU result.
//! Runs as its own test binary so the environment is set before the GPU state is created.

#[test]
fn guided_filter_auto_falls_back_to_cpu_exactly() {
    std::env::set_var("PHOTO_ENGINE_GPU", "0");
    let (w, h) = (40usize, 30usize);
    let g: Vec<f32> = (0..w * h).map(|i| ((i * 37) % 101) as f32 / 100.0).collect();
    assert!(engine::gpu::guided_filter(&g, &g, w, h, 3, 0.01).is_none(), "GPU should be disabled");
    let auto = engine::guided::guided_filter_auto(&g, &g, w, h, 3, 0.01);
    let cpu = engine::guided::guided_filter(&g, &g, w, h, 3, 0.01);
    assert_eq!(auto, cpu);
}
