//! Snapshots: the complete, serializable view of the table that frontends
//! render from.
//!
//! Every [`Table::apply`](crate::Table::apply) call returns a fresh
//! [`Snapshot`]. The snapshot carries everything a UI needs — including
//! the set of legal actions — so frontends never compute rules. Hidden
//! information stays hidden: the dealer's hole card is absent from the
//! snapshot until the engine reveals it.

use serde::{Deserialize, Serialize};

use crate::action::ActionKind;
use crate::card::Card;
use crate::event::HandOutcome;
use crate::rules::Rules;

/// The resting phases of a round, in casino order.
///
/// Dealing, the dealer's blackjack peek, the dealer's draw, and settlement
/// are instantaneous inside [`Table::apply`](crate::Table::apply) — the
/// engine never rests in them — so they are reported through the returned
/// [`Event`](crate::Event) stream rather than as phases. The table only
/// pauses where a human decision is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Phase {
    /// Bets are open; [`Deal`](crate::Action::Deal) starts the round.
    Betting,
    /// The dealer shows an ace; seats with bets are deciding on insurance,
    /// in seat order, before the dealer peeks.
    InsuranceOffer,
    /// Seats play their hands in casino order, seat 0 first.
    PlayerTurn,
    /// The round is settled; [`NextRound`](crate::Action::NextRound)
    /// clears the felt.
    RoundOver,
}

/// Where a hand stands in the life of a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HandStatus {
    /// Awaiting action (or, mid-split, awaiting its second card).
    Playing,
    /// Standing on its total, by choice or automatically.
    Stood,
    /// Over 21.
    Bust,
    /// A natural two-card 21 on an unsplit hand.
    Blackjack,
    /// Surrendered.
    Surrendered,
}

/// A seat's insurance position this round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Insurance {
    /// No offer was made (no dealer ace, no bet, or insurance disabled).
    NotOffered,
    /// The offer is open and this seat has not yet decided.
    Pending,
    /// The seat declined.
    Declined,
    /// The seat bought insurance for `amount` (half the main bet, rounded
    /// down).
    Taken {
        /// The insurance stake in whole dollars.
        amount: u32,
    },
}

/// One player hand as the table sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandSnapshot {
    /// The cards, in the order dealt.
    pub cards: Vec<Card>,
    /// The current bet on the hand (doubled if the hand doubled down).
    pub bet: u32,
    /// Whether the hand doubled down.
    pub doubled: bool,
    /// Whether the hand was created by a split. A two-card 21 on a split
    /// hand is not a blackjack.
    pub from_split: bool,
    /// The hand's status.
    pub status: HandStatus,
    /// The best total of the cards.
    pub total: u8,
    /// Whether an ace currently counts as 11 in `total`.
    pub soft: bool,
    /// How the hand settled, once the round is over.
    pub outcome: Option<HandOutcome>,
    /// Net dollars won or lost on the hand, once the round is over.
    pub payout: Option<i32>,
}

/// One seat as the table sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatSnapshot {
    /// The seat's main bet, or `None` if the seat is sitting out.
    pub bet: Option<u32>,
    /// The seat's insurance position.
    pub insurance: Insurance,
    /// The seat's hands (more than one after splits). Empty before the deal.
    pub hands: Vec<HandSnapshot>,
    /// The seat's net result for the round — every hand plus insurance —
    /// once the round is over.
    pub round_net: Option<i32>,
}

/// The dealer's hand as visible from the table.
///
/// The hole card is `None` until the engine reveals it; a snapshot never
/// contains information a player at the table could not see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealerSnapshot {
    /// The dealer's face-up card, once dealt.
    pub upcard: Option<Card>,
    /// The hole card, present only after it has been revealed.
    pub hole_card: Option<Card>,
    /// Whether a face-down hole card is on the felt.
    pub hole_card_dealt: bool,
    /// Cards drawn after the hole card, in order.
    pub draws: Vec<Card>,
    /// The dealer's best total, known only once the hole card is revealed.
    pub total: Option<u8>,
}

/// Public depth information about the shoe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShoeStatus {
    /// Cards dealt (drawn or burned) since the last shuffle.
    pub cards_dealt: usize,
    /// Undealt cards remaining.
    pub cards_remaining: usize,
    /// Cards in the discard pile.
    pub discard_pile_size: usize,
    /// Whether the cut card has been passed (the next round reshuffles).
    pub cut_card_reached: bool,
}

/// The seat and hand the engine is waiting on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActiveHand {
    /// The active seat.
    pub seat: usize,
    /// The active hand within the seat (always 0 outside player turns).
    pub hand: usize,
}

/// The complete visible state of the table after a transition.
///
/// `legal_actions` is authoritative: it lists exactly the action kinds
/// [`Table::apply`](crate::Table::apply) will accept next, for the active
/// seat (during betting, for any seat). It is never empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// The frozen table rules.
    pub rules: Rules,
    /// The current resting phase.
    pub phase: Phase,
    /// Every seat, in seat order.
    pub seats: Vec<SeatSnapshot>,
    /// The dealer's visible hand.
    pub dealer: DealerSnapshot,
    /// The seat/hand the engine is waiting on, if any.
    pub active: Option<ActiveHand>,
    /// Shoe depth information.
    pub shoe: ShoeStatus,
    /// Exactly the actions the engine will accept next.
    pub legal_actions: Vec<ActionKind>,
}
