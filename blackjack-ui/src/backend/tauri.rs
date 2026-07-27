//! The Tauri IPC implementation of [`Backend`], and the only place the
//! frontend touches the Tauri global.
//!
//! The binding is a single hand-rolled `wasm_bindgen` extern to
//! `window.__TAURI__.core.invoke` (available because the app sets
//! `app.withGlobalTauri: true` in `tauri.conf.json`). Arguments and
//! results cross the boundary through `serde_wasm_bindgen`, so both sides
//! speak exactly the shared serde types from `blackjack-core` and
//! `blackjack-protocol`.

use blackjack_core::Action;
use blackjack_protocol::{Backend, BackendError, HumanActionArgs, SessionView};
use serde::Serialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    /// `window.__TAURI__.core.invoke(cmd, args)` from Tauri 2.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    fn tauri_invoke(cmd: &str, args: JsValue) -> js_sys::Promise;
}

fn transport(cmd: &str, detail: impl core::fmt::Display) -> BackendError {
    BackendError::Transport(format!("{cmd}: {detail}"))
}

/// Call one Tauri command and decode its reply.
///
/// A rejected promise normally carries the command's serialized
/// [`BackendError`]; anything undecodable (missing global, panicked
/// command) is folded into [`BackendError::Transport`].
async fn invoke<T: DeserializeOwned>(cmd: &str, args: JsValue) -> Result<T, BackendError> {
    match JsFuture::from(tauri_invoke(cmd, args)).await {
        Ok(value) => serde_wasm_bindgen::from_value(value).map_err(|err| transport(cmd, err)),
        Err(error) => Err(
            serde_wasm_bindgen::from_value::<BackendError>(error.clone())
                .unwrap_or_else(|_| transport(cmd, format!("{error:?}"))),
        ),
    }
}

fn to_args(args: &impl Serialize) -> Result<JsValue, BackendError> {
    serde_wasm_bindgen::to_value(args)
        .map_err(|err| BackendError::Transport(format!("failed to encode arguments: {err}")))
}

/// [`Backend`] over Tauri IPC: the native shell holds the authoritative
/// session and this struct is a stateless client for it.
#[derive(Debug, Clone, Copy, Default)]
pub struct TauriBackend;

impl TauriBackend {
    /// A new IPC client. Cheap: all state lives in the Tauri process.
    pub fn new() -> TauriBackend {
        TauriBackend
    }
}

impl Backend for TauriBackend {
    async fn start_session(&self) -> Result<SessionView, BackendError> {
        invoke("start_session", JsValue::UNDEFINED).await
    }

    async fn view(&self) -> Result<SessionView, BackendError> {
        invoke("view", JsValue::UNDEFINED).await
    }

    async fn advance(&self) -> Result<SessionView, BackendError> {
        invoke("advance", JsValue::UNDEFINED).await
    }

    async fn human_action(&self, action: Action) -> Result<SessionView, BackendError> {
        invoke("human_action", to_args(&HumanActionArgs { action })?).await
    }

    async fn walk_away(&self) -> Result<SessionView, BackendError> {
        invoke("walk_away", JsValue::UNDEFINED).await
    }
}
