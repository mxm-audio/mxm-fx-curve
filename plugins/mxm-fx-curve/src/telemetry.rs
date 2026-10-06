//! Lock-free, lossy editor telemetry. It is observation only and never feeds DSP decisions.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub struct Telemetry {
    connected: AtomicBool,
    levels: [[AtomicU32; 2]; mxm_fx_curve_dsp::MAX_STAGES],
    input: [AtomicU32; 2],
    output: [AtomicU32; 2],
    input_clipped: [AtomicBool; 2],
    output_clipped: [AtomicBool; 2],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            connected: AtomicBool::new(false),
            levels: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            input: std::array::from_fn(|_| AtomicU32::new(0)),
            output: std::array::from_fn(|_| AtomicU32::new(0)),
            input_clipped: std::array::from_fn(|_| AtomicBool::new(false)),
            output_clipped: std::array::from_fn(|_| AtomicBool::new(false)),
        }
    }
}

impl Telemetry {
    pub fn connect(&self, connected: bool) {
        self.connected.store(connected, Ordering::Release);
        if !connected {
            for pair in &self.levels {
                pair[0].store(0, Ordering::Relaxed);
                pair[1].store(0, Ordering::Relaxed);
            }
            for peak in self.input.iter().chain(&self.output) {
                peak.store(0, Ordering::Relaxed);
            }
            for clipped in self.input_clipped.iter().chain(&self.output_clipped) {
                clipped.store(false, Ordering::Relaxed);
            }
        }
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    pub fn observe(&self, levels: &[[f32; 2]; mxm_fx_curve_dsp::MAX_STAGES], count: usize) {
        if !self.connected() {
            return;
        }
        for (stage, pair) in levels.iter().enumerate().take(count) {
            peak_store(&self.levels[stage][0], pair[0]);
            peak_store(&self.levels[stage][1], pair[1]);
        }
    }

    pub fn observe_input(&self, peaks: [f32; 2], clipped: [bool; 2]) {
        self.observe_stereo(&self.input, &self.input_clipped, peaks, clipped);
    }

    pub fn observe_output(&self, peaks: [f32; 2], clipped: [bool; 2]) {
        self.observe_stereo(&self.output, &self.output_clipped, peaks, clipped);
    }

    fn observe_stereo(
        &self,
        targets: &[AtomicU32; 2],
        clip_targets: &[AtomicBool; 2],
        peaks: [f32; 2],
        clipped: [bool; 2],
    ) {
        if !self.connected() {
            return;
        }
        for channel in 0..2 {
            peak_store(&targets[channel], peaks[channel]);
            if clipped[channel] {
                clip_targets[channel].store(true, Ordering::Relaxed);
            }
        }
    }

    pub fn take(&self, stage: usize) -> Option<(f32, f32)> {
        let pair = self.levels.get(stage)?;
        let input = f32::from_bits(pair[0].swap(0, Ordering::AcqRel));
        let output = f32::from_bits(pair[1].swap(0, Ordering::AcqRel));
        (input > 0.0 || output > 0.0).then_some((input, output))
    }

    pub fn take_input(&self) -> [f32; 2] {
        take_stereo(&self.input)
    }

    pub fn take_output(&self) -> [f32; 2] {
        take_stereo(&self.output)
    }

    pub fn input_clipped(&self) -> [bool; 2] {
        clipped_stereo(&self.input_clipped)
    }

    pub fn output_clipped(&self) -> [bool; 2] {
        clipped_stereo(&self.output_clipped)
    }

    pub fn clear_input_clip(&self, channel: usize) {
        if let Some(clipped) = self.input_clipped.get(channel) {
            clipped.store(false, Ordering::Relaxed);
        }
    }

    pub fn clear_output_clip(&self, channel: usize) {
        if let Some(clipped) = self.output_clipped.get(channel) {
            clipped.store(false, Ordering::Relaxed);
        }
    }
}

fn take_stereo(targets: &[AtomicU32; 2]) -> [f32; 2] {
    targets
        .each_ref()
        .map(|peak| f32::from_bits(peak.swap(0, Ordering::AcqRel)))
}

fn clipped_stereo(targets: &[AtomicBool; 2]) -> [bool; 2] {
    targets.each_ref().map(|clip| clip.load(Ordering::Relaxed))
}

fn peak_store(target: &AtomicU32, sample: f32) {
    let value = sample.clamp(0.0, 1.0).to_bits();
    let mut previous = target.load(Ordering::Relaxed);
    while value > previous {
        match target.compare_exchange_weak(previous, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => previous = observed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_input_and_output_peaks_combine_reset_and_latch_per_channel() {
        let telemetry = Telemetry::default();
        telemetry.connect(true);
        telemetry.observe_input([0.25, 1.1], [false, true]);
        telemetry.observe_input([0.75, 0.2], [false, false]);
        telemetry.observe_output([1.1, 0.4], [true, false]);
        telemetry.observe_output([0.2, 0.8], [false, false]);

        assert_eq!(telemetry.take_input(), [0.75, 1.0]);
        assert_eq!(telemetry.take_output(), [1.0, 0.8]);
        assert_eq!(telemetry.take_input(), [0.0, 0.0]);
        assert_eq!(telemetry.take_output(), [0.0, 0.0]);
        assert_eq!(telemetry.input_clipped(), [false, true]);
        assert_eq!(telemetry.output_clipped(), [true, false]);

        telemetry.clear_input_clip(1);
        telemetry.clear_output_clip(0);
        assert_eq!(telemetry.input_clipped(), [false, false]);
        assert_eq!(telemetry.output_clipped(), [false, false]);
    }
}
