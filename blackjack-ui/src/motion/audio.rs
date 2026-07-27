//! The WebAudio mixer: three synthesized table sounds, no assets.
//!
//! Every cue is one voice — a shared white-noise buffer through a
//! bandpass filter into a tiny gain envelope — built from the pure
//! [`SoundSpec`](super::sound::SoundSpec)s in [`sound`](super::sound).
//!
//! # Autoplay policy
//!
//! Browsers refuse to start audio without a user gesture, so the
//! [`AudioContext`] is created and resumed lazily by [`Mixer::unlock`].
//! [`Motion::new`](super::Motion::new) installs window `pointerdown`
//! and `keydown` listeners that call it, and the input layer (#9) can
//! call [`Motion::unlock_audio`](super::Motion::unlock_audio) from its
//! own first gesture as well; until one of those fires, cues are
//! silently dropped. Wasm-only: the host build animates without sound.

use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{AudioBuffer, AudioContext, AudioContextState, BiquadFilterType};

use super::sound::SoundCue;

/// Length of the shared noise buffer in seconds.
const NOISE_SECONDS: f64 = 1.0;

/// The lazy WebAudio mixer.
#[derive(Default)]
pub struct Mixer {
    ctx: RefCell<Option<AudioContext>>,
    noise: RefCell<Option<AudioBuffer>>,
}

impl Mixer {
    /// A mixer with no context yet; silent until [`Mixer::unlock`].
    pub fn new() -> Mixer {
        Mixer::default()
    }

    /// Create (or resume) the audio context. Must be called from a user
    /// gesture the first time; harmless any time after.
    pub fn unlock(&self) {
        let mut slot = self.ctx.borrow_mut();
        if slot.is_none() {
            *slot = AudioContext::new().ok();
        }
        if let Some(ctx) = slot.as_ref()
            && ctx.state() == AudioContextState::Suspended
        {
            let _ = ctx.resume();
        }
    }

    /// Play one cue now. Dropped silently until the context is unlocked
    /// and running — sound is garnish here, never load-bearing.
    pub fn play(&self, cue: SoundCue) {
        let ctx = self.ctx.borrow();
        let Some(ctx) = ctx.as_ref() else {
            return;
        };
        if ctx.state() != AudioContextState::Running {
            return;
        }
        let Some(noise) = self.noise_buffer(ctx) else {
            return;
        };
        let spec = cue.spec();
        let now = ctx.current_time();
        // noise -> bandpass -> envelope -> speakers
        let (Ok(source), Ok(filter), Ok(gain)) = (
            ctx.create_buffer_source(),
            ctx.create_biquad_filter(),
            ctx.create_gain(),
        ) else {
            return;
        };
        source.set_buffer(Some(&noise));
        filter.set_type(BiquadFilterType::Bandpass);
        filter.frequency().set_value(spec.center_hz as f32);
        filter.q().set_value(spec.q as f32);
        let envelope = gain.gain();
        let _ = envelope.set_value_at_time(0.0, now);
        let _ = envelope.linear_ramp_to_value_at_time(spec.peak_gain as f32, now + spec.attack_s);
        let _ = envelope.exponential_ramp_to_value_at_time(0.0001, now + spec.duration_s);
        let ok = source.connect_with_audio_node(&filter).is_ok()
            && filter.connect_with_audio_node(&gain).is_ok()
            && gain.connect_with_audio_node(&ctx.destination()).is_ok();
        if !ok {
            return;
        }
        // The envelope is silent past `duration_s`; the source ends by
        // itself at the one-second buffer boundary.
        let _ = source.start();
    }

    /// The shared one-second noise buffer, built on first use.
    fn noise_buffer(&self, ctx: &AudioContext) -> Option<AudioBuffer> {
        let mut slot = self.noise.borrow_mut();
        if let Some(buffer) = slot.as_ref() {
            return Some(buffer.clone());
        }
        let sample_rate = ctx.sample_rate();
        let length = (f64::from(sample_rate) * NOISE_SECONDS) as u32;
        let buffer = ctx.create_buffer(1, length, sample_rate).ok()?;
        // A tiny deterministic xorshift; rand would be dead weight here.
        let mut state: u32 = 0x9e37_79b9;
        let mut samples = vec![0f32; length as usize];
        for sample in &mut samples {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *sample = (state as f32 / u32::MAX as f32) * 2.0 - 1.0;
        }
        buffer.copy_to_channel(&mut samples, 0).ok()?;
        *slot = Some(buffer.clone());
        Some(buffer)
    }
}

/// Install window `pointerdown`/`keydown` listeners that unlock audio
/// on the first user gesture. The closures are intentionally leaked:
/// they live for the life of the app.
pub fn install_unlock_handlers(motion: &super::Motion) {
    let Some(window) = web_sys::window() else {
        return;
    };
    for kind in ["pointerdown", "keydown"] {
        let motion = motion.clone();
        let closure = Closure::<dyn FnMut()>::new(move || motion.unlock_audio());
        let _ = window.add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref());
        closure.forget();
    }
}
