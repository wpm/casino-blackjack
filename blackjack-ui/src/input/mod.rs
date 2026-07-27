//! Interaction: felt gestures and their keyboard mirror. No buttons,
//! ever — every action is a mouse gesture analogous to the real table
//! signal, or a key.
//!
//! # Architecture
//!
//! Three layers, outermost first:
//!
//! 1. **Pure recognizers** ([`geometry`], [`gesture`], [`keyboard`],
//!    [`staging`]): plain functions from pointer strokes (in SVG viewBox
//!    coordinates) and the current snapshot's context to
//!    `Option<Intent>`. Fully host-testable; all thresholds and zone
//!    shapes live here.
//! 2. **The overlay** ([`overlay::InputLayer`]): a transparent SVG over
//!    the scene that converts DOM events to scene coordinates, renders
//!    the rack / staged bet / drag ghost / zone hints, and calls the
//!    recognizers.
//! 3. **The app** (main.rs): receives [`Intent`]s, maps them to engine
//!    actions through [`resolve`], and submits over the backend seam.
//!
//! # Safety invariant
//!
//! A gesture resolves to an action only when that [`ActionKind`](
//! blackjack_core::ActionKind) is in the snapshot's `legal_actions` and
//! the gesture is the human seat's to make — checked once in the
//! recognizers and again, independently, in [`resolve`]. A misread is a
//! no-op, never a wrong action; ambiguous input resolves to nothing.
//! The engine's illegal-action error is unreachable from this UI — any
//! `BackendError::Rejected` the app sees is a bug in this module.

pub mod geometry;
pub mod gesture;
pub mod keyboard;
pub mod overlay;
pub mod staging;

pub use geometry::{Point, Stroke};
pub use gesture::{
    GestureCtx, Grab, Intent, StrokeOutcome, classify_stroke, grab_at, human_seat, resolve,
};
pub use keyboard::{KeyCommand, command_intent, key_command};
pub use overlay::InputLayer;
