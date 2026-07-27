//! Motion and sound: the engine's event list, choreographed.
//!
//! The UI never invents game facts — every animation and every sound is
//! derived from the [`Event`](blackjack_core::Event)s the engine
//! reports, paced by the dealer's [`Pace`]. The architecture keeps two
//! states apart:
//!
//! - the **authoritative snapshot** inside each [`Transition`] — where
//!   the engine rests;
//! - the **display state** ([`display::DisplayState`]) — what the scene
//!   shows right now, advanced one event at a time.
//!
//! [`plan`] turns a transition into a queue of [`plan::TimedStep`]s
//! (pure, host-tested); on wasm the [`driver`] plays that queue with
//! `requestAnimationFrame`, moving one object in the [`overlay`] layer
//! and patching the display snapshot as each step lands, with a sparse
//! synthesized sound ([`sound`], [`audio`]) at the landing moments. The
//! final step of every plan adopts the authoritative snapshot, so the
//! display always arrives exactly where the engine rests. Transitions
//! arriving mid-animation append to the queue; events are never dropped
//! or batched.
//!
//! # The seam
//!
//! [`Motion`] is the whole surface: `main.rs` calls [`Motion::play`]
//! instead of writing the snapshot signal directly, and renders from
//! [`Motion::display_signal`]. When the session layer (#12) drives
//! [`TableLife`](blackjack_core::TableLife), it feeds each engine
//! step's real pace through [`Motion::play_paced`] (or
//! [`Motion::set_pace`]); until then the pace signal defaults to
//! [`Motion::DEFAULT_PACE`], the Measured dealer's numbers.

pub mod display;
pub mod overlay;
pub mod paths;
pub mod plan;
pub mod sound;

#[cfg(target_arch = "wasm32")]
mod audio;
#[cfg(target_arch = "wasm32")]
mod driver;

use blackjack_core::{Pace, Snapshot, Transition};
use leptos::prelude::*;

pub use overlay::{MotionOverlay, Overlay};

/// The choreographer's handle: owns the display signals and the
/// animation queue. Cheap to clone; all clones share one queue.
#[derive(Clone)]
pub struct Motion {
    /// What the scene renders: the display snapshot, advanced
    /// event-by-event as animations land.
    display: RwSignal<Option<Snapshot>>,
    /// The transient animation layer's current frame.
    overlay: RwSignal<Option<Overlay>>,
    /// The dealer's pace for planning new transitions.
    pace: RwSignal<Pace>,
    #[cfg(target_arch = "wasm32")]
    driver: std::rc::Rc<driver::Driver>,
}

impl Default for Motion {
    fn default() -> Motion {
        Motion::new()
    }
}

impl Motion {
    /// The pace assumed until #12 wires the real dealer through:
    /// the Measured persona's numbers from `blackjack_core::life`.
    pub const DEFAULT_PACE: Pace = Pace {
        card_ms: 400,
        decision_ms: 700,
        reveal_ms: 700,
        settlement_ms: 500,
        between_rounds_ms: 2200,
    };

    /// A fresh choreographer with an empty display. On wasm this also
    /// installs the animation-frame callback and the first-gesture
    /// audio unlock listeners (see [`Motion::unlock_audio`]).
    pub fn new() -> Motion {
        let motion = Motion {
            display: RwSignal::new(None),
            overlay: RwSignal::new(None),
            pace: RwSignal::new(Motion::DEFAULT_PACE),
            #[cfg(target_arch = "wasm32")]
            driver: std::rc::Rc::new(driver::Driver::new()),
        };
        #[cfg(target_arch = "wasm32")]
        {
            motion.install_raf();
            audio::install_unlock_handlers(&motion);
        }
        motion
    }

    /// The signal the scene renders from.
    pub fn display_signal(&self) -> RwSignal<Option<Snapshot>> {
        self.display
    }

    /// The signal the [`MotionOverlay`] layer renders from.
    pub fn overlay_signal(&self) -> RwSignal<Option<Overlay>> {
        self.overlay
    }

    /// Set the pace used to plan transitions from now on. The #12 seam:
    /// feed `Step.pace` here whenever the engine reports one.
    pub fn set_pace(&self, pace: Pace) {
        self.pace.set(pace);
    }

    /// Queue a transition for animated playback: its events play one at
    /// a time at the current pace, and the display ends at exactly
    /// `transition.snapshot`. If animations are still playing, the new
    /// transition waits its turn — events are never dropped.
    pub fn play(&self, transition: Transition) {
        let pace = self.pace.get_untracked();
        self.play_at(transition, pace);
    }

    /// [`Motion::play`] with an explicit pace — the one-call seam for
    /// #12, which receives a pace alongside every engine step.
    pub fn play_paced(&self, transition: Transition, pace: Pace) {
        self.pace.set(pace);
        self.play_at(transition, pace);
    }

    /// Wasm: plan against the end of the queue and start the driver.
    #[cfg(target_arch = "wasm32")]
    fn play_at(&self, transition: Transition, pace: Pace) {
        {
            let mut state = self.driver.state.borrow_mut();
            let steps = plan::plan_transition(&mut state.planning, &transition, pace);
            state.queue.extend(steps);
        }
        self.ensure_running();
    }

    /// Host: no frames to play; the display adopts the final snapshot
    /// immediately. Keeps `main.rs` target-agnostic and testable.
    #[cfg(not(target_arch = "wasm32"))]
    fn play_at(&self, transition: Transition, _pace: Pace) {
        self.display.set(Some(transition.snapshot));
    }

    /// Create or resume the audio context. Must first happen inside a
    /// user gesture (browser autoplay policy); `Motion::new` already
    /// listens for the first window `pointerdown`/`keydown`, and the
    /// input layer (#9) may also call this from its own handlers.
    pub fn unlock_audio(&self) {
        #[cfg(target_arch = "wasm32")]
        self.driver.audio.unlock();
    }
}
