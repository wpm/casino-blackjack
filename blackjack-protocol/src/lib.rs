//! The seam between frontends and the blackjack engine.
//!
//! Frontends never touch [`Table`](blackjack_core::Table) directly: every
//! interaction goes through the async [`Backend`] trait, and every call
//! answers with a [`Transition`] — the full new [`Snapshot`](
//! blackjack_core::Snapshot) plus the ordered [`Event`](
//! blackjack_core::Event)s that produced it. The wire types are exactly
//! blackjack-core's serde types, shared by both sides of the boundary so
//! it cannot drift.
//!
//! This crate is platform-free. The Tauri IPC implementation lives in
//! `blackjack-ui` (wasm only); an in-process WASM implementation can
//! implement the same trait without touching this crate.

use std::fmt;

use blackjack_core::{Action, ActionError, Transition};
use serde::{Deserialize, Serialize};

/// Why a [`Backend`] call failed.
///
/// Serializable so backends behind an IPC boundary can send it across the
/// wire verbatim; the frontend sees the same enum either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendError {
    /// The engine rejected the action; the table is unchanged.
    Rejected(ActionError),
    /// No session has been started yet.
    NoSession,
    /// The call never reached the engine or the reply was unreadable
    /// (IPC failure, malformed payload).
    Transport(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Rejected(err) => write!(f, "action rejected: {err}"),
            BackendError::NoSession => write!(f, "no session has been started"),
            BackendError::Transport(msg) => write!(f, "backend transport error: {msg}"),
        }
    }
}

impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackendError::Rejected(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ActionError> for BackendError {
    fn from(err: ActionError) -> BackendError {
        BackendError::Rejected(err)
    }
}

/// Wire shape of the `submit_action` call's arguments.
///
/// Both sides of an IPC boundary use this struct — the field names are the
/// argument names on the wire — so the argument shape cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitActionArgs {
    /// The acting seat.
    pub seat: usize,
    /// The action to perform.
    pub action: Action,
}

/// The frontend's only way to reach the engine.
///
/// A session is one [`Table`](blackjack_core::Table) playing rounds under
/// one rule set; who backs it (a Tauri process over IPC today, an
/// in-process WASM engine or a richer session arc later) is invisible to
/// callers — the surface is snapshot/transition-shaped, never
/// table-shaped. Every method resolves to a [`Transition`]; calls that
/// change nothing ([`start_session`](Backend::start_session),
/// [`snapshot`](Backend::snapshot)) carry an empty event list.
///
/// On wasm the returned futures are not `Send` (they await JavaScript
/// promises), so this trait deliberately puts no `Send` bound on them;
/// drive it with a local spawner such as `spawn_local`. It is consequently
/// not dyn-compatible — consumers should be generic over `B: Backend`.
#[allow(async_fn_in_trait)]
pub trait Backend {
    /// Start a fresh session, replacing any existing one, and return its
    /// opening state (a table in the betting phase; events empty).
    async fn start_session(&self) -> Result<Transition, BackendError>;

    /// The current state, changing nothing (events empty).
    async fn snapshot(&self) -> Result<Transition, BackendError>;

    /// Submit one action for one seat, exactly as
    /// [`Table::apply`](blackjack_core::Table::apply): on success the
    /// table advances; on rejection it is untouched.
    async fn submit_action(&self, seat: usize, action: Action) -> Result<Transition, BackendError>;

    /// Clear a finished round and open betting for the next one.
    ///
    /// [`Action::NextRound`] is seat-agnostic in the engine, so the
    /// default implementation submits it for seat 0.
    async fn next_round(&self) -> Result<Transition, BackendError> {
        self.submit_action(0, Action::NextRound).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackjack_core::Phase;

    #[test]
    fn backend_errors_render_useful_messages() {
        let err = BackendError::Rejected(ActionError::WrongPhase {
            action: blackjack_core::ActionKind::Hit,
            phase: Phase::Betting,
        });
        assert_eq!(
            err.to_string(),
            "action rejected: Hit is not legal during the Betting phase"
        );
        assert_eq!(
            BackendError::NoSession.to_string(),
            "no session has been started"
        );
    }

    #[test]
    fn backend_error_serde_round_trips() {
        let errors = [
            BackendError::Rejected(ActionError::NoBetsPlaced),
            BackendError::NoSession,
            BackendError::Transport("ipc down".into()),
        ];
        for err in errors {
            let json = serde_json::to_string(&err).unwrap();
            let back: BackendError = serde_json::from_str(&json).unwrap();
            assert_eq!(err, back);
        }
    }

    #[test]
    fn submit_action_args_serialize_with_stable_field_names() {
        let args = SubmitActionArgs {
            seat: 2,
            action: Action::PlaceBet(25),
        };
        let json = serde_json::to_value(args).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "seat": 2, "action": { "PlaceBet": 25 } })
        );
    }

    #[test]
    fn action_errors_convert_into_backend_errors() {
        let err: BackendError = ActionError::NoBetsPlaced.into();
        assert_eq!(err, BackendError::Rejected(ActionError::NoBetsPlaced));
    }
}
