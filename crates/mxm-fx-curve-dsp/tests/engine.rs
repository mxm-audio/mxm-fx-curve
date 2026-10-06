use mxm_fx_curve_dsp::{
    AntialiasMode, ControlPoint, Curve, CurveEngine, CurveError, CurveTable, Handle,
    MAX_MAKEUP_GAIN, MAX_POINTS, MAX_STAGES, StageMode, StageSpec,
};

fn stage(curve: Curve, mode: StageMode) -> StageSpec {
    StageSpec { curve, mode }
}

fn memoryless(curve: Curve, antialias: AntialiasMode) -> StageSpec {
    stage(curve, StageMode::Memoryless { antialias })
}

#[test]
fn identity_is_exact_in_direct_and_adaa_modes() {
    for antialias in [AntialiasMode::Direct, AntialiasMode::Adaa] {
        let mut engine =
            CurveEngine::prepare(&[memoryless(Curve::identity(), antialias)], 48_000.0)
                .expect("identity prepares");
        for i in -100..=100 {
            let x = i as f32 / 100.0;
            assert_eq!(engine.process([x, -x], 1.0), [x, -x]);
        }
    }
}

#[test]
fn nominal_makeup_uses_the_complete_static_curve_and_stays_bounded() {
    let identity =
        CurveEngine::prepare(&[StageSpec::memoryless(Curve::identity())], 48_000.0).unwrap();
    assert_eq!(identity.nominal_makeup_gain(), 1.0);

    let reduction = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.4),
        ControlPoint::curve(1.0, 0.5),
    ])
    .unwrap();
    let reduction = CurveEngine::prepare(&[StageSpec::memoryless(reduction)], 48_000.0).unwrap();
    assert!(reduction.nominal_makeup_gain() > 1.0);

    let lift = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let lift = CurveEngine::prepare(&[StageSpec::memoryless(lift)], 48_000.0).unwrap();
    assert!(lift.nominal_makeup_gain() < 1.0);

    let silence = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(1.0, 0.0),
    ])
    .unwrap();
    let silence = CurveEngine::prepare(&[StageSpec::memoryless(silence)], 48_000.0).unwrap();
    assert_eq!(silence.nominal_makeup_gain(), MAX_MAKEUP_GAIN);
}

#[test]
fn exact_y_alignment_is_flat() {
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.25, 0.6),
        ControlPoint::curve(0.75, 0.6),
        ControlPoint::curve(1.0, 1.0),
    ])
    .expect("flat curve is valid");
    let table = CurveTable::prepare(&curve);
    for i in 0..=100 {
        let x = 0.25 + 0.5 * i as f32 / 100.0;
        assert!((table.evaluate(x) - 0.6).abs() < 2.0e-6);
    }
}

#[test]
fn exact_x_alignment_is_right_continuous() {
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.2),
        ControlPoint::curve(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .expect("a snapped discontinuity is valid");
    let table = CurveTable::prepare(&curve);
    assert!(table.evaluate(0.5 - 1.0e-5) < 0.21);
    assert_eq!(table.evaluate(0.5), 0.8);
    assert!(table.evaluate(0.5 + 1.0e-5) > 0.79);
    assert!(table.integral(0.5).is_finite());
}

#[test]
fn representably_narrow_and_stacked_segments_remain_finite() {
    let smallest_positive = f32::from_bits(1);
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(smallest_positive, 1.0),
        ControlPoint::curve(smallest_positive, 0.2),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let table = CurveTable::prepare(&curve);
    for x in [0.0, smallest_positive, 0.5, 1.0] {
        assert!(table.evaluate(x).is_finite());
        assert!(table.integral(x).is_finite());
    }
    assert_eq!(table.evaluate(smallest_positive), 0.2);
}

#[test]
fn reversed_points_and_crossed_handles_are_refused() {
    assert_eq!(
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::curve(0.8, 0.5),
            ControlPoint::curve(0.7, 0.6),
            ControlPoint::curve(1.0, 1.0),
        ]),
        Err(CurveError::ReversedX)
    );
    assert_eq!(
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::handles(
                0.5,
                0.5,
                Handle { dx: -0.2, dy: 0.0 },
                Handle { dx: -0.1, dy: 0.0 },
            ),
            ControlPoint::curve(1.0, 1.0),
        ]),
        Err(CurveError::HandleCrossesSegment)
    );
}

