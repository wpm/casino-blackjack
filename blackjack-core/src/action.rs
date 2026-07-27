//! Player and table actions, and the typed errors returned when an action
//! is illegal.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::snapshot::Phase;

/// Everything a player (or the table) can ask the engine to do.
///
/// Every state transition goes through
/// [`Table::apply`](crate::Table::apply) with one of these. Table-level
/// actions ([`Deal`](Action::Deal) and [`NextRound`](Action::NextRound))
/// may be issued with any valid seat index; all other actions must come
/// from the seat the engine says is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    /// Place (or replace) this seat's main bet for the coming round, in
    /// whole dollars. Legal only during [`Phase::Betting`].
    PlaceBet(u32),
    /// Close betting and deal the round. Legal once at least one seat has
    /// a bet.
    Deal,
    /// Take one more card on the active hand.
    Hit,
    /// Stand on the active hand.
    Stand,
    /// Double the bet on the active two-card hand, take exactly one more
    /// card, and stand.
    Double,
    /// Split the active two-card pair into two hands.
    Split,
    /// Late surrender: forfeit the hand and recover half the bet.
    Surrender,
    /// Buy insurance for half the main bet (rounded down).
    TakeInsurance,
    /// Decline the insurance offer.
    DeclineInsurance,
    /// Clear the finished round and return to betting, reshuffling if the
    /// cut card was reached. Legal only during [`Phase::RoundOver`].
    NextRound,
}

impl Action {
    /// The payload-free kind of this action, as used in legal-action sets.
    pub fn kind(self) -> ActionKind {
        match self {
            Action::PlaceBet(_) => ActionKind::PlaceBet,
            Action::Deal => ActionKind::Deal,
            Action::Hit => ActionKind::Hit,
            Action::Stand => ActionKind::Stand,
            Action::Double => ActionKind::Double,
            Action::Split => ActionKind::Split,
            Action::Surrender => ActionKind::Surrender,
            Action::TakeInsurance => ActionKind::TakeInsurance,
            Action::DeclineInsurance => ActionKind::DeclineInsurance,
            Action::NextRound => ActionKind::NextRound,
        }
    }
}

/// An [`Action`] stripped of its payload, used to describe which actions
/// are legal and to report errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    /// See [`Action::PlaceBet`].
    PlaceBet,
    /// See [`Action::Deal`].
    Deal,
    /// See [`Action::Hit`].
    Hit,
    /// See [`Action::Stand`].
    Stand,
    /// See [`Action::Double`].
    Double,
    /// See [`Action::Split`].
    Split,
    /// See [`Action::Surrender`].
    Surrender,
    /// See [`Action::TakeInsurance`].
    TakeInsurance,
    /// See [`Action::DeclineInsurance`].
    DeclineInsurance,
    /// See [`Action::NextRound`].
    NextRound,
}

/// Why an action was rejected.
///
/// An illegal action never mutates the table: the caller gets one of these
/// and the state is exactly as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionError {
    /// The seat index is not on this table.
    SeatOutOfRange {
        /// The offending seat index.
        seat: usize,
        /// Number of seats at the table.
        seats: usize,
    },
    /// The action is not valid in the current phase.
    WrongPhase {
        /// The attempted action.
        action: ActionKind,
        /// The phase the table is in.
        phase: Phase,
    },
    /// The action came from a seat other than the active one.
    OutOfTurn {
        /// The seat that acted.
        seat: usize,
        /// The seat whose turn it is.
        active_seat: usize,
    },
    /// The bet is outside the table limits.
    BetOutOfRange {
        /// The attempted bet.
        bet: u32,
        /// Table minimum.
        min: u32,
        /// Table maximum.
        max: u32,
    },
    /// [`Action::Deal`] with no bets on the felt.
    NoBetsPlaced,
    /// Doubling is not allowed on this hand under the table rules.
    DoubleNotAllowed,
    /// Splitting is not allowed on this hand under the table rules.
    SplitNotAllowed,
    /// Surrender is not allowed on this hand under the table rules.
    SurrenderNotAllowed,
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::SeatOutOfRange { seat, seats } => {
                write!(f, "seat {seat} is out of range for a {seats}-seat table")
            }
            ActionError::WrongPhase { action, phase } => {
                write!(f, "{action:?} is not legal during the {phase:?} phase")
            }
            ActionError::OutOfTurn { seat, active_seat } => {
                write!(f, "seat {seat} acted but seat {active_seat} is active")
            }
            ActionError::BetOutOfRange { bet, min, max } => {
                write!(f, "bet ${bet} is outside the table limits ${min}-${max}")
            }
            ActionError::NoBetsPlaced => write!(f, "cannot deal with no bets placed"),
            ActionError::DoubleNotAllowed => write!(f, "doubling is not allowed on this hand"),
            ActionError::SplitNotAllowed => write!(f, "splitting is not allowed on this hand"),
            ActionError::SurrenderNotAllowed => write!(f, "surrender is not allowed on this hand"),
        }
    }
}

impl std::error::Error for ActionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_maps_to_its_kind() {
        let pairs = [
            (Action::PlaceBet(10), ActionKind::PlaceBet),
            (Action::Deal, ActionKind::Deal),
            (Action::Hit, ActionKind::Hit),
            (Action::Stand, ActionKind::Stand),
            (Action::Double, ActionKind::Double),
            (Action::Split, ActionKind::Split),
            (Action::Surrender, ActionKind::Surrender),
            (Action::TakeInsurance, ActionKind::TakeInsurance),
            (Action::DeclineInsurance, ActionKind::DeclineInsurance),
            (Action::NextRound, ActionKind::NextRound),
        ];
        for (action, kind) in pairs {
            assert_eq!(action.kind(), kind);
        }
    }

    #[test]
    fn action_serde_round_trips() {
        let action = Action::PlaceBet(25);
        let json = serde_json::to_string(&action).unwrap();
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(action, back);
    }

    #[test]
    fn errors_render_useful_messages() {
        let err = ActionError::BetOutOfRange {
            bet: 5,
            min: 10,
            max: 500,
        };
        assert_eq!(
            err.to_string(),
            "bet $5 is outside the table limits $10-$500"
        );
    }
}
