//! The authoritative game session held by the Tauri shell, and the plain
//! functions the IPC commands wrap.
//!
//! The `#[tauri::command]` fns in `lib.rs` are one-line wrappers over the
//! free functions here, so tests can drive the exact command code path
//! without a running Tauri app. All engine access goes through
//! [`Table::apply`]; this layer adds only session lifecycle
//! ([`BackendError::NoSession`]) and locking.

use std::sync::{Mutex, MutexGuard};

use blackjack_core::{Action, Rules, Table, Transition};
use blackjack_protocol::BackendError;
use rand::RngCore;
use rand_chacha::ChaCha8Rng;

/// One live game: a table under canonical rules on a seeded shoe.
///
/// Later issues swap what backs a session (AI-run seats, the full session
/// arc) without changing the command surface, which stays
/// snapshot/transition-shaped.
pub struct GameSession {
    table: Table<ChaCha8Rng>,
}

impl GameSession {
    /// Open a canonical-rules table whose shoe is seeded from `seed`.
    pub fn new(seed: u64) -> GameSession {
        GameSession {
            table: Table::from_seed(Rules::canonical(), seed),
        }
    }
}

/// The Tauri-managed authoritative game state: at most one session.
#[derive(Default)]
pub struct SessionState(Mutex<Option<GameSession>>);

fn lock(state: &SessionState) -> MutexGuard<'_, Option<GameSession>> {
    // A poisoned mutex means a command panicked mid-apply; the table is
    // still structurally valid (apply mutates only after validating), so
    // recover the guard rather than wedging the app.
    state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Start a fresh session with an OS-random seed, replacing any existing
/// one, and return its opening state (betting phase, events empty).
pub fn start_session(state: &SessionState) -> Result<Transition, BackendError> {
    start_session_with_seed(state, rand::rng().next_u64())
}

/// [`start_session`] with a caller-chosen seed, for deterministic tests
/// and replays. The commands only ever call this through
/// [`start_session`].
pub fn start_session_with_seed(
    state: &SessionState,
    seed: u64,
) -> Result<Transition, BackendError> {
    let session = GameSession::new(seed);
    let snapshot = session.table.snapshot();
    *lock(state) = Some(session);
    Ok(Transition {
        snapshot,
        events: Vec::new(),
    })
}

/// The current state, changing nothing (events empty).
pub fn snapshot(state: &SessionState) -> Result<Transition, BackendError> {
    let guard = lock(state);
    let session = guard.as_ref().ok_or(BackendError::NoSession)?;
    Ok(Transition {
        snapshot: session.table.snapshot(),
        events: Vec::new(),
    })
}

/// Submit one action for one seat, exactly as [`Table::apply`].
pub fn submit_action(
    state: &SessionState,
    seat: usize,
    action: Action,
) -> Result<Transition, BackendError> {
    let mut guard = lock(state);
    let session = guard.as_mut().ok_or(BackendError::NoSession)?;
    session
        .table
        .apply(seat, action)
        .map_err(BackendError::Rejected)
}
