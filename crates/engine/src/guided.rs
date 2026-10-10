//! Guided filter (He, Sun, Tang 2010), edge-preserving. Ported from the author's own
//! Limestone repo (`crates/adjust-core/src/filter.rs`), adapted to this engine's `box_blur`.
//! Parameters: `guidance` steers the edges, `src` is smoothed, `radius` is the window, `eps`
//! sets how strongly flat-vs-edge is decided.

use crate::adaptive_exposure::box_blur;

pub fn guided_filter(guidance: &[f32], src: &[f32], w: usize, h: usize, radius: usize, eps: f32) -> Vec<f32> {
    let mean_i = box_blur(guidance, w, h, radius);
    let mean_p = box_blur(src, w, h, radius);
    let ii: Vec<f32> = guidance.iter().map(|i| i * i).collect();
    let ip: Vec<f32> = guidance.iter().zip(src).map(|(i, p)| i * p).collect();
    let corr_i = box_blur(&ii, w, h, radius);
    let corr_ip = box_blur(&ip, w, h, radius);

    let n = w * h;
    let mut a = vec![0.0f32; n];
    let mut b = vec![0.0f32; n];
    for k in 0..n {
        let var_i = corr_i[k] - mean_i[k] * mean_i[k];
        let cov_ip = corr_ip[k] - mean_i[k] * mean_p[k];
        a[k] = cov_ip / (var_i + eps);
        b[k] = mean_p[k] - a[k] * mean_i[k];
    }
    let mean_a = box_blur(&a, w, h, radius);
    let mean_b = box_blur(&b, w, h, radius);
    (0..n).map(|k| mean_a[k] * guidance[k] + mean_b[k]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_input_is_unchanged() {
        let g = vec![0.4f32; 36];
        let out = guided_filter(&g, &g, 6, 6, 2, 1e-3);
        assert!(out.iter().all(|v| (v - 0.4).abs() < 1e-4));
    }

    #[test]
    fn keeps_a_step_edge_sharper_than_a_box_blur() {
        // 12x1 step from 0 to 1 at x=6. Guided filtering on itself should keep the edge.
        let step: Vec<f32> = (0..12).map(|x| if x < 6 { 0.0 } else { 1.0 }).collect();
        let guided = guided_filter(&step, &step, 12, 1, 2, 1e-4);
        let boxed = box_blur(&step, 12, 1, 2);
        // Pixels just either side of the edge: guided moves less than the box blur does.
        assert!((guided[5] - 0.0).abs() < (boxed[5] - 0.0).abs());
        assert!((guided[6] - 1.0).abs() < (boxed[6] - 1.0).abs());
    }
}
