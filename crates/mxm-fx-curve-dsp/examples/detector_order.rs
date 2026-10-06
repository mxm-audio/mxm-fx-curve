//! C1 detector-order decision probe.
//!
//! Compares smoothing the detected level before a hard curve against smoothing the gain produced by
//! that curve. It reports post-curve control-signal energy above 10 kHz under the fastest supported
//! ballistics. Lower means the smoothing actually bounded the signal applied to the audio path.
//!
//! `cargo run -p mxm-fx-curve-dsp --release --example detector_order`

use mxm_fx_curve_dsp::{ControlPoint, Curve, CurveEngine, CurveTable, StageMode, StageSpec};
use mxm_measure::spectrum::{N, fft, periods_for};
use std::f64::consts::TAU;

fn main() {
    let rate = 48_000.0f64;
    let periods = periods_for(997.0, rate, N);
    let increment = TAU * periods as f64 / N as f64;
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.45, 0.45),
        ControlPoint::linear(1.0, 0.58),
    ])
    .unwrap();
    let table = CurveTable::prepare(&curve);
    let coefficient = (-1.0 / (0.0001 * rate)).exp();

    let mut production = CurveEngine::prepare(
        &[StageSpec {
            curve,
            mode: StageMode::Detector {
                attack_ms: 0.1,
                release_ms: 0.1,
            },
        }],
        rate as f32,
    )
    .unwrap();
    let mut post_curve = Vec::with_capacity(N);
    let mut pre_curve = Vec::with_capacity(N);
    let mut envelope = 0.0f64;
    for sample in 0..N * 2 {
        let level = 0.6 + 0.39 * (increment * sample as f64).sin();
        production.process([level as f32, level as f32], 1.0);
        envelope = level + coefficient * (envelope - level);
        let before_gain = table.evaluate(envelope as f32) as f64 / envelope.max(1.0e-12);
        if sample >= N {
            post_curve.push(production.stages()[0].detector_gain().unwrap());
            pre_curve.push(before_gain as f32);
        }
    }

    println!("detector smoothing-order probe, fastest 0.1 ms ballistics");
    println!(
        "level before curve: {:7.2} dB high-band/AC",
        high_band_ratio_db(&pre_curve, rate, 10_000.0)
    );
    println!(
        "gain after curve:   {:7.2} dB high-band/AC",
        high_band_ratio_db(&post_curve, rate, 10_000.0)
    );
}

fn high_band_ratio_db(signal: &[f32], rate: f64, boundary_hz: f64) -> f64 {
    let mean = signal.iter().map(|&sample| sample as f64).sum::<f64>() / signal.len() as f64;
    let mut re: Vec<f64> = signal.iter().map(|&sample| sample as f64 - mean).collect();
    let mut im = vec![0.0; signal.len()];
    fft(&mut re, &mut im).expect("power-of-two finite probe");
    let first = (boundary_hz * signal.len() as f64 / rate).ceil() as usize;
    let nyquist = signal.len() / 2;
    let total: f64 = (1..nyquist)
        .map(|bin| re[bin] * re[bin] + im[bin] * im[bin])
        .sum();
    let high: f64 = (first..nyquist)
        .map(|bin| re[bin] * re[bin] + im[bin] * im[bin])
        .sum();
    10.0 * (high / total).log10()
}
