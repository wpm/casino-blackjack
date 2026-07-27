//! Frontend library for Casino Blackjack.
//!
//! The UI reaches the engine exclusively through
//! [`backend::Backend`]; see that module for the seam and its Tauri IPC
//! implementation. The visual table itself lives in [`scene`]: pure
//! components that render a [`Snapshot`](blackjack_core::Snapshot) and
//! never call the backend. Interaction — felt gestures and the keyboard
//! mirror — lives in [`input`], which emits legality-gated intents and
//! likewise never calls the backend itself. Annotations suspended above
//! the table — the frosted-glass help layer and whatever reuses its
//! idiom — live in [`overlay`].

pub mod backend;
pub mod input;
pub mod overlay;
pub mod scene;
