//! Application entry point: start a session through the [`Backend`]
//! seam, hold the [`Snapshot`] in a signal, and render the table scene
//! with the gesture layer ([`InputLayer`]) over it.
//!
//! Interaction flow: the input layer emits legality-gated
//! [`Intent`]s; [`handle_intent`] maps them to engine [`Action`]s
//! through [`blackjack_ui::input::resolve`] (an independent second
//! gate) and submits them over the backend seam, writing the returned
//! snapshot back into the signal. Posting a bet chains straight into
//! `Deal` — at this one-human table, the round starts the moment the
//! bet is down.
//!
//! [`Backend`]: blackjack_ui::backend::Backend

use std::cell::RefCell;

use blackjack_core::{Action, ActionKind, Rules, Snapshot, Table};
use blackjack_ui::input::{GestureCtx, InputLayer, Intent, human_seat, resolve};
use blackjack_ui::overlay::HelpOverlay;
use blackjack_ui::scene::TableScene;
use leptos::prelude::*;
use rand_chacha::ChaCha8Rng;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

thread_local! {
    /// DEV-ONLY fallback table (see [`dev_snapshot`]).
    static DEV_TABLE: RefCell<Option<Table<ChaCha8Rng>>> = const { RefCell::new(None) };
}

/// DEV-ONLY fallback: a fresh table built in-process from a fixed seed.
///
/// Used when the Tauri IPC global is absent — i.e. `trunk serve` in a
/// plain browser — so the scene still renders (and plays) for
/// development. The real app always goes through the backend seam;
/// nothing outside this fallback ever touches [`Table`] from the
/// frontend.
fn dev_snapshot() -> Snapshot {
    DEV_TABLE.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| Table::from_seed(Rules::canonical(), 0))
            .snapshot()
    })
}

/// DEV-ONLY fallback for [`submit`]: apply directly to the local table.
fn dev_submit(snapshot: RwSignal<Option<Snapshot>>, seat: usize, action: Action, chain_deal: bool) {
    let latest = DEV_TABLE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let table = guard.as_mut()?;
        let mut latest = match table.apply(seat, action) {
            Ok(transition) => transition.snapshot,
            Err(error) => {
                leptos::logging::error!("input-layer bug: engine rejected {action:?}: {error}");
                return None;
            }
        };
        if chain_deal && latest.legal_actions.contains(&ActionKind::Deal) {
            match table.apply(seat, Action::Deal) {
                Ok(transition) => latest = transition.snapshot,
                Err(error) => {
                    leptos::logging::error!("input-layer bug: engine rejected Deal: {error}");
                }
            }
        }
        Some(latest)
    });
    if let Some(latest) = latest {
        snapshot.set(Some(latest));
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

/// Populate `snapshot` with the opening state: `start_session` over the
/// backend seam, or the dev-only fallback outside a Tauri shell.
#[cfg(target_arch = "wasm32")]
fn open_session(snapshot: RwSignal<Option<Snapshot>>) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    if !tauri_available() {
        snapshot.set(Some(dev_snapshot()));
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
    snapshot.set(Some(dev_snapshot()));
}

/// Submit one action for the seat over the backend seam and write the
/// returned snapshot into the signal. With `chain_deal`, a successful
/// call is followed by `Deal` as soon as the engine offers it.
///
/// Every action arriving here has passed [`resolve`]'s legality gate,
/// so a `Rejected` reply is an input-layer bug and is logged as one.
#[cfg(target_arch = "wasm32")]
fn submit(snapshot: RwSignal<Option<Snapshot>>, seat: usize, action: Action, chain_deal: bool) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    if !tauri_available() {
        dev_submit(snapshot, seat, action, chain_deal);
        return;
    }
    leptos::task::spawn_local(async move {
        let backend = TauriBackend::new();
        match backend.submit_action(seat, action).await {
            Ok(transition) => {
                let deal = chain_deal
                    && transition
                        .snapshot
                        .legal_actions
                        .contains(&ActionKind::Deal);
                snapshot.set(Some(transition.snapshot));
                if deal {
                    match backend.submit_action(seat, Action::Deal).await {
                        Ok(transition) => snapshot.set(Some(transition.snapshot)),
                        Err(error) => {
                            leptos::logging::error!(
                                "input-layer bug: engine rejected Deal: {error}"
                            );
                        }
                    }
                }
            }
            Err(error) => {
                leptos::logging::error!("input-layer bug: engine rejected {action:?}: {error}");
            }
        }
    });
}

/// See the wasm `submit`; host builds only ever have the dev fallback.
#[cfg(not(target_arch = "wasm32"))]
fn submit(snapshot: RwSignal<Option<Snapshot>>, seat: usize, action: Action, chain_deal: bool) {
    dev_submit(snapshot, seat, action, chain_deal);
}

/// Map an [`Intent`] from the input layer to an engine action and
/// submit it. [`Intent::WalkAway`] is not an engine action: it is noted
/// in `walked_away` for #12's cash-out arc.
fn handle_intent(
    snapshot: RwSignal<Option<Snapshot>>,
    walked_away: RwSignal<bool>,
    intent: Intent,
) {
    let Some(current) = snapshot.get_untracked() else {
        return;
    };
    if intent == Intent::WalkAway {
        walked_away.set(true);
        leptos::logging::log!("walk-away noted; cash-out arrives with #12");
        return;
    }
    let seat = human_seat(&current);
    let ctx = GestureCtx::from_snapshot(&current, seat);
    let Some(action) = resolve(&ctx, intent) else {
        return;
    };
    submit(
        snapshot,
        seat,
        action,
        matches!(action, Action::PlaceBet(_)),
    );
}

#[component]
fn App() -> impl IntoView {
    let snapshot = RwSignal::new(Option::<Snapshot>::None);
    let walked_away = RwSignal::new(false);
    open_session(snapshot);
    let on_intent =
        UnsyncCallback::new(move |intent: Intent| handle_intent(snapshot, walked_away, intent));
    view! {
        <main style="position:relative;margin:0;padding:0;width:100vw;height:100vh;overflow:hidden;background:#0b0910;">
            {move || {
                snapshot.get().map(|snapshot| view! { <TableScene snapshot=snapshot /> })
            }}
            <InputLayer snapshot=snapshot on_intent=on_intent />
            // The help glass: hold `?` or F1. Mounted last so it sits
            // above the gesture layer; pointer-events pass through it.
            <HelpOverlay snapshot=snapshot />
        </main>
    }
}
