use mxm_fx_curve_dsp::{AntialiasMode, ControlPoint, Curve, CurveEngine, StageMode, StageSpec};
use mxm_measure::spectrum::{N, alias_to_signal_db, periods_for};
use std::f64::consts::TAU;

#[test]
fn adaa_improves_smooth_corner_and_discontinuous_curves() {
    let curves = [
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::curve(0.35, 0.72),
            ControlPoint::curve(0.72, 0.9),
            ControlPoint::curve(1.0, 1.0),
        ])
        .unwrap(),
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::linear(0.35, 0.78),
            ControlPoint::curve(1.0, 1.0),
        ])
        .unwrap(),
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::curve(0.5, 0.2),
            ControlPoint::curve(0.5, 0.82),
            ControlPoint::curve(1.0, 1.0),
        ])
        .unwrap(),
    ];
    // Measured by `examples/curve_alias.rs`: first-order ADAA improved these by 7.85, 7.80 and
    // 10.85 dB respectively at 48 kHz. The margins preserve the decision without encoding the
    // development machine's last decimals as product truth.
    let minimum_improvements = [6.0, 6.0, 8.0];
    for (curve, minimum) in curves.into_iter().zip(minimum_improvements) {
        let direct = alias_level(&curve, AntialiasMode::Direct);
        let adaa = alias_level(&curve, AntialiasMode::Adaa);
        assert!(
            direct - adaa >= minimum,
            "ADAA improvement {direct:.2} - {adaa:.2} dB did not reach {minimum:.2} dB"
        );
    }
}

fn alias_level(curve: &Curve, antialias: AntialiasMode) -> f64 {
    let rate = 48_000.0;
    let periods = periods_for(997.0, rate, N);
    let increment = TAU * periods as f64 / N as f64;
    let mut engine = CurveEngine::prepare(
        &[StageSpec {
            curve: curve.clone(),
            mode: StageMode::Memoryless { antialias },
        }],
        rate as f32,
    )
    .unwrap();
    let mut output = Vec::with_capacity(N);
    for sample in 0..N * 2 {
        let input = (0.95 * (increment * sample as f64).sin()) as f32;
        let shaped = engine.process([input, input], 1.0)[0];
        if sample >= N {
            output.push(shaped);
        }
    }
    alias_to_signal_db(&output, periods).unwrap()
}
