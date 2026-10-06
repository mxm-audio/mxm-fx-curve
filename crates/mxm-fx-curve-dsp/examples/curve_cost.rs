//! C1 bounded-cost harness for maximum-size states and a two-chain publication transition.
//!
//! `cargo run -p mxm-fx-curve-dsp --release --example curve_cost`

use mxm_fx_curve_dsp::{
    AntialiasMode, ControlPoint, Curve, CurveEngine, MAX_POINTS, MAX_STAGES, StageMode, StageSpec,
};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    println!("curve maximum-state transition cost");
    println!("points per stage:      {MAX_POINTS}");
    println!("stages per chain:      {MAX_STAGES}");
    println!("simultaneous chains:   2\n");
    for rate in [48_000.0f32, 192_000.0, 768_000.0] {
        measure(rate);
    }
}

fn measure(rate: f32) {
    let frames = rate as usize * 5;
    let specs = maximum_chain();
    let preparation_start = Instant::now();
    let mut old = CurveEngine::prepare(&specs, rate).expect("maximum chain prepares");
    let mut new = CurveEngine::prepare(&specs, rate).expect("second maximum chain prepares");
    let preparation = preparation_start.elapsed();

    let start = Instant::now();
    let mut checksum = 0.0f32;
    for i in 0..frames {
        let phase = i as f32 * 0.017_453_292;
        let input = [phase.sin() * 0.9, (phase * 1.01).sin() * 0.9];
        let a = old.process(black_box(input), 1.0);
        let b = new.process(black_box(input), 1.0);
        checksum += black_box(a[0] + b[1]);
    }
    let elapsed = start.elapsed();
    let audio_seconds = frames as f64 / rate as f64;
    let realtime_ratio = audio_seconds / elapsed.as_secs_f64();

    println!("sample rate:           {rate:.0} Hz");
    println!(
        "table preparation:     {:.3} ms",
        preparation.as_secs_f64() * 1_000.0
    );
    println!("wall time for 5 s:     {:.3} s", elapsed.as_secs_f64());
    println!("realtime throughput:   {realtime_ratio:.1}x");
    println!("checksum:              {checksum:.6}\n");
}

fn maximum_chain() -> Vec<StageSpec> {
    let mut points = Vec::with_capacity(MAX_POINTS);
    for index in 0..MAX_POINTS {
        let x = if index == MAX_POINTS / 2 {
            (index - 1) as f32 / (MAX_POINTS - 1) as f32
        } else {
            index as f32 / (MAX_POINTS - 1) as f32
        };
        let y = if index == MAX_POINTS / 2 {
            (x.sqrt() + 0.15).min(1.0)
        } else {
            x.sqrt()
        };
        points.push(if index > 0 && index + 1 < MAX_POINTS && index % 10 == 0 {
            ControlPoint::linear(x, y)
        } else {
            ControlPoint::curve(x, y)
        });
    }
    let curve = Curve::new(points).unwrap();
    (0..MAX_STAGES)
        .map(|index| StageSpec {
            curve: curve.clone(),
            mode: if index % 2 == 0 {
                StageMode::Memoryless {
                    antialias: AntialiasMode::Auto,
                }
            } else {
                StageMode::Detector {
                    attack_ms: 10.0,
                    release_ms: 100.0,
                }
            },
        })
        .collect()
}
