//! The seam between frontends and the blackjack engine.
//!
//! Frontends never touch the engine directly: every interaction goes
//! through the async [`Backend`] trait, and every call answers with a
//! [`SessionView`] — the full new [`Snapshot`](blackjack_core::Snapshot)
//! plus the ordered [`Event`](blackjack_core::Event)s that produced it,
//! the human's chip rack, the dealer's pace, what the session waits on,
//! and where the session stands in its life. The wire types are exactly
//! blackjack-core's serde types (plus this crate's session types),
//! shared by both sides of the boundary so it cannot drift.
//!
//! The session state machine itself — buy-in, warm-up rounds, chip
//! conservation, walk-away, game over — lives here too, as
//! [`SessionArc`]: platform-free, so the Tauri shell and the in-browser
//! dev fallback run the identical arc. The Tauri IPC implementation
//! lives in `blackjack-ui` (wasm only).

mod session;

use std::fmt;

use blackjack_core::{Action, ActionError};
use serde::{Deserialize, Serialize};

pub use session::{SessionArc, SessionStatus, SessionView, buy_in};

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
    /// The human's rack cannot cover the chips the action needs.
    InsufficientChips {
        /// The dollars the action would put on the felt.
        requested: u32,
        /// The dollars in the rack.
        available: u32,
    },
    /// Walking away is only possible between rounds, with no chips on
    /// the felt. A live hand is never abandoned.
    NotBetweenRounds,
    /// The session has ended (cashed out or game over); no further play
    /// is possible. Relaunching the app is the only way to a new one.
    SessionOver,
    /// The call never reached the engine or the reply was unreadable
    /// (IPC failure, malformed payload).
    Transport(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::Rejected(err) => write!(f, "action rejected: {err}"),
            BackendError::NoSession => write!(f, "no session has been started"),
            BackendError::InsufficientChips {
                requested,
                available,
            } => write!(
                f,
                "the rack cannot cover ${requested} (it holds ${available})"
            ),
            BackendError::NotBetweenRounds => {
                write!(f, "walking away is only possible between rounds")
            }
            BackendError::SessionOver => write!(f, "the session is over"),
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

/// Wire shape of the `human_action` call's arguments.
///
/// Both sides of an IPC boundary use this struct — the field names are the
/// argument names on the wire — so the argument shape cannot drift. There
/// is no seat argument: the backend owns the human's seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanActionArgs {
    /// The action the human performs.
    pub action: Action,
}

/// The frontend's only way to reach the engine.
///
/// A session is one [`SessionArc`] — a living table with one human seat,
/// a rack of real chips, and a beginning and an end; who backs it (a
/// Tauri process over IPC today, an in-process WASM engine later) is
/// invisible to callers. Every method resolves to a [`SessionView`];
/// calls that change nothing ([`start_session`](Backend::start_session),
/// [`view`](Backend::view)) carry an empty event list.
///
/// The driving contract mirrors [`TableLife`](blackjack_core::TableLife):
/// while a view's `awaiting` is [`Awaiting::Engine`](
/// blackjack_core::Awaiting::Engine) and its `status` is
/// [`SessionStatus::Playing`], pump [`advance`](Backend::advance) and
/// animate each returned view at its pace; otherwise rest until the human
/// acts through [`human_action`](Backend::human_action) (or
/// [`walk_away`](Backend::walk_away) between rounds).
///
/// On wasm the returned futures are not `Send` (they await JavaScript
/// promises), so this trait deliberately puts no `Send` bound on them;
/// drive it with a local spawner such as `spawn_local`. It is consequently
/// not dyn-compatible — consumers should be generic over `B: Backend`.
#[allow(async_fn_in_trait)]
pub trait Backend {
    /// Start a fresh session, replacing any existing one: a random
    /// buy-in, a table already several rounds into its life, the human
    /// seat open. Events are empty — the player arrives at a table that
    /// simply is.
    async fn start_session(&self) -> Result<SessionView, BackendError>;

    /// The current state, changing nothing (events empty).
    async fn view(&self) -> Result<SessionView, BackendError>;

    /// One engine beat ([`TableLife::advance`](
    /// blackjack_core::TableLife::advance)): an AI bet, an AI decision,
    /// the deal, settlement, between-round housekeeping. Harmless while
    /// the session waits on the human or is over (an empty view comes
    /// back).
    async fn advance(&self) -> Result<SessionView, BackendError>;

    /// Submit one action for the human seat. Legality is re-gated
    /// server-side: the engine validates the action and the rack must
    /// cover any chips it commits.
    async fn human_action(&self, action: Action) -> Result<SessionView, BackendError>;

    /// Leave the table. Valid only between rounds with no chips on the
    /// felt: the seat empties, the rack colors up, and the returned
    /// view's status is [`SessionStatus::CashedOut`] with the dollars
    /// walked away with.
    async fn walk_away(&self) -> Result<SessionView, BackendError>;
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
        assert_eq!(
            BackendError::InsufficientChips {
                requested: 500,
                available: 240,
            }
            .to_string(),
            "the rack cannot cover $500 (it holds $240)"
        );
        assert_eq!(
            BackendError::NotBetweenRounds.to_string(),
            "walking away is only possible between rounds"
        );
        assert_eq!(BackendError::SessionOver.to_string(), "the session is over");
    }

    #[test]
    fn backend_error_serde_round_trips() {
        let errors = [
            BackendError::Rejected(ActionError::NoBetsPlaced),
            BackendError::NoSession,
            BackendError::InsufficientChips {
                requested: 100,
                available: 40,
            },
            BackendError::NotBetweenRounds,
            BackendError::SessionOver,
            BackendError::Transport("ipc down".into()),
        ];
        for err in errors {
            let json = serde_json::to_string(&err).unwrap();
            let back: BackendError = serde_json::from_str(&json).unwrap();
            assert_eq!(err, back);
        }
    }

    #[test]
    fn human_action_args_serialize_with_stable_field_names() {
        let args = HumanActionArgs {
            action: Action::PlaceBet(25),
        };
        let json = serde_json::to_value(args).unwrap();
        assert_eq!(json, serde_json::json!({ "action": { "PlaceBet": 25 } }));
    }

    #[test]
    fn action_errors_convert_into_backend_errors() {
        let err: BackendError = ActionError::NoBetsPlaced.into();
        assert_eq!(err, BackendError::Rejected(ActionError::NoBetsPlaced));
    }
}
