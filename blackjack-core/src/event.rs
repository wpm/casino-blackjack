//! Events: the ordered, animatable record of everything that happens at
//! the table.
//!
//! Every [`Table::apply`](crate::Table::apply) call returns the events it
//! caused, in the order they physically happened. Frontends replay them
//! for animation; concatenated across a session they form a complete game
//! log. Events never leak hidden information: the dealer's hole card
//! appears only in [`Event::HoleCardRevealed`].

use serde::{Deserialize, Serialize};

use crate::card::Card;

/// How a settled hand came out, from the player's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HandOutcome {
    /// A natural blackjack, paid at the table's blackjack payout.
    Blackjack,
    /// Beat the dealer; paid 1:1.
    Win,
    /// Tied the dealer; the bet is returned.
    Push,
    /// Lost to the dealer's higher total or blackjack.
    Lose,
    /// Busted; the bet is lost regardless of the dealer's hand.
    Bust,
    /// Surrendered; half the bet (rounded down) is returned.
    Surrender,
}

/// One observable thing that happened at the table.
///
/// `seat` fields are seat indices (`0..Rules::seats`); `hand` fields index
/// into that seat's hands after splits. Settlement `amount`s are the
/// player's net win (positive) or loss (negative) in whole dollars,
/// excluding the returned stake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// A seat placed (or replaced) its main bet.
    BetPlaced {
        /// The betting seat.
        seat: usize,
        /// The bet in whole dollars.
        amount: u32,
    },
    /// The discard pile was shuffled back into the shoe.
    ShoeShuffled,
    /// A card was burned face down after a shuffle.
    CardBurned,
    /// A card was dealt face up to a player hand.
    CardDealt {
        /// The receiving seat.
        seat: usize,
        /// The receiving hand within the seat.
        hand: usize,
        /// The card, face up.
        card: Card,
    },
    /// A card was dealt face up to the dealer (the upcard or a draw).
    DealerCardDealt {
        /// The card, face up.
        card: Card,
    },
    /// The dealer took a hole card face down. The card is not disclosed.
    HoleCardDealt,
    /// The dealer turned over the hole card.
    HoleCardRevealed {
        /// The formerly hidden card.
        card: Card,
    },
    /// A seat was dealt a natural blackjack.
    PlayerBlackjack {
        /// The lucky seat.
        seat: usize,
    },
    /// The dealer shows an ace; insurance is open to every seat with a bet.
    InsuranceOffered,
    /// A seat bought insurance.
    InsuranceTaken {
        /// The insuring seat.
        seat: usize,
        /// The insurance stake: half the main bet, rounded down.
        amount: u32,
    },
    /// A seat declined insurance.
    InsuranceDeclined {
        /// The declining seat.
        seat: usize,
    },
    /// An insurance bet was settled: 2:1 winnings if the dealer had
    /// blackjack, otherwise the stake is lost.
    InsuranceResolved {
        /// The insured seat.
        seat: usize,
        /// Net result: `+2 * stake` on a dealer blackjack, `-stake` otherwise.
        amount: i32,
    },
    /// The dealer checked the hole card for blackjack.
    DealerPeeked {
        /// Whether the dealer has a natural. When true the round ends
        /// immediately; when false play continues and the hole card stays
        /// hidden.
        blackjack: bool,
    },
    /// A pair was split into two hands.
    HandSplit {
        /// The splitting seat.
        seat: usize,
        /// The hand that split; the new hand is inserted directly after it.
        hand: usize,
    },
    /// A hand doubled down.
    DoubledDown {
        /// The doubling seat.
        seat: usize,
        /// The doubled hand.
        hand: usize,
        /// The new total bet on the hand.
        bet: u32,
    },
    /// A hand stood, by choice or automatically (21, or a one-card split ace).
    PlayerStood {
        /// The standing seat.
        seat: usize,
        /// The standing hand.
        hand: usize,
        /// The hand's final total.
        total: u8,
    },
    /// A hand went over 21.
    HandBusted {
        /// The busting seat.
        seat: usize,
        /// The busted hand.
        hand: usize,
        /// The busted total.
        total: u8,
    },
    /// A seat surrendered its hand.
    PlayerSurrendered {
        /// The surrendering seat.
        seat: usize,
    },
    /// The dealer finished drawing and stood.
    DealerStood {
        /// The dealer's final total.
        total: u8,
    },
    /// The dealer busted.
    DealerBust {
        /// The dealer's busted total.
        total: u8,
    },
    /// A hand was settled.
    HandSettled {
        /// The settled seat.
        seat: usize,
        /// The settled hand.
        hand: usize,
        /// How the hand came out.
        outcome: HandOutcome,
        /// Net dollars won (positive) or lost (negative) on the hand.
        amount: i32,
    },
    /// The shoe's cut card has been passed; the next round starts from a
    /// fresh shuffle.
    CutCardReached,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Rank, Suit};

    #[test]
    fn event_serde_round_trips() {
        let events = [
            Event::BetPlaced {
                seat: 2,
                amount: 25,
            },
            Event::CardDealt {
                seat: 0,
                hand: 1,
                card: Card::new(Rank::Ace, Suit::Hearts),
            },
            Event::HoleCardDealt,
            Event::DealerPeeked { blackjack: false },
            Event::HandSettled {
                seat: 3,
                hand: 0,
                outcome: HandOutcome::Blackjack,
                amount: 15,
            },
        ];
        for event in events {
            let json = serde_json::to_string(&event).unwrap();
            let back: Event = serde_json::from_str(&json).unwrap();
            assert_eq!(event, back);
        }
    }
}
