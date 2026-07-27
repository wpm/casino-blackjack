//! Application entry point: start a session through the [`Backend`]
//! seam, feed every [`Transition`] to the [`Motion`] choreographer, and
//! render the table scene from the display snapshot it advances.
//!
//! [`Backend`]: blackjack_ui::backend::Backend

use blackjack_core::Transition;
use blackjack_ui::motion::{Motion, MotionOverlay};
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
fn dev_fallback_transition() -> Transition {
    Transition {
        snapshot: blackjack_core::Table::from_seed(blackjack_core::Rules::canonical(), 0)
            .snapshot(),
        events: Vec::new(),
    }
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

/// Open the session and hand its opening transition to the
/// choreographer: `start_session` over the backend seam, or the
/// dev-only fallback outside a Tauri shell.
#[cfg(target_arch = "wasm32")]
fn open_session(motion: Motion) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    if !tauri_available() {
        motion.play(dev_fallback_transition());
        return;
    }
    leptos::task::spawn_local(async move {
        match TauriBackend::new().start_session().await {
            Ok(transition) => motion.play(transition),
            Err(error) => leptos::logging::error!("start_session failed: {error}"),
        }
    });
}

/// Host builds have no Tauri shell and never actually serve the UI;
/// keep the scene renderable for host-side tooling and tests.
#[cfg(not(target_arch = "wasm32"))]
fn open_session(motion: Motion) {
    motion.play(dev_fallback_transition());
}

#[component]
fn App() -> impl IntoView {
    // The one seam this file owns: transitions go through the
    // choreographer (`motion.play(transition)`), which advances the
    // display snapshot event-by-event; the scene renders that signal.
    let motion = Motion::new();
    open_session(motion.clone());
    let display = motion.display_signal();
    view! {
        <main style="position:relative;margin:0;padding:0;width:100vw;height:100vh;overflow:hidden;background:#0b0910;">
            {move || {
                display.get().map(|snapshot| view! { <TableScene snapshot=snapshot /> })
            }}
            <MotionOverlay motion=motion.clone() />
        </main>
    }
}
