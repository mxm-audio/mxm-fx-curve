//! `mxm-fx-curve` — a serial transfer-curve effect.

macro_rules! plugin_name {
    () => {
        "mxm-fx-curve"
    };
}

pub const NAME: &str = plugin_name!();
pub const CLAP_ID: &str = concat!("dk.mxm.", plugin_name!());

mod canvas;
pub mod editor;
pub mod model;
pub mod params;
pub mod preset;
pub mod preset_designs;
mod publication;
mod telemetry;

use nice_plug::prelude::*;
use params::MxmFxCurveParams;
use publication::AudioConsumer;
use std::sync::Arc;

pub struct MxmFxCurve {
    pub params: Arc<MxmFxCurveParams>,
    consumer: AudioConsumer,
    telemetry: Arc<telemetry::Telemetry>,
    makeup_ramp: MakeupRamp,
    sample_rate: f32,
    channels: usize,
    activated: bool,
}

#[derive(Default)]
struct MakeupRamp {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl MakeupRamp {
    fn reset(&mut self, enabled: bool) {
        self.current = if enabled { 1.0 } else { 0.0 };
        self.target = self.current;
        self.step = 0.0;
        self.remaining = 0;
    }

    fn next(&mut self, enabled: bool, sample_rate: f32) -> f32 {
        let target = if enabled { 1.0 } else { 0.0 };
        if target != self.target {
            self.target = target;
            self.remaining = (sample_rate * 0.020).round().max(1.0) as u32;
            self.step = (target - self.current) / self.remaining as f32;
        }
        if self.remaining > 0 {
            self.current += self.step;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            }
        }
        self.current
    }
}

struct BlockTelemetry {
    stage_levels: [[f32; 2]; mxm_fx_curve_dsp::MAX_STAGES],
    stage_count: usize,
    input_peaks: [f32; 2],
    input_clipped: [bool; 2],
    output_peaks: [f32; 2],
    output_clipped: [bool; 2],
}

impl Default for BlockTelemetry {
    fn default() -> Self {
        Self {
            stage_levels: [[0.0; 2]; mxm_fx_curve_dsp::MAX_STAGES],
            stage_count: 0,
            input_peaks: [0.0; 2],
            input_clipped: [false; 2],
            output_peaks: [0.0; 2],
            output_clipped: [false; 2],
        }
    }
}

impl Default for MxmFxCurve {
    fn default() -> Self {
        Self {
            params: Arc::new(MxmFxCurveParams::default()),
            consumer: AudioConsumer::default(),
            telemetry: Arc::new(telemetry::Telemetry::default()),
            makeup_ramp: MakeupRamp::default(),
            sample_rate: 48_000.0,
            channels: 2,
            activated: false,
        }
    }
}

impl MxmFxCurve {
    fn prepare(&mut self, sample_rate: f32, channels: usize) -> bool {
        let sample_rate = valid_sample_rate(sample_rate);
        let can_transition = self.activated && self.sample_rate.to_bits() == sample_rate.to_bits();
        self.channels = channels.clamp(1, 2);
        if !can_transition {
            self.makeup_ramp.reset(self.params.auto_makeup.value());
        }
        if self.params.curves.rejected() {
            return false;
        }
        let bank = self.params.curves.bank();
        if can_transition {
            // A host-state restore reactivates the plugin while audio is stopped. Keep its prepared
            // candidate and enter the same transition used by an editor mutation; do not rebuild it
            // and silently replace live history.
            self.consumer.accept_update(&bank);
            return true;
        }
        if self.params.curves.prepare_for_rate(sample_rate).is_err() {
            return false;
        }
        let activated = self.consumer.activate(&bank, sample_rate);
        if activated {
            self.sample_rate = sample_rate;
            self.activated = true;
        }
        activated
    }

    pub fn prepare_for_test(&mut self, sample_rate: f32, channels: usize) -> bool {
        self.prepare(sample_rate, channels)
    }

    pub fn process_block_for_test(&mut self, channels: &mut [&mut [f32]]) -> ProcessStatus {
        self.process_block(channels)
    }

