//! The wall-clock driver: play the timed queue with
//! `requestAnimationFrame`, patching the display snapshot and firing
//! sounds as steps complete.
//!
//! All choreography decisions were made host-side by
//! [`plan`](super::plan); this module only advances time. Wall-clock
//! timing is fine here — the no-clock rule binds the engine, not the
//! UI. One RAF loop runs while the queue is non-empty and parks itself
//! when the table is at rest.

use std::cell::RefCell;
use std::collections::VecDeque;

use leptos::prelude::Set;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use super::audio::Mixer;
use super::display::DisplayState;
use super::overlay::{Overlay, frame};
use super::plan::{Patch, TimedStep};
use super::sound::SoundCue;

/// The step currently on the felt.
struct Active {
    step: TimedStep,
    /// RAF timestamp when the step began.
    start: f64,
    /// Whether the display patch has landed.
    patched: bool,
}

/// Mutable driver state, behind one `RefCell`.
pub(super) struct DriverState {
    /// Steps waiting to play. New transitions append; nothing is ever
    /// dropped.
    pub(super) queue: VecDeque<TimedStep>,
    /// The display state after everything queued has played — what new
    /// transitions plan against.
    pub(super) planning: DisplayState,
    /// The display state as currently shown; patched step by step.
    shown: DisplayState,
    current: Option<Active>,
    /// Whether a RAF callback is scheduled.
    running: bool,
}

/// The RAF driver plus the audio mixer. One per [`Motion`](super::Motion).
pub(super) struct Driver {
    pub(super) state: RefCell<DriverState>,
    /// The persistent RAF callback, installed once by
    /// [`Motion::new`](super::Motion::new).
    raf: RefCell<Option<Closure<dyn FnMut(f64)>>>,
    pub(super) audio: Mixer,
}

impl Driver {
    pub(super) fn new() -> Driver {
        Driver {
            state: RefCell::new(DriverState {
                queue: VecDeque::new(),
                planning: DisplayState::new(),
                shown: DisplayState::new(),
                current: None,
                running: false,
            }),
            raf: RefCell::new(None),
            audio: Mixer::new(),
        }
    }
}

impl super::Motion {
    /// Install the persistent RAF callback. Called once from
    /// [`Motion::new`](super::Motion::new); the callback holds a clone
    /// of the `Motion` handle for the life of the app.
    pub(super) fn install_raf(&self) {
        let motion = self.clone();
        *self.driver.raf.borrow_mut() = Some(Closure::new(move |now: f64| motion.tick(now)));
    }

    /// Schedule a frame if the loop is parked.
    pub(super) fn ensure_running(&self) {
        {
            let mut state = self.driver.state.borrow_mut();
            if state.running {
                return;
            }
            state.running = true;
        }
        self.request_frame();
    }

    fn request_frame(&self) {
        let raf = self.driver.raf.borrow();
        if let (Some(window), Some(callback)) = (web_sys::window(), raf.as_ref()) {
            let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
        }
    }

    /// One animation frame: advance the current step, commit patches
    /// that have come due, start the next step when this one lands.
    fn tick(&self, now: f64) {
        let mut sounds: Vec<SoundCue> = Vec::new();
        let mut display_update: Option<Option<blackjack_core::Snapshot>> = None;
        // Assigned on every loop exit: either a frame or a cleared layer.
        let overlay_update: Option<Overlay>;
        let keep_running;
        {
            let mut state = self.driver.state.borrow_mut();
            loop {
                let Some(mut active) = state.current.take() else {
                    match state.queue.pop_front() {
                        Some(step) => {
                            if let Some(cue) = step.sound.filter(|cue| cue.at_step_start()) {
                                sounds.push(cue);
                            }
                            state.current = Some(Active {
                                step,
                                start: now,
                                patched: false,
                            });
                            continue;
                        }
                        None => {
                            // At rest: clear the layer and park the loop.
                            overlay_update = None;
                            state.running = false;
                            break;
                        }
                    }
                };
                let duration = f64::from(active.step.duration_ms);
                let t = if duration <= 0.0 {
                    1.0
                } else {
                    ((now - active.start) / duration).clamp(0.0, 1.0)
                };
                if !active.patched && t >= active.step.patch_at {
                    active.patched = true;
                    let patch = std::mem::replace(&mut active.step.patch, Patch::None);
                    match patch {
                        Patch::Event(event) => state.shown.apply(&event),
                        Patch::ClearFelt => state.shown.clear_felt(),
                        Patch::Adopt(snapshot) => state.shown.adopt(*snapshot),
                        Patch::None => {}
                    }
                    display_update = Some(state.shown.snapshot().cloned());
                }
                if t >= 1.0 {
                    if let Some(cue) = active.step.sound.filter(|cue| !cue.at_step_start()) {
                        sounds.push(cue);
                    }
                    // The step landed; start the next one this frame.
                    continue;
                }
                overlay_update = frame(&active.step.kind, t);
                state.current = Some(active);
                break;
            }
            keep_running = state.running;
        }
        // Signals fire outside the borrow: subscribers may re-enter
        // `Motion` (e.g. #9 submitting an action from an effect).
        if let Some(snapshot) = display_update {
            self.display.set(snapshot);
        }
        self.overlay.set(overlay_update);
        for cue in sounds {
            self.driver.audio.play(cue);
        }
        if keep_running {
            self.request_frame();
        }
    }
}
