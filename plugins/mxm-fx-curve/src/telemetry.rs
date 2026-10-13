//! Lock-free, lossy editor telemetry. It is observation only and never feeds DSP decisions.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub struct Telemetry {
    connected: AtomicBool,
    levels: [[AtomicU32; 2]; mxm_fx_curve_dsp::MAX_STAGES],
    /// The stereo input and output meters, one `PeakMeter` per channel: the collection's one meter
    /// policy, latched also by the DSP's own per-channel clip flags (`mxm_telemetry`).
    input: [mxm_telemetry::PeakMeter; 2],
    output: [mxm_telemetry::PeakMeter; 2],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            connected: AtomicBool::new(false),
            levels: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            input: [
                mxm_telemetry::PeakMeter::new(),
                mxm_telemetry::PeakMeter::new(),
            ],
            output: [
                mxm_telemetry::PeakMeter::new(),
                mxm_telemetry::PeakMeter::new(),
            ],
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
            // The levels are transient and go; a clip latch stays until the user clicks its meter
            // (`mxm_telemetry::PeakMeter`), closing the editor in between included.
            for meter in self.input.iter().chain(&self.output) {
                let _ = meter.take();
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
        self.observe_stereo(&self.input, peaks, clipped);
    }

    pub fn observe_output(&self, peaks: [f32; 2], clipped: [bool; 2]) {
        self.observe_stereo(&self.output, peaks, clipped);
    }

    fn observe_stereo(
        &self,
        meters: &[mxm_telemetry::PeakMeter; 2],
        peaks: [f32; 2],
        clipped: [bool; 2],
    ) {
        if !self.connected() {
            return;
        }
        for channel in 0..2 {
            meters[channel].publish(peaks[channel]);
            if clipped[channel] {
                meters[channel].latch_clip();
            }
        }
    }

    pub fn take(&self, stage: usize) -> Option<(f32, f32)> {
        let pair = self.levels.get(stage)?;
        let input = f32::from_bits(pair[0].swap(0, Ordering::AcqRel));
        let output = f32::from_bits(pair[1].swap(0, Ordering::AcqRel));
        (input > 0.0 || output > 0.0).then_some((input, output))
    }

    /// The input's level per channel since the editor last read it, at most full scale: the rails
    /// end there.
    pub fn take_input(&self) -> [f32; 2] {
        self.input.each_ref().map(|meter| meter.take().min(1.0))
    }

    /// The output's level per channel, likewise.
    pub fn take_output(&self) -> [f32; 2] {
        self.output.each_ref().map(|meter| meter.take().min(1.0))
    }

    pub fn input_clipped(&self) -> [bool; 2] {
        self.input.each_ref().map(mxm_telemetry::PeakMeter::clipped)
    }

    pub fn output_clipped(&self) -> [bool; 2] {
        self.output
            .each_ref()
            .map(mxm_telemetry::PeakMeter::clipped)
    }

    pub fn clear_input_clip(&self, channel: usize) {
        if let Some(meter) = self.input.get(channel) {
            meter.clear_clip();
        }
    }

    pub fn clear_output_clip(&self, channel: usize) {
        if let Some(meter) = self.output.get(channel) {
            meter.clear_clip();
        }
    }
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

    /// **Closing the editor keeps a clip latched**: the levels go, the latch waits for the user's
    /// click on its meter.
    #[test]
    fn closing_and_reopening_the_editor_keeps_a_clip_latched_until_it_is_clicked() {
        let telemetry = Telemetry::default();
        telemetry.connect(true);
        telemetry.observe_input([0.5, 0.2], [true, false]);
        telemetry.observe_output([0.3, 0.4], [false, true]);
        telemetry.connect(false);
        telemetry.connect(true);
        assert_eq!(telemetry.take_input(), [0.0, 0.0], "the levels are gone");
        assert_eq!(
            telemetry.input_clipped(),
            [true, false],
            "the input latch stays"
        );
        assert_eq!(
            telemetry.output_clipped(),
            [false, true],
            "the output latch stays"
        );
        telemetry.clear_input_clip(0);
        assert_eq!(
            telemetry.input_clipped(),
            [false, false],
            "a click clears it"
        );
    }
}
