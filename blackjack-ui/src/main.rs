//! Application entry point: start a session through the [`Backend`]
//! seam, drive the session arc, feed every [`SessionView`] to the
//! [`Motion`] choreographer, and render the table scene from the display
//! snapshot it advances, with the gesture layer ([`InputLayer`]) over it.
//!
//! Two snapshots live here on purpose. The *authoritative* snapshot is
//! whatever the engine last returned; the input layer gates gestures
//! against it. The *display* snapshot trails it, advanced event-by-event
//! by the choreographer so cards fly and chips slide at dealer pace.
//!
//! # The drive loop
//!
//! Every backend reply is a [`SessionView`]: transition, rack, pace,
//! [`Awaiting`], [`SessionStatus`]. [`deliver`] records it (authoritative
//! snapshot, rack, status) and hands the transition to
//! [`Motion::play_paced`] at the dealer's real pace. While the view says
//! [`Awaiting::Engine`] and the session is still
//! [`SessionStatus::Playing`], [`pump`] keeps calling `advance` — one
//! engine beat per call, each animated in turn — and stops the moment a
//! `Human*` state or a terminal status arrives. Human input re-enters
//! through [`handle_intent`]: the input layer emits legality-gated
//! [`Intent`]s, [`resolve`] maps them to engine [`Action`]s (an
//! independent second gate), and `human_action` posts them; the returned
//! view restarts the pump. [`Intent::WalkAway`] goes to `walk_away`
//! instead — valid only between rounds, enforced server-side.
//!
//! When the status leaves `Playing`, the table is replaced by one of two
//! quiet full-screen states: cash-out (the one dollar figure in the game
//! outside the placard) or game over. Neither offers a restart:
//! relaunching the app is the only way to a new universe.
//!
//! [`Backend`]: blackjack_ui::backend::Backend

use std::cell::RefCell;

use blackjack_core::{Action, Awaiting, ChipStack, Snapshot};
use blackjack_protocol::{BackendError, SessionArc, SessionStatus, SessionView};
use blackjack_ui::input::{GestureCtx, InputLayer, Intent, human_seat, resolve};
use blackjack_ui::motion::{Motion, MotionOverlay};
use blackjack_ui::overlay::HelpOverlay;
use blackjack_ui::scene::TableScene;
use leptos::either::EitherOf3;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

/// The app's authoritative session signals, one per [`SessionView`]
/// field the UI gates or renders from. `Copy`, like all leptos signals.
#[derive(Clone, Copy)]
struct Session {
    /// The engine's last word; gestures gate against this.
    authoritative: RwSignal<Option<Snapshot>>,
    /// The human's rack — the real chips staging draws from.
    rack: RwSignal<ChipStack>,
    /// What the session waits on; drives the pump.
    awaiting: RwSignal<Awaiting>,
    /// Where the session stands; anything but `Playing` ends the table.
    status: RwSignal<SessionStatus>,
}

impl Session {
    fn new() -> Session {
        Session {
            authoritative: RwSignal::new(None),
            rack: RwSignal::new(ChipStack::new()),
            awaiting: RwSignal::new(Awaiting::Engine),
            status: RwSignal::new(SessionStatus::Playing),
        }
    }
}

/// Record a view: the authoritative signals update immediately (gestures
/// gate against them), and the choreographer plays the events out on the
/// display snapshot at the dealer's pace.
fn deliver(session: Session, motion: &Motion, view: SessionView) {
    session
        .authoritative
        .set(Some(view.transition.snapshot.clone()));
    session.rack.set(view.rack);
    session.awaiting.set(view.awaiting);
    session.status.set(view.status);
    motion.play_paced(view.transition, view.pace);
}

/// Whether the engine has pending work the client should pump.
fn engine_pending(session: Session) -> bool {
    session.awaiting.get_untracked() == Awaiting::Engine
        && session.status.get_untracked() == SessionStatus::Playing
}

thread_local! {
    /// DEV-ONLY fallback session (see [`dev_start`]).
    static DEV_SESSION: RefCell<Option<SessionArc>> = const { RefCell::new(None) };
}

/// DEV-ONLY fallback: the same [`SessionArc`] the Tauri shell runs,
/// built in-process from OS randomness.
///
/// Used when the Tauri IPC global is absent — i.e. `trunk serve` in a
/// plain browser — so the full session arc (buy-in, table life, walk
/// away, game over) is playable during development. The real app always
/// goes through the backend seam; nothing outside this fallback ever
/// touches the engine from the frontend.
fn dev_start() -> SessionView {
    use rand::RngCore;
    DEV_SESSION.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| SessionArc::from_seed(rand::rng().next_u64()))
            .view()
    })
}

