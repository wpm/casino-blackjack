//! Application entry point: start a session through the [`Backend`]
//! seam, feed every [`Transition`] to the [`Motion`] choreographer, and
//! render the table scene from the display snapshot it advances, with
//! the gesture layer ([`InputLayer`]) over it.
//!
//! Two snapshots live here on purpose. The *authoritative* snapshot is
//! whatever the engine last returned; the input layer gates gestures
//! against it. The *display* snapshot trails it, advanced event-by-event
//! by the choreographer so cards fly and chips slide at dealer pace.
//!
//! Interaction flow: the input layer emits legality-gated
//! [`Intent`]s; [`handle_intent`] maps them to engine [`Action`]s
//! through [`blackjack_ui::input::resolve`] (an independent second
//! gate) and submits them over the backend seam; every returned
//! [`Transition`] updates the authoritative snapshot and enters the
//! choreographer's queue. Posting a bet chains straight into `Deal` —
//! at this one-human table, the round starts the moment the bet is
//! down.
//!
//! [`Backend`]: blackjack_ui::backend::Backend

use std::cell::RefCell;

use blackjack_core::{Action, ActionKind, Rules, Snapshot, Table, Transition};
use blackjack_ui::input::{GestureCtx, InputLayer, Intent, human_seat, resolve};
use blackjack_ui::motion::{Motion, MotionOverlay};
use blackjack_ui::overlay::HelpOverlay;
use blackjack_ui::scene::TableScene;
use leptos::prelude::*;
use rand_chacha::ChaCha8Rng;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// Record a transition: the authoritative snapshot updates immediately
/// (gestures gate against it), and the choreographer plays the events
/// out on the display snapshot at table pace.
fn deliver(authoritative: RwSignal<Option<Snapshot>>, motion: &Motion, transition: Transition) {
    authoritative.set(Some(transition.snapshot.clone()));
    motion.play(transition);
}

thread_local! {
    /// DEV-ONLY fallback table (see [`dev_start`]).
    static DEV_TABLE: RefCell<Option<Table<ChaCha8Rng>>> = const { RefCell::new(None) };
}

/// DEV-ONLY fallback: a fresh table built in-process from a fixed seed.
///
/// Used when the Tauri IPC global is absent — i.e. `trunk serve` in a
/// plain browser — so the scene still renders (and plays) for
/// development. The real app always goes through the backend seam;
/// nothing outside this fallback ever touches [`Table`] from the
/// frontend.
fn dev_start() -> Transition {
    DEV_TABLE.with(|cell| {
        let snapshot = cell
            .borrow_mut()
            .get_or_insert_with(|| Table::from_seed(Rules::canonical(), 0))
            .snapshot();
        Transition {
            snapshot,
            events: Vec::new(),
        }
    })
}

/// DEV-ONLY fallback for [`submit`]: apply directly to the local table
/// and return each resulting transition for the choreographer.
fn dev_submit(seat: usize, action: Action, chain_deal: bool) -> Vec<Transition> {
    DEV_TABLE.with(|cell| {
        let mut guard = cell.borrow_mut();
        let Some(table) = guard.as_mut() else {
            return Vec::new();
        };
        let mut transitions = Vec::new();
        match table.apply(seat, action) {
            Ok(transition) => {
                let deal = chain_deal
                    && transition
                        .snapshot
                        .legal_actions
                        .contains(&ActionKind::Deal);
                transitions.push(transition);
                if deal {
                    match table.apply(seat, Action::Deal) {
                        Ok(transition) => transitions.push(transition),
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
        transitions
    })
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

/// Open the session: `start_session` over the backend seam, or the
/// dev-only fallback outside a Tauri shell.
#[cfg(target_arch = "wasm32")]
fn open_session(authoritative: RwSignal<Option<Snapshot>>, motion: Motion) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    if !tauri_available() {
        deliver(authoritative, &motion, dev_start());
        return;
    }
    leptos::task::spawn_local(async move {
        match TauriBackend::new().start_session().await {
            Ok(transition) => deliver(authoritative, &motion, transition),
            Err(error) => leptos::logging::error!("start_session failed: {error}"),
        }
    });
}

/// Host builds have no Tauri shell and never actually serve the UI;
/// keep the scene renderable for host-side tooling and tests.
#[cfg(not(target_arch = "wasm32"))]
fn open_session(authoritative: RwSignal<Option<Snapshot>>, motion: Motion) {
    deliver(authoritative, &motion, dev_start());
}

/// Submit one action for the seat over the backend seam, delivering
/// every returned transition. With `chain_deal`, a successful call is
/// followed by `Deal` as soon as the engine offers it.
///
/// Every action arriving here has passed [`resolve`]'s legality gate,
/// so a `Rejected` reply is an input-layer bug and is logged as one.
#[cfg(target_arch = "wasm32")]
fn submit(
    authoritative: RwSignal<Option<Snapshot>>,
    motion: Motion,
    seat: usize,
    action: Action,
    chain_deal: bool,
) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    if !tauri_available() {
        for transition in dev_submit(seat, action, chain_deal) {
            deliver(authoritative, &motion, transition);
        }
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
                deliver(authoritative, &motion, transition);
                if deal {
                    match backend.submit_action(seat, Action::Deal).await {
                        Ok(transition) => deliver(authoritative, &motion, transition),
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
fn submit(
    authoritative: RwSignal<Option<Snapshot>>,
    motion: Motion,
    seat: usize,
    action: Action,
    chain_deal: bool,
) {
    for transition in dev_submit(seat, action, chain_deal) {
        deliver(authoritative, &motion, transition);
    }
}

/// Map an [`Intent`] from the input layer to an engine action and
/// submit it. [`Intent::WalkAway`] is not an engine action: it is noted
/// in `walked_away` for #12's cash-out arc.
fn handle_intent(
    authoritative: RwSignal<Option<Snapshot>>,
    motion: Motion,
    walked_away: RwSignal<bool>,
    intent: Intent,
) {
    let Some(current) = authoritative.get_untracked() else {
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
        authoritative,
        motion,
        seat,
        action,
        matches!(action, Action::PlaceBet(_)),
    );
}

#[component]
fn App() -> impl IntoView {
    let authoritative = RwSignal::new(Option::<Snapshot>::None);
    let walked_away = RwSignal::new(false);
    let motion = Motion::new();
    open_session(authoritative, motion.clone());
    let display = motion.display_signal();
    let on_intent = UnsyncCallback::new({
        let motion = motion.clone();
        move |intent: Intent| handle_intent(authoritative, motion.clone(), walked_away, intent)
    });
    view! {
        <main style="position:relative;margin:0;padding:0;width:100vw;height:100vh;overflow:hidden;background:#0b0910;">
            {move || {
                display.get().map(|snapshot| view! { <TableScene snapshot=snapshot /> })
            }}
            <MotionOverlay motion=motion.clone() />
            <InputLayer snapshot=authoritative on_intent=on_intent />
            // The help glass: hold `?` or F1. Mounted last so it sits
            // above the gesture layer; pointer-events pass through it.
            <HelpOverlay snapshot=authoritative />
        </main>
    }
}
