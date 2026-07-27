//! Sound cues: sparse, synthesized, at the threshold of notice.
//!
//! Three sounds only — a card's slide, a chip's clink, the shuffle's
//! riffle — every one a filtered noise burst with a tiny envelope,
//! synthesized live by the WebAudio mixer in [`audio`](super::audio).
//! No samples, no music, no ambience, and deliberately no volume
//! control: if a volume slider ever feels needed, the sounds are wrong.
//! Amplitudes here are capped low and tested to stay that way.
//!
//! This module is pure data (which cue, what filter, what envelope) so
//! the mapping tests on the host; only the mixer touches WebAudio.

/// One synthesized table sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundCue {
    /// A card leaving the shoe and hitting the felt: a soft noise swish.
    CardSlide,
    /// Chips settling: a short, bright filtered tick.
    ChipClink,
    /// The shuffle: a longer riffle of noise.
    Riffle,
}

impl SoundCue {
    /// Whether the cue plays when its step begins rather than when it
    /// completes. The riffle accompanies the whole shuffle; slides and
    /// clinks mark the moment the object lands.
    pub fn at_step_start(self) -> bool {
        matches!(self, SoundCue::Riffle)
    }

    /// The cue's synthesis parameters.
    pub fn spec(self) -> SoundSpec {
        match self {
            SoundCue::CardSlide => SoundSpec {
                center_hz: 1900.0,
                q: 0.9,
                peak_gain: 0.055,
                attack_s: 0.012,
                duration_s: 0.10,
            },
            SoundCue::ChipClink => SoundSpec {
                center_hz: 4300.0,
                q: 7.0,
                peak_gain: 0.05,
                attack_s: 0.004,
                duration_s: 0.045,
            },
            SoundCue::Riffle => SoundSpec {
                center_hz: 2400.0,
                q: 0.7,
                peak_gain: 0.04,
                attack_s: 0.05,
                duration_s: 0.45,
            },
        }
    }
}

/// Parameters for one noise-burst voice: white noise through a bandpass
/// filter into a gain envelope (linear attack, exponential decay).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundSpec {
    /// Bandpass center frequency.
    pub center_hz: f64,
    /// Bandpass resonance.
    pub q: f64,
    /// Envelope peak. Kept far below full scale — these sounds sit at
    /// the threshold of notice by design.
    pub peak_gain: f64,
    /// Linear attack time in seconds.
    pub attack_s: f64,
    /// Total voice duration in seconds.
    pub duration_s: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SoundCue; 3] = [SoundCue::CardSlide, SoundCue::ChipClink, SoundCue::Riffle];

    #[test]
    fn every_cue_stays_at_the_threshold_of_notice() {
        for cue in ALL {
            let spec = cue.spec();
            assert!(spec.peak_gain <= 0.06, "{cue:?} is too loud");
            assert!(spec.duration_s <= 0.5, "{cue:?} overstays");
            assert!(spec.attack_s < spec.duration_s, "{cue:?} envelope");
            assert!(spec.center_hz > 0.0 && spec.q > 0.0);
        }
    }

    #[test]
    fn only_the_riffle_leads_its_step() {
        assert!(SoundCue::Riffle.at_step_start());
        assert!(!SoundCue::CardSlide.at_step_start());
        assert!(!SoundCue::ChipClink.at_step_start());
    }

    #[test]
    fn the_clink_is_the_shortest_and_the_riffle_the_longest() {
        let clink = SoundCue::ChipClink.spec().duration_s;
        let slide = SoundCue::CardSlide.spec().duration_s;
        let riffle = SoundCue::Riffle.spec().duration_s;
        assert!(clink < slide && slide < riffle);
    }
}
