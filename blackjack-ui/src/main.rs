//! Application entry point: start a session through the [`Backend`]
//! seam, hold the [`Snapshot`] in a signal, and render the table scene
//! from it.
//!
//! [`Backend`]: blackjack_ui::backend::Backend

use blackjack_core::Snapshot;
use blackjack_ui::scene::TableScene;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// DEV-ONLY fallback: a fresh table built in-process from a fixed seed.
///
/// Used when the Tauri IPC global is absent — i.e. `trunk serve` in a
/// plain browser — so the scene still renders for development. The real
/// app always goes through the backend seam; nothing outside this
/// fallback ever touches [`blackjack_core::Table`] from the frontend.
fn dev_fallback_snapshot() -> Snapshot {
    blackjack_core::Table::from_seed(blackjack_core::Rules::canonical(), 0).snapshot()
}

/// Whether the Tauri IPC global (`window.__TAURI__`) is present.
#[cfg(target_arch = "wasm32")]
fn tauri_available() -> bool {
    js_sys::Reflect::has(
        &js_sys::global(),
        &wasm_bindgen::JsValue::from_str("__TAURI__"),
    )
    .unwrap_or(false)
}

/// Populate `snapshot` with the opening state: `start_session` over the
/// backend seam, or the dev-only fallback outside a Tauri shell.
#[cfg(target_arch = "wasm32")]
fn open_session(snapshot: RwSignal<Option<Snapshot>>) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    if !tauri_available() {
        snapshot.set(Some(dev_fallback_snapshot()));
        return;
    }
    leptos::task::spawn_local(async move {
        match TauriBackend::new().start_session().await {
            Ok(transition) => snapshot.set(Some(transition.snapshot)),
            Err(error) => leptos::logging::error!("start_session failed: {error}"),
        }
    });
}

/// Host builds have no Tauri shell and never actually serve the UI;
/// keep the scene renderable for host-side tooling and tests.
#[cfg(not(target_arch = "wasm32"))]
fn open_session(snapshot: RwSignal<Option<Snapshot>>) {
    snapshot.set(Some(dev_fallback_snapshot()));
}

#[component]
fn App() -> impl IntoView {
    let snapshot = RwSignal::new(Option::<Snapshot>::None);
    open_session(snapshot);
    view! {
        <main style="margin:0;padding:0;width:100vw;height:100vh;overflow:hidden;background:#0b0910;">
            {move || {
                snapshot.get().map(|snapshot| view! { <TableScene snapshot=snapshot /> })
            }}
        </main>
    }
}