#[test]
fn authored_count_bounds_are_enforced() {
    let too_many_points = (0..=MAX_POINTS)
        .map(|index| {
            let value = index as f32 / MAX_POINTS as f32;
            ControlPoint::curve(value, value)
        })
        .collect();
    assert_eq!(Curve::new(too_many_points), Err(CurveError::TooManyPoints));

    let spec = memoryless(Curve::identity(), AntialiasMode::Adaa);
    let too_many_stages = vec![spec; MAX_STAGES + 1];
    assert!(CurveEngine::prepare(&too_many_stages, 48_000.0).is_err());
}

#[test]
fn production_antialiasing_is_derived_not_persisted() {
    let continuous = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let discontinuous = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.2),
        ControlPoint::curve(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let smooth = CurveEngine::prepare(&[StageSpec::memoryless(continuous)], 48_000.0).unwrap();
    let jump = CurveEngine::prepare(&[StageSpec::memoryless(discontinuous)], 48_000.0).unwrap();
    assert_eq!(
        smooth.stages()[0].antialias_mode(),
        Some(AntialiasMode::Direct)
    );
    assert_eq!(jump.stages()[0].antialias_mode(), Some(AntialiasMode::Adaa));
}

#[test]
fn memoryless_curves_are_odd_and_silence_preserving() {
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.35, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let mut engine =
        CurveEngine::prepare(&[memoryless(curve, AntialiasMode::Direct)], 48_000.0).unwrap();
    assert_eq!(engine.process([0.0, -0.0], 1.0), [0.0, 0.0]);
    let output = engine.process([0.2, -0.2], 1.0);
    assert!((output[0] + output[1]).abs() < 1.0e-7);
}

#[test]
fn detector_ballistics_match_the_declared_one_pole_gain_law() {
    let compressor = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.5, 0.5),
        ControlPoint::linear(1.0, 0.5),
    ])
    .unwrap();
    let mut engine = CurveEngine::prepare(
        &[stage(
            compressor,
            StageMode::Detector {
                attack_ms: 1.0,
                release_ms: 10.0,
            },
        )],
        48_000.0,
    )
    .unwrap();
    for _ in 0..48 {
        engine.process([1.0, 1.0], 1.0);
    }
    let attacked = engine.stages()[0].detector_gain().unwrap();
    let expected_attack = 0.5 + 0.5 * (-1.0f32).exp();
    assert!((attacked - expected_attack).abs() < 2.0e-6);

    for _ in 0..480 {
        engine.process([0.0, 0.0], 1.0);
    }
    let released = engine.stages()[0].detector_gain().unwrap();
    let expected_release = 1.0 + (expected_attack - 1.0) * (-1.0f32).exp();
    assert!((released - expected_release).abs() < 2.0e-6);
}

#[test]
fn detector_linking_preserves_the_stereo_ratio() {
    let compressor = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.5),
        ControlPoint::curve(1.0, 0.7),
    ])
    .unwrap();
    let mut engine = CurveEngine::prepare(
        &[stage(
            compressor,
            StageMode::Detector {
                attack_ms: 0.1,
                release_ms: 100.0,
            },
        )],
        48_000.0,
    )
    .unwrap();
    let mut output = [0.0; 2];
    for _ in 0..2_000 {
        output = engine.process([0.8, 0.4], 1.0);
    }
    assert!(output[0] < 0.8);
    assert!((output[0] / output[1] - 2.0).abs() < 1.0e-5);
}

#[test]
fn replacement_carries_only_the_unchanged_prefix() {
    let detector = stage(
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::curve(0.5, 0.5),
            ControlPoint::curve(1.0, 0.7),
        ])
        .unwrap(),
        StageMode::Detector {
            attack_ms: 10.0,
            release_ms: 100.0,
        },
    );
    let shaper = memoryless(
        Curve::new(vec![
            ControlPoint::curve(0.0, 0.0),
            ControlPoint::curve(0.5, 0.8),
            ControlPoint::curve(1.0, 1.0),
        ])
        .unwrap(),
        AntialiasMode::Adaa,
    );
    let specs = [detector.clone(), shaper.clone()];
    let mut old = CurveEngine::prepare(&specs, 48_000.0).unwrap();
    for _ in 0..1_000 {
        old.process([0.8, -0.4], 1.0);
    }
    let mut identical = CurveEngine::prepare(&specs, 48_000.0).unwrap();
    assert_eq!(identical.carry_compatible_prefix_history_from(&old), 2);
    assert_eq!(
        identical.process([0.7, -0.3], 1.0),
        old.process([0.7, -0.3], 1.0)
    );

    let changed_second = [
        detector.clone(),
        memoryless(Curve::identity(), AntialiasMode::Adaa),
    ];
    let mut suffix = CurveEngine::prepare(&changed_second, 48_000.0).unwrap();
    assert_eq!(suffix.carry_compatible_prefix_history_from(&old), 1);

    let changed_first = [memoryless(Curve::identity(), AntialiasMode::Adaa), shaper];
    let mut all = CurveEngine::prepare(&changed_first, 48_000.0).unwrap();
    assert_eq!(all.carry_compatible_prefix_history_from(&old), 0);
}

