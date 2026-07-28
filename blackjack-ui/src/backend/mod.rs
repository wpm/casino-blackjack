//! The UI's engine access seam.
//!
//! Everything the UI knows about the game arrives through the
//! [`Backend`] trait (defined platform-free in `blackjack-protocol`):
//! snapshots and events in, actions out. Two implementations plug in
//! underneath without the UI noticing:
//!
//! - [`TauriBackend`] (wasm only): each call is a Tauri `invoke` to the
//!   native shell, which holds the authoritative table.
//! - [`InProcessBackend`]: the engine runs inside the frontend itself —
//!   the same [`SessionArc`](blackjack_protocol::SessionArc), no shell.
//!
//! [`ActiveBackend`] is the one place that choice is made. With the
//! `in-process` cargo feature (the web build for GitHub Pages) the
//! in-process engine is *the* backend and the Tauri module is not even
//! compiled. Without it, the app probes for the Tauri IPC global at
//! runtime and falls back to the in-process engine — the `trunk serve`
//! dev loop in a plain browser.

use blackjack_core::Action;
use blackjack_protocol::SessionView;
pub use blackjack_protocol::{Backend, BackendError};

mod in_process;
#[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
mod tauri;

pub use in_process::InProcessBackend;
#[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
pub use tauri::TauriBackend;

/// Whether the Tauri IPC global (`window.__TAURI__`) is present.
#[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
fn tauri_available() -> bool {
    js_sys::Reflect::has(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("__TAURI__"),
    )
    .unwrap_or(false)
}

/// The backend this build actually talks to, chosen by [`select`](
/// ActiveBackend::select). The [`Backend`] trait is not dyn-compatible
/// (its futures are unnameable), so runtime selection is this enum
/// rather than a `Box<dyn Backend>`.
#[derive(Debug, Clone, Copy)]
pub enum ActiveBackend {
    /// The native shell over Tauri IPC.
    #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
    Tauri(TauriBackend),
    /// The engine in-process: the web build, and the dev fallback when
    /// no Tauri shell is present.
    InProcess(InProcessBackend),
}

impl ActiveBackend {
    /// Choose the backend for this build. Compiled with `in-process`
    /// (or on a non-wasm host) the answer is static; otherwise the
    /// Tauri global decides at runtime. Cheap — both backends are
    /// stateless handles — so callers select per call site.
    pub fn select() -> ActiveBackend {
        #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
        if tauri_available() {
            return ActiveBackend::Tauri(TauriBackend::new());
        }
        ActiveBackend::InProcess(InProcessBackend::new())
    }
}

impl Backend for ActiveBackend {
    async fn start_session(&self) -> Result<SessionView, BackendError> {
        match self {
            #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
            ActiveBackend::Tauri(backend) => backend.start_session().await,
            ActiveBackend::InProcess(backend) => backend.start_session().await,
        }
    }

    async fn view(&self) -> Result<SessionView, BackendError> {
        match self {
            #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
            ActiveBackend::Tauri(backend) => backend.view().await,
            ActiveBackend::InProcess(backend) => backend.view().await,
        }
    }

    async fn advance(&self) -> Result<SessionView, BackendError> {
        match self {
            #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
            ActiveBackend::Tauri(backend) => backend.advance().await,
            ActiveBackend::InProcess(backend) => backend.advance().await,
        }
    }

    async fn human_action(&self, action: Action) -> Result<SessionView, BackendError> {
        match self {
            #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
            ActiveBackend::Tauri(backend) => backend.human_action(action).await,
            ActiveBackend::InProcess(backend) => backend.human_action(action).await,
        }
    }

    async fn walk_away(&self) -> Result<SessionView, BackendError> {
        match self {
            #[cfg(all(target_arch = "wasm32", not(feature = "in-process")))]
            ActiveBackend::Tauri(backend) => backend.walk_away().await,
            ActiveBackend::InProcess(backend) => backend.walk_away().await,
        }
    }
}
