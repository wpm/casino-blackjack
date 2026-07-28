//! Playing-card primitives: [`Suit`], [`Rank`], and [`Card`].

use serde::{Deserialize, Serialize};

/// One of the four French suits.
///
/// Suits never affect blackjack outcomes; they exist so frontends can render
/// real cards and so a shoe holds distinguishable, countable cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}

impl Suit {
    /// All four suits, in ascending [`Ord`] order.
    pub const ALL: [Suit; 4] = [Suit::Clubs, Suit::Diamonds, Suit::Hearts, Suit::Spades];
}

/// A card rank, `Two` through `Ace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rank {
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
    Ace,
}

impl Rank {
    /// All thirteen ranks, in ascending [`Ord`] order.
    pub const ALL: [Rank; 13] = [
        Rank::Two,
        Rank::Three,
        Rank::Four,
        Rank::Five,
        Rank::Six,
        Rank::Seven,
        Rank::Eight,
        Rank::Nine,
        Rank::Ten,
        Rank::Jack,
        Rank::Queen,
        Rank::King,
        Rank::Ace,
    ];

    /// The blackjack value of this rank, counting an ace as 11.
    ///
    /// Hand-value logic is responsible for demoting aces from 11 to 1 as
    /// needed; see [`crate::HandValue`].
    pub fn value(self) -> u8 {
        match self {
            Rank::Two => 2,
            Rank::Three => 3,
            Rank::Four => 4,
            Rank::Five => 5,
            Rank::Six => 6,
            Rank::Seven => 7,
            Rank::Eight => 8,
            Rank::Nine => 9,
            Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => 10,
            Rank::Ace => 11,
        }
    }

    /// True for the four ten-value ranks: ten, jack, queen, and king.
    pub fn is_ten_value(self) -> bool {
        self.value() == 10
    }

    /// True only for the ace.
    pub fn is_ace(self) -> bool {
        self == Rank::Ace
    }
}

/// A single playing card: a [`Rank`] and a [`Suit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Card {
    /// The card's rank.
    pub rank: Rank,
    /// The card's suit.
    pub suit: Suit,
}

impl Card {
    /// Construct a card from a rank and suit.
    pub fn new(rank: Rank, suit: Suit) -> Card {
        Card { rank, suit }
    }

    /// The blackjack value of this card, counting an ace as 11.
    pub fn value(self) -> u8 {
        self.rank.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_values_follow_blackjack_rules() {
        assert_eq!(Rank::Two.value(), 2);
        assert_eq!(Rank::Nine.value(), 9);
        assert_eq!(Rank::Ten.value(), 10);
        assert_eq!(Rank::Jack.value(), 10);
        assert_eq!(Rank::Queen.value(), 10);
        assert_eq!(Rank::King.value(), 10);
        assert_eq!(Rank::Ace.value(), 11);
    }

    #[test]
    fn ten_value_ranks_are_exactly_the_four_broadway_tens() {
        let tens: Vec<Rank> = Rank::ALL.into_iter().filter(|r| r.is_ten_value()).collect();
        assert_eq!(tens, [Rank::Ten, Rank::Jack, Rank::Queen, Rank::King]);
    }

    #[test]
    fn only_the_ace_is_an_ace() {
        let aces: Vec<Rank> = Rank::ALL.into_iter().filter(|r| r.is_ace()).collect();
        assert_eq!(aces, [Rank::Ace]);
    }

    #[test]
    fn card_serde_round_trips() {
        let card = Card::new(Rank::Ace, Suit::Spades);
        let json = serde_json::to_string(&card).unwrap();
        let back: Card = serde_json::from_str(&json).unwrap();
        assert_eq!(card, back);
    }
}
