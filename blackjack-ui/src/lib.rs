//! Frontend library for Casino Blackjack.
//!
//! The UI reaches the engine exclusively through
//! [`backend::Backend`]; see that module for the seam and its Tauri IPC
//! implementation. The visual table itself lives in [`scene`]: pure
//! components that render a [`Snapshot`](blackjack_core::Snapshot) and
//! never call the backend. Interaction — felt gestures and the keyboard
//! mirror — lives in [`input`], which emits legality-gated intents and
//! likewise never calls the backend itself. Event-driven animation and
//! sound live in [`motion`], which choreographs transitions onto a
//! display snapshot without ever inventing game facts.

pub mod backend;
pub mod input;
pub mod motion;
pub mod scene;
