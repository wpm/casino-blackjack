//! The UI's engine access seam.
//!
//! Everything the UI knows about the game arrives through the
//! [`Backend`] trait (defined platform-free in `blackjack-protocol`):
//! snapshots and events in, actions out. Implementations plug in
//! underneath without the UI noticing:
//!
//! - [`TauriBackend`] (wasm only, this module): each call is a Tauri
//!   `invoke` to the native shell, which holds the authoritative table.
//! - A future in-process implementation (issue #14) will run the engine
//!   inside the wasm module behind the same trait.

pub use blackjack_protocol::{Backend, BackendError};

#[cfg(target_arch = "wasm32")]
mod tauri;

#[cfg(target_arch = "wasm32")]
pub use tauri::TauriBackend;
