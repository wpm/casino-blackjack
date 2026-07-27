//! Pure casino blackjack rules engine.
//!
//! This crate has no UI or platform dependencies. All game state lives here;
//! frontends consume snapshots and never compute rules themselves.

mod action;
mod card;
mod event;
mod hand;
mod rules;
mod shoe;
mod snapshot;
mod table;

pub use action::{Action, ActionError, ActionKind};
pub use card::{Card, Rank, Suit};
pub use event::{Event, HandOutcome};
pub use hand::{HandValue, is_ace_pair, is_blackjack, is_rank_pair, is_value_pair};
pub use rules::{BlackjackPayout, Rules, Soft17};
pub use shoe::{DECK_SIZE, Shoe};
pub use snapshot::{
    ActiveHand, DealerSnapshot, HandSnapshot, HandStatus, Insurance, Phase, SeatSnapshot,
    ShoeStatus, Snapshot,
};
pub use table::{Table, Transition};

mod chips;

pub use chips::{ChipColor, ChipError, ChipStack, Denomination, Rack, payout_chips};

/// Engine version, exposed so shells can report what they embed.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_version_matches_crate() {
        assert_eq!(ENGINE_VERSION, env!("CARGO_PKG_VERSION"));
    }
}