    fn process_frame(
        &mut self,
        bank: &publication::EngineBank,
        dry: [f32; 2],
        mut telemetry: Option<&mut BlockTelemetry>,
    ) -> [f32; 2] {
        let raw_mix = self.params.mix.smoothed.next();
        let mix = if raw_mix.is_finite() {
            raw_mix.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let raw_input_gain = self.params.input_gain.smoothed.next();
        let input_gain = if raw_input_gain.is_finite() {
            raw_input_gain.clamp(
                1.0 / mxm_fx_curve_dsp::MAX_MAKEUP_GAIN,
                mxm_fx_curve_dsp::MAX_MAKEUP_GAIN,
            )
        } else {
            1.0
        };
        let makeup_amount = self
            .makeup_ramp
            .next(self.params.auto_makeup.value(), self.sample_rate);
        let wet_input = apply_input_gain(dry, input_gain);
        if let Some(block) = telemetry.as_mut() {
            for (channel, sample) in wet_input.iter().enumerate() {
                block.input_peaks[channel] = block.input_peaks[channel].max(sample.abs());
                block.input_clipped[channel] |= sample.abs() >= 1.0;
            }
        }
        if mix == 0.0 {
            self.consumer.settle_and_finish(bank);
            return dry;
        }
        let wet = if let Some(block) = telemetry {
            let mut frame_levels = [[0.0; 2]; mxm_fx_curve_dsp::MAX_STAGES];
            let (wet, count) = self.consumer.process_with_stage_levels(
                bank,
                wet_input,
                makeup_amount,
                &mut frame_levels,
            );
            block.stage_count = block.stage_count.max(count);
            for (block, frame) in block.stage_levels.iter_mut().zip(frame_levels).take(count) {
                block[0] = block[0].max(frame[0]);
                block[1] = block[1].max(frame[1]);
            }
            wet
        } else {
            self.consumer.process(bank, wet_input, makeup_amount)
        };
        [
            dry[0] + (wet[0] - dry[0]) * mix,
            dry[1] + (wet[1] - dry[1]) * mix,
        ]
    }

    fn process_block(&mut self, channels: &mut [&mut [f32]]) -> ProcessStatus {
        let Some(first) = channels.first() else {
            return ProcessStatus::Normal;
        };
        let samples = first.len();
        let stereo = self.channels == 2 && channels.len() >= 2;
        let bank = self.params.curves.bank();
        self.consumer.accept_update(&bank);

        let mut silent = true;
        for channel in channels.iter_mut().take(self.channels) {
            for sample in channel.iter_mut().take(samples) {
                if !sample.is_finite() {
                    *sample = 0.0;
                }
                silent &= *sample == 0.0;
            }
        }
        if silent {
            for _ in 0..samples {
                let _ = self.params.mix.smoothed.next();
                let _ = self.params.input_gain.smoothed.next();
                let _ = self
                    .makeup_ramp
                    .next(self.params.auto_makeup.value(), self.sample_rate);
            }
            self.consumer.settle_and_finish(&bank);
            if !stereo && channels.len() > 1 {
                channels[1][..samples].fill(0.0);
            }
            return ProcessStatus::Normal;
        }

        let collect_telemetry = self.telemetry.connected();
        let mut telemetry = BlockTelemetry::default();
        if stereo {
            let (left, rest) = channels.split_at_mut(1);
            for (left, right) in left[0][..samples]
                .iter_mut()
                .zip(rest[0][..samples].iter_mut())
            {
                let block = collect_telemetry.then_some(&mut telemetry);
                let output = self.process_frame(&bank, [*left, *right], block);
                if collect_telemetry {
                    for (channel, sample) in output.iter().enumerate() {
                        telemetry.output_peaks[channel] =
                            telemetry.output_peaks[channel].max(sample.abs());
                        telemetry.output_clipped[channel] |= sample.abs() >= 1.0;
                    }
                }
                *left = output[0];
                *right = output[1];
            }
        } else {
            for sample in &mut channels[0][..samples] {
                let block = collect_telemetry.then_some(&mut telemetry);
                let output = self.process_frame(&bank, [*sample; 2], block);
                if collect_telemetry {
                    for (channel, output_sample) in output.iter().enumerate() {
                        telemetry.output_peaks[channel] =
                            telemetry.output_peaks[channel].max(output_sample.abs());
                        telemetry.output_clipped[channel] |= output_sample.abs() >= 1.0;
                    }
                }
                *sample = output[0];
            }
        }
        if collect_telemetry {
            self.telemetry
                .observe(&telemetry.stage_levels, telemetry.stage_count);
            self.telemetry
                .observe_input(telemetry.input_peaks, telemetry.input_clipped);
            self.telemetry
                .observe_output(telemetry.output_peaks, telemetry.output_clipped);
        }
        ProcessStatus::Normal
    }
}

impl Plugin for MxmFxCurve {
    const NAME: &'static str = NAME;
    const VENDOR: &'static str = "mxm";
    const URL: &'static str = "https://mxm.dk";
    const EMAIL: &'static str = "plugins@mxm.dk";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = editor::MxmFxCurveEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        editor::create(self.params.clone(), self.telemetry.clone())
    }

    fn filter_state(state: &mut PluginState) {
        let valid = state.fields.get("curves").is_none_or(|serialized| {
            nice_plug::params::persist::deserialize_field::<model::CurveStackState>(serialized)
                .ok()
                .is_some_and(|model| model.validate().is_ok())
        });
        if !valid {
            state.params.clear();
            state.fields.clear();
        }
    }

    fn activate(
        &mut self,
        layout: &AudioIOLayout,
        config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.prepare(
            config.sample_rate,
            layout
                .main_input_channels
                .map_or(2, |channels| channels.get() as usize),
        )
    }

    fn reset(&mut self) {
        let bank = self.params.curves.bank();
        self.consumer.reset(&bank);
        self.makeup_ramp.reset(self.params.auto_makeup.value());
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.process_block(buffer.as_slice())
    }
}

impl ClapPlugin for MxmFxCurve {
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Draw curves that shape the sound: saturation, clipping and dynamics");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::MultiEffects,
        ClapFeature::Compressor,
        ClapFeature::Distortion,
        ClapFeature::Mono,
        ClapFeature::Stereo,
    ];
}

nice_export_clap!(MxmFxCurve);

fn apply_input_gain(frame: [f32; 2], gain: f32) -> [f32; 2] {
    if gain == 1.0 {
        return frame;
    }
    frame.map(|sample| {
        (f64::from(sample) * f64::from(gain)).clamp(
            -(mxm_fx_curve_dsp::MAX_OUTPUT as f64),
            mxm_fx_curve_dsp::MAX_OUTPUT as f64,
        ) as f32
    })
}

fn valid_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() {
        sample_rate.clamp(1_000.0, 768_000.0)
    } else {
        48_000.0
    }
}

#[cfg(test)]
mod tests;

/// What a player reads — on hover in the editor, and in a host's plugin browser — speaks to the
/// player about the sound, never about the machine or the code (`mxm_plugin_test::hover_text`).
#[cfg(test)]
mod speaks_to_the_player {
    #[test]
    fn hover_text() {
        mxm_plugin_test::hover_text::speaks_to_the_player(env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn host_description() {
        mxm_plugin_test::hover_text::host_description_speaks_to_the_player(env!(
            "CARGO_MANIFEST_DIR"
        ));
    }
}
