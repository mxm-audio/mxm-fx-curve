//! C1 antialias decision harness.
//!
//! Renders the three accepted curve classes with direct lookup and first-order ADAA, then reports
//! unwanted-bin energy and deviation from a band-limited high-rate reference. Run with:
//!
//! `cargo run -p mxm-fx-curve-dsp --release --example curve_alias`

use mxm_fx_curve_dsp::{
    AntialiasMode, ControlPoint, Curve, CurveEngine, CurveTable, StageMode, StageSpec,
};
use mxm_measure::spectrum::{N, alias_to_signal_db, periods_for};
use std::f64::consts::{PI, TAU};

fn main() {
    let rate = 48_000.0;
    println!("curve alias measurement: {rate:.0} Hz, N={N}");
    println!("alias is unwanted-bin RMS relative to wanted harmonics");
    println!("error is RMS deviation from a 16x band-limited reference (lower is better)");
    for requested in [997.0, 7_000.0] {
        let periods = periods_for(requested, rate, N);
        let frequency = periods as f64 * rate / N as f64;
        println!("\nprobe frequency: {frequency:.3} Hz");
        for (name, curve) in curves() {
            let reference = oversampled_reference(&curve, periods, 16);
            print!("{name:14}");
            for mode in [AntialiasMode::Direct, AntialiasMode::Adaa] {
                let rendered = render(&curve, mode, rate as f32, periods);
                let aliases =
                    alias_to_signal_db(&rendered, periods).expect("finite non-silent render");
                let error = error_to_reference_db(&rendered, &reference);
                print!("  {mode:?}: alias {aliases:7.2} dB, error {error:7.2} dB");
            }
            println!();
        }
    }
}

fn render(curve: &Curve, antialias: AntialiasMode, rate: f32, periods: usize) -> Vec<f32> {
    let spec = StageSpec {
        curve: curve.clone(),
        mode: StageMode::Memoryless { antialias },
    };
    let mut engine = CurveEngine::prepare(&[spec], rate).expect("measurement stage prepares");
    let increment = TAU * periods as f64 / N as f64;
    let mut output = Vec::with_capacity(N);
    // Two complete analysis windows make the ADAA history periodic before the retained window.
    for sample in 0..N * 2 {
        let input = (0.95 * (increment * sample as f64).sin()) as f32;
        let shaped = engine.process([input, input], 1.0)[0];
        if sample >= N {
            output.push(shaped);
        }
    }
    output
}

fn oversampled_reference(curve: &Curve, periods: usize, factor: usize) -> Vec<f32> {
    let table = CurveTable::prepare(curve);
    let taps = decimation_taps(factor);
    let middle = (taps.len() - 1) as f64 / 2.0;
    let increment = TAU * periods as f64 / N as f64;
    // Start the high-rate source before phase zero by the FIR's group delay. The symmetric filter's
    // retained output is then aligned to the host-rate samples rather than merely being clean.
    let high: Vec<f64> = (0..N * factor + taps.len())
        .map(|sample| {
            let phase = increment * (sample as f64 - middle) / factor as f64;
            let input = 0.95 * phase.sin();
            input.signum() * table.evaluate(input.abs() as f32) as f64
        })
        .collect();
    (0..N)
        .map(|sample| {
            taps.iter()
                .enumerate()
                .map(|(tap, &coefficient)| coefficient * high[sample * factor + tap])
                .sum::<f64>() as f32
        })
        .collect()
}

fn decimation_taps(factor: usize) -> Vec<f64> {
    let length = 64 * factor + 1;
    let cutoff = 0.45 / factor as f64;
    let middle = (length - 1) as f64 / 2.0;
    let mut taps: Vec<f64> = (0..length)
        .map(|index| {
            let x = index as f64 - middle;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * PI * cutoff * x).sin() / (PI * x)
            };
            let window = 0.42 - 0.5 * (2.0 * PI * index as f64 / (length - 1) as f64).cos()
                + 0.08 * (4.0 * PI * index as f64 / (length - 1) as f64).cos();
            sinc * window
        })
        .collect();
    let sum: f64 = taps.iter().sum();
    for tap in &mut taps {
        *tap /= sum;
    }
    taps
}

fn error_to_reference_db(rendered: &[f32], reference: &[f32]) -> f64 {
    let error: f64 = rendered
        .iter()
        .zip(reference)
        .map(|(&actual, &expected)| {
            let difference = actual as f64 - expected as f64;
            difference * difference
        })
        .sum();
    let signal: f64 = reference
        .iter()
        .map(|&sample| (sample as f64).powi(2))
        .sum();
    10.0 * (error / signal).log10()
}

fn curves() -> [(&'static str, Curve); 3] {
    let smooth = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.35, 0.72),
        ControlPoint::curve(0.72, 0.9),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let corner = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.35, 0.78),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let discontinuous = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.2),
        ControlPoint::curve(0.5, 0.82),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    [
        ("smooth", smooth),
        ("corner", corner),
        ("discontinuous", discontinuous),
    ]
}
