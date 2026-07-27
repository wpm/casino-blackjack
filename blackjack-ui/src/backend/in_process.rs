//! The in-process implementation of [`Backend`]: the complete engine
//! running inside the frontend itself, no shell required.
//!
//! One [`SessionArc`] — the identical platform-free arc the Tauri shell
//! holds behind a mutex — lives in a thread-local here, and every trait
//! method is a direct, synchronous call on it wrapped in the trait's
//! async signatures. Since the game is configurationless and
//! persistence-free, this is the whole game: the web build (the
//! `in-process` cargo feature) and the `trunk serve` dev fallback both
//! run it, with no feature gaps against the desktop app.
//!
//! The session is seeded from OS randomness (`getrandom`'s `wasm_js`
//! backend in the browser; see `.cargo/config.toml`), so every launch is
//! a different table — dev fallback included.

use std::cell::RefCell;

use blackjack_core::Action;
use blackjack_protocol::{Backend, BackendError, SessionArc, SessionView};

thread_local! {
    /// The one live session. Wasm is single-threaded, so a thread-local
    /// is effectively a process global — the same lifetime the Tauri
    /// shell gives its mutex-held arc.
    static SESSION: RefCell<Option<SessionArc>> = const { RefCell::new(None) };
}

/// Run `f` against the live session, or [`BackendError::NoSession`].
fn with_session<T>(
    f: impl FnOnce(&mut SessionArc) -> Result<T, BackendError>,
) -> Result<T, BackendError> {
    SESSION.with(|cell| match cell.borrow_mut().as_mut() {
        Some(session) => f(session),
        None => Err(BackendError::NoSession),
    })
}

/// [`Backend`] running the engine in-process: a stateless handle to the
/// thread-local [`SessionArc`], mirroring how [`TauriBackend`] is a
/// stateless client for the shell's session.
///
/// [`TauriBackend`]: super::TauriBackend
#[derive(Debug, Clone, Copy, Default)]
pub struct InProcessBackend;

impl InProcessBackend {
    /// A new handle. Cheap: all state lives in the thread-local session.
    pub fn new() -> InProcessBackend {
        InProcessBackend
    }
}

impl Backend for InProcessBackend {
    async fn start_session(&self) -> Result<SessionView, BackendError> {
        use rand::RngCore;
        let seed = rand::rng().next_u64();
        SESSION.with(|cell| Ok(cell.borrow_mut().insert(SessionArc::from_seed(seed)).view()))
    }

    async fn view(&self) -> Result<SessionView, BackendError> {
        with_session(|session| Ok(session.view()))
    }

    async fn advance(&self) -> Result<SessionView, BackendError> {
        with_session(|session| Ok(session.advance()))
    }

    async fn human_action(&self, action: Action) -> Result<SessionView, BackendError> {
        with_session(|session| session.human_action(action))
    }

    async fn walk_away(&self) -> Result<SessionView, BackendError> {
        with_session(|session| session.walk_away())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackjack_protocol::SessionStatus;

    /// The in-process futures never await anything: poll once, done.
    fn block_on<F: Future>(fut: F) -> F::Output {
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(value) => value,
            std::task::Poll::Pending => unreachable!("in-process futures are immediately ready"),
        }
    }

    // Each #[test] runs on its own thread, so the thread-local session
    // is naturally isolated between tests.

    #[test]
    fn every_call_before_start_is_no_session() {
        let backend = InProcessBackend::new();
        assert_eq!(block_on(backend.view()), Err(BackendError::NoSession));
        assert_eq!(block_on(backend.advance()), Err(BackendError::NoSession));
        assert_eq!(
            block_on(backend.human_action(Action::PlaceBet(10))),
            Err(BackendError::NoSession)
        );
        assert_eq!(block_on(backend.walk_away()), Err(BackendError::NoSession));
    }

    #[test]
    fn start_session_opens_a_playing_mid_life_table() {
        let backend = InProcessBackend::new();
        let view = block_on(backend.start_session()).unwrap();
        assert_eq!(view.status, SessionStatus::Playing);
        // The player arrives at a table that simply is: no events.
        assert!(view.transition.events.is_empty());
        // The buy-in is real chips in the rack.
        assert!(view.rack.total() > 0);
        // Warm-up already happened: the shoe is burned in.
        assert!(view.transition.snapshot.shoe.cards_dealt > 0);
    }

    #[test]
    fn advance_moves_the_table_and_view_does_not() {
        let backend = InProcessBackend::new();
        let opened = block_on(backend.start_session()).unwrap();
        let viewed = block_on(backend.view()).unwrap();
        assert_eq!(opened, viewed);
        // Pump until something happens; the arc always makes progress.
        let mut beats = 0;
        loop {
            beats += 1;
            assert!(beats < 1_000, "advance stopped making progress");
            let view = block_on(backend.advance()).unwrap();
            if !view.transition.events.is_empty() {
                break;
            }
        }
    }

    #[test]
    fn two_sessions_are_different_tables() {
        // OS-random seeds: consecutive sessions must not repeat. (A
        // collision is a 1-in-2^64 event; a deterministic seed leaking
        // in here would fail this test every time.)
        let backend = InProcessBackend::new();
        let first = block_on(backend.start_session()).unwrap();
        let second = block_on(backend.start_session()).unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn start_session_replaces_any_existing_session() {
        let backend = InProcessBackend::new();
        block_on(backend.start_session()).unwrap();
        block_on(backend.advance()).unwrap();
        let fresh = block_on(backend.start_session()).unwrap();
        assert_eq!(fresh, block_on(backend.view()).unwrap());
    }
}