/// DEV-ONLY: run `f` against the local session, if one is open.
fn dev_session<T>(f: impl FnOnce(&mut SessionArc) -> T) -> Option<T> {
    DEV_SESSION.with(|cell| cell.borrow_mut().as_mut().map(f))
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

#[cfg(not(target_arch = "wasm32"))]
fn tauri_available() -> bool {
    false
}

/// While the engine has work and the session lives, keep calling
/// `advance` — one beat per call — delivering every returned view to the
/// choreographer. Stops on a `Human*` awaiting or a terminal status.
fn pump(session: Session, motion: Motion) {
    if !engine_pending(session) {
        return;
    }
    if tauri_available() {
        pump_backend(session, motion);
        return;
    }
    // The dev session is local and synchronous; each advance is cheap
    // and the loop provably rests at every human decision.
    let mut guard = 0u32;
    while engine_pending(session) {
        guard += 1;
        if guard > 100_000 {
            leptos::logging::error!("dev pump stopped making progress");
            return;
        }
        let Some(view) = dev_session(|s| s.advance()) else {
            return;
        };
        deliver(session, &motion, view);
    }
}

/// The async pump against the Tauri backend, at most one in flight.
#[cfg(target_arch = "wasm32")]
fn pump_backend(session: Session, motion: Motion) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    thread_local! {
        /// Reentrancy guard: at most one pump loop in flight.
        static PUMPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if PUMPING.with(|flag| flag.replace(true)) {
        return;
    }
    leptos::task::spawn_local(async move {
        let backend = TauriBackend::new();
        while engine_pending(session) {
            match backend.advance().await {
                Ok(view) => deliver(session, &motion, view),
                Err(error) => {
                    leptos::logging::error!("advance failed: {error}");
                    break;
                }
            }
        }
        PUMPING.with(|flag| flag.set(false));
    });
}

/// Host builds never have a Tauri shell; the dev loop covers them.
#[cfg(not(target_arch = "wasm32"))]
fn pump_backend(_session: Session, _motion: Motion) {}

/// Open the session: `start_session` over the backend seam, or the
/// dev-only fallback outside a Tauri shell. Either way the pump takes
/// over — the player arrives mid-life and the table simply carries on.
fn open_session(session: Session, motion: Motion) {
    if !tauri_available() {
        deliver(session, &motion, dev_start());
        pump(session, motion);
        return;
    }
    open_backend(session, motion);
}

/// Open the session against the Tauri backend.
#[cfg(target_arch = "wasm32")]
fn open_backend(session: Session, motion: Motion) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    leptos::task::spawn_local(async move {
        match TauriBackend::new().start_session().await {
            Ok(view) => {
                deliver(session, &motion, view);
                pump(session, motion);
            }
            Err(error) => leptos::logging::error!("start_session failed: {error}"),
        }
    });
}

/// Host builds never have a Tauri shell; [`dev_start`] covers them.
#[cfg(not(target_arch = "wasm32"))]
fn open_backend(_session: Session, _motion: Motion) {}

/// How to report a backend rejection: anything the input layer's two
/// legality gates should have caught is an input-layer bug; chip
/// shortfalls (a double or split the rack cannot cover) and mistimed
/// walk-aways are quieter, expected refusals — the gesture layer cannot
/// see the server's chip accounting.
fn log_rejection(context: &str, error: &BackendError) {
    match error {
        BackendError::InsufficientChips { .. } | BackendError::NotBetweenRounds => {
            leptos::logging::log!("{context} refused: {error}");
        }
        _ => leptos::logging::error!("input-layer bug: {context} rejected: {error}"),
    }
}

/// Submit one action for the human seat over the backend seam, then let
/// the pump play out whatever the action set in motion.
fn submit(session: Session, motion: Motion, action: Action) {
    if !tauri_available() {
        let Some(result) = dev_session(|s| s.human_action(action)) else {
            return;
        };
        match result {
            Ok(view) => {
                deliver(session, &motion, view);
                pump(session, motion);
            }
            Err(error) => log_rejection(&format!("{action:?}"), &error),
        }
        return;
    }
    submit_backend(session, motion, action);
}

/// Submit the human's action against the Tauri backend.
#[cfg(target_arch = "wasm32")]
fn submit_backend(session: Session, motion: Motion, action: Action) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    leptos::task::spawn_local(async move {
        match TauriBackend::new().human_action(action).await {
            Ok(view) => {
                deliver(session, &motion, view);
                pump(session, motion);
            }
            Err(error) => log_rejection(&format!("{action:?}"), &error),
        }
    });
}

/// Host builds never have a Tauri shell; the dev session covers them.
#[cfg(not(target_arch = "wasm32"))]
fn submit_backend(_session: Session, _motion: Motion, _action: Action) {}

