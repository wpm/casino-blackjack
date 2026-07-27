//! Frontend library for Casino Blackjack.
//!
//! The UI reaches the engine exclusively through
//! [`backend::Backend`]; see that module for the seam and its Tauri IPC
//! implementation. The visual table itself lives in [`scene`]: pure
//! components that render a [`Snapshot`](blackjack_core::Snapshot) and
//! never call the backend.

pub mod backend;
pub mod motion;
pub mod scene;
