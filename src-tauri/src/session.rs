//! The authoritative game session held by the Tauri shell, and the plain
//! functions the IPC commands wrap.
//!
//! The `#[tauri::command]` fns in `lib.rs` are one-line wrappers over the
//! free functions here, so tests can drive the exact command code path
//! without a running Tauri app. The session state machine itself is
//! [`SessionArc`] (shared with the browser dev fallback via
//! `blackjack-protocol`); this layer adds only session lifecycle
//! ([`BackendError::NoSession`]), OS-random seeding, and locking.
//! Nothing is ever persisted: a session lives and dies with the process.

use std::sync::{Mutex, MutexGuard};

use blackjack_core::{Action, ChipStack};
use blackjack_protocol::{BackendError, SessionArc, SessionView};
use rand::RngCore;

/// The Tauri-managed authoritative game state: at most one session.
#[derive(Default)]
pub struct SessionState(Mutex<Option<SessionArc>>);

fn lock(state: &SessionState) -> MutexGuard<'_, Option<SessionArc>> {
    // A poisoned mutex means a command panicked mid-call; the session is
    // still structurally valid (the engine mutates only after
    // validating), so recover the guard rather than wedging the app.
    state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Run `f` against the live session, or [`BackendError::NoSession`].
fn with_session<T>(
    state: &SessionState,
    f: impl FnOnce(&mut SessionArc) -> Result<T, BackendError>,
) -> Result<T, BackendError> {
    let mut guard = lock(state);
    let session = guard.as_mut().ok_or(BackendError::NoSession)?;
    f(session)
}

/// Start a fresh session with an OS-random seed, replacing any existing
/// one: randomized buy-in, table already warm from AI-only rounds. The
/// returned view's events are empty — the player arrives at a table that
/// simply is.
pub fn start_session(state: &SessionState) -> Result<SessionView, BackendError> {
    start_session_with_seed(state, rand::rng().next_u64())
}

/// [`start_session`] with a caller-chosen seed, for deterministic tests
/// and replays. The commands only ever call this through
/// [`start_session`].
pub fn start_session_with_seed(
    state: &SessionState,
    seed: u64,
) -> Result<SessionView, BackendError> {
    let session = SessionArc::from_seed(seed);
    let view = session.view();
    *lock(state) = Some(session);
    Ok(view)
}

/// [`start_session_with_seed`] with the buy-in and warm-up length pinned
/// as well — for tests that need a poor player or a known table depth.
pub fn start_session_with_buy_in(
    state: &SessionState,
    seed: u64,
    chips: ChipStack,
    warmup_rounds: u32,
) -> Result<SessionView, BackendError> {
    let session = SessionArc::with_buy_in(seed, chips, warmup_rounds);
    let view = session.view();
    *lock(state) = Some(session);
    Ok(view)
}

/// The current state, changing nothing (events empty).
pub fn view(state: &SessionState) -> Result<SessionView, BackendError> {
    with_session(state, |session| Ok(session.view()))
}

/// One engine beat ([`blackjack_core::TableLife::advance`]).
pub fn advance(state: &SessionState) -> Result<SessionView, BackendError> {
    with_session(state, |session| Ok(session.advance()))
}

/// Submit one action for the human seat, legality re-gated server-side.
pub fn human_action(state: &SessionState, action: Action) -> Result<SessionView, BackendError> {
    with_session(state, |session| session.human_action(action))
}

/// Leave the table (between rounds only): color up, cash out.
pub fn walk_away(state: &SessionState) -> Result<SessionView, BackendError> {
    with_session(state, |session| session.walk_away())
}