/// Leave the table: `walk_away` over the seam. The server allows it only
/// between rounds with no chips on the felt; a refusal (say, a posted
/// bet the round is about to play) is logged quietly and play goes on.
fn walk_away(session: Session, motion: Motion) {
    if !tauri_available() {
        let Some(result) = dev_session(|s| s.walk_away()) else {
            return;
        };
        match result {
            Ok(view) => deliver(session, &motion, view),
            Err(error) => log_rejection("walk-away", &error),
        }
        return;
    }
    walk_away_backend(session, motion);
}

/// Walk away against the Tauri backend.
#[cfg(target_arch = "wasm32")]
fn walk_away_backend(session: Session, motion: Motion) {
    use blackjack_ui::backend::{Backend, TauriBackend};

    leptos::task::spawn_local(async move {
        match TauriBackend::new().walk_away().await {
            Ok(view) => deliver(session, &motion, view),
            Err(error) => log_rejection("walk-away", &error),
        }
    });
}

/// Host builds never have a Tauri shell; the dev session covers them.
#[cfg(not(target_arch = "wasm32"))]
fn walk_away_backend(_session: Session, _motion: Motion) {}

/// Map an [`Intent`] from the input layer to its backend call.
/// [`Intent::WalkAway`] goes to the session's walk-away; everything else
/// resolves to an engine action through [`resolve`] (the second,
/// independent legality gate) and submits as the human's action.
fn handle_intent(session: Session, motion: Motion, intent: Intent) {
    let Some(current) = session.authoritative.get_untracked() else {
        return;
    };
    if intent == Intent::WalkAway {
        walk_away(session, motion);
        return;
    }
    let seat = human_seat(&current);
    let ctx = GestureCtx::from_snapshot(&current, seat);
    let Some(action) = resolve(&ctx, intent) else {
        return;
    };
    submit(session, motion, action);
}

/// The quiet full-screen idiom shared by the session's two endings:
/// a dark room, a few words, nothing to click.
#[component]
fn QuietScreen(children: Children) -> impl IntoView {
    view! {
        <div style="position:absolute;inset:0;display:flex;flex-direction:column;\
                    align-items:center;justify-content:center;gap:1.5rem;\
                    background:#0b0910;color:#d8d2c0;user-select:none;\
                    font-family:Georgia, 'Times New Roman', serif;">
            {children()}
        </div>
    }
}

/// The cash-out screen: the one dollar figure in the game outside the
/// placard, landing with weight. Win or lose, this is what the night
/// was worth. No restart — relaunching the app is the only way back.
#[component]
fn CashOutScreen(dollars: u32) -> impl IntoView {
    view! {
        <QuietScreen>
            <p style="margin:0;font-size:0.9rem;letter-spacing:0.35em;\
                      text-transform:uppercase;opacity:0.55;">
                "You leave with"
            </p>
            <p style="margin:0;font-size:5.5rem;color:#ffd97a;">{format!("${dollars}")}</p>
        </QuietScreen>
    }
}

/// The game-over screen: the same quiet idiom, no figure at all.
#[component]
fn GameOverScreen() -> impl IntoView {
    view! {
        <QuietScreen>
            <p style="margin:0;font-size:2.4rem;letter-spacing:0.5em;\
                      text-transform:uppercase;opacity:0.8;">
                "Game Over"
            </p>
        </QuietScreen>
    }
}

#[component]
fn App() -> impl IntoView {
    let session = Session::new();
    let motion = Motion::new();
    open_session(session, motion.clone());
    let display = motion.display_signal();
    let on_intent = UnsyncCallback::new({
        let motion = motion.clone();
        move |intent: Intent| handle_intent(session, motion.clone(), intent)
    });
    view! {
        <main style="position:relative;margin:0;padding:0;width:100vw;height:100vh;overflow:hidden;background:#0b0910;">
            {move || {
                display.get().map(|snapshot| view! { <TableScene snapshot=snapshot /> })
            }}
            <MotionOverlay motion=motion.clone() />
            {move || match session.status.get() {
                SessionStatus::Playing => {
                    EitherOf3::A(
                        view! {
                            <InputLayer snapshot=session.authoritative rack=session.rack on_intent=on_intent />
                            // The help glass: hold `?` or F1. Mounted last
                            // so it sits above the gesture layer;
                            // pointer-events pass through it.
                            <HelpOverlay snapshot=session.authoritative />
                        },
                    )
                }
                SessionStatus::CashedOut { dollars } => {
                    EitherOf3::B(view! { <CashOutScreen dollars=dollars /> })
                }
                SessionStatus::GameOver => EitherOf3::C(view! { <GameOverScreen /> }),
            }}
        </main>
    }
}