#[test]
fn stage_order_changes_the_sound() {
    let boost = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let limit = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.5, 0.5),
        ControlPoint::linear(1.0, 0.55),
    ])
    .unwrap();
    let direct = |curve| memoryless(curve, AntialiasMode::Direct);
    let mut a =
        CurveEngine::prepare(&[direct(boost.clone()), direct(limit.clone())], 48_000.0).unwrap();
    let mut b = CurveEngine::prepare(&[direct(limit), direct(boost)], 48_000.0).unwrap();
    assert_ne!(a.process([0.6, 0.6], 1.0), b.process([0.6, 0.6], 1.0));
}

#[test]
fn reset_leaves_exact_silence_and_no_tail() {
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::curve(0.5, 0.2),
        ControlPoint::curve(0.5, 0.8),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let mut engine = CurveEngine::prepare(&[StageSpec::memoryless(curve)], 48_000.0).unwrap();
    for _ in 0..100 {
        engine.process([0.8, -0.4], 1.0);
    }
    engine.reset();
    for _ in 0..100 {
        assert_eq!(engine.process([0.0, 0.0], 1.0), [0.0, 0.0]);
    }
}

#[test]
fn mix_zero_is_finite_dry_to_the_bit_and_parks() {
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.2, 0.9),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let mut engine =
        CurveEngine::prepare(&[memoryless(curve, AntialiasMode::Adaa)], 48_000.0).unwrap();
    let input = [0.1234567, -0.7654321];
    assert_eq!(engine.process(input, 0.0), input);
    assert!(engine.is_parked());
    assert_eq!(engine.process([f32::NAN, f32::INFINITY], 0.0), [0.0, 0.0]);
}

#[test]
fn parked_detector_returns_from_silence_not_stale_gain() {
    let gate = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.6, 0.0),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    let spec = stage(
        gate,
        StageMode::Detector {
            attack_ms: 0.1,
            release_ms: 5_000.0,
        },
    );
    let mut parked = CurveEngine::prepare(std::slice::from_ref(&spec), 48_000.0).unwrap();
    let mut silent = CurveEngine::prepare(&[spec], 48_000.0).unwrap();
    for _ in 0..1_000 {
        parked.process([1.0, 1.0], 1.0);
    }
    assert_eq!(parked.process([0.0, 0.0], 0.0), [0.0, 0.0]);
    // Mix-zero settles to the same destination as an unbroken, fully settled passage of silence.
    assert_eq!(
        parked.process([0.8, 0.8], 1.0),
        silent.process([0.8, 0.8], 1.0)
    );
}

#[test]
fn every_public_sample_rate_corner_stays_finite() {
    let curve = Curve::new(vec![
        ControlPoint::curve(0.0, 0.0),
        ControlPoint::linear(0.5, 0.1),
        ControlPoint::curve(1.0, 1.0),
    ])
    .unwrap();
    for rate in [1_000.0, 48_000.0, 768_000.0] {
        let mut engine = CurveEngine::prepare(
            &[stage(
                curve.clone(),
                StageMode::Detector {
                    attack_ms: 0.1,
                    release_ms: 5_000.0,
                },
            )],
            rate,
        )
        .unwrap();
        for input in [
            [0.0, 0.0],
            [1.0, -1.0],
            [f32::MAX, f32::MIN],
            [f32::NAN, f32::INFINITY],
        ] {
            let output = engine.process(input, 1.0);
            assert!(output.into_iter().all(f32::is_finite));
        }
    }
}
