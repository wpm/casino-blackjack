//! Hand evaluation: best totals, soft/hard classification, and the
//! two-card predicates (blackjack, rank pair, value pair).

use crate::card::{Card, Rank};

/// The blackjack value of a set of cards.
///
/// Computed with [`HandValue::of`]. The value is the *best* legal reading of
/// the hand: aces count as 11 wherever that does not bust the hand, otherwise
/// as 1. A hand is *soft* when an ace is currently counted as 11 (so the hand
/// cannot bust by taking one more card); otherwise it is *hard*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandValue {
    total: u8,
    soft: bool,
}

impl HandValue {
    /// Evaluate the cards of a hand.
    ///
    /// An empty slice evaluates to a hard 0.
    pub fn of(cards: &[Card]) -> HandValue {
        let mut total: u32 = 0;
        let mut aces = 0u32;
        for card in cards {
            // Count every ace as 1 for now; promote one to 11 below if legal.
            if card.rank.is_ace() {
                aces += 1;
                total += 1;
            } else {
                total += u32::from(card.value());
            }
        }
        // At most one ace can count as 11 (two would already total 22).
        if aces > 0 && total + 10 <= 21 {
            HandValue {
                total: (total + 10) as u8,
                soft: true,
            }
        } else {
            HandValue {
                total: total.min(u32::from(u8::MAX)) as u8,
                soft: false,
            }
        }
    }

    /// The best total for the hand: the highest legal reading, or the
    /// minimum reading when every reading busts.
    pub fn total(self) -> u8 {
        self.total
    }

    /// True when an ace is counted as 11 in [`HandValue::total`].
    pub fn is_soft(self) -> bool {
        self.soft
    }

    /// True when even the best total exceeds 21.
    pub fn is_bust(self) -> bool {
        self.total > 21
    }
}

/// True when the cards are a *two-card* 21: an ace plus a ten-value card.
///
/// This is the primitive "natural" predicate only. Whether a two-card 21
/// after a split still pays as blackjack is a table-rules question decided by
/// higher layers; this function knows nothing about splits.
pub fn is_blackjack(cards: &[Card]) -> bool {
    cards.len() == 2 && HandValue::of(cards).total() == 21
}

/// True when the hand is exactly two cards of the same [`Rank`].
///
/// Under split-by-rank rules a king and a queen are *not* a pair; see
/// [`is_value_pair`] for the looser split-by-value predicate.
pub fn is_rank_pair(cards: &[Card]) -> bool {
    matches!(cards, [a, b] if a.rank == b.rank)
}

/// True when the hand is exactly two cards of the same blackjack value.
///
/// Under split-by-value rules any two ten-value cards — for example a king
/// and a queen — form a splittable pair. Every rank pair is also a value
/// pair.
pub fn is_value_pair(cards: &[Card]) -> bool {
    matches!(cards, [a, b] if a.value() == b.value())
}

/// True when the hand is a pair of aces (always splittable, and the usual
/// subject of special resplit/hit restrictions).
pub fn is_ace_pair(cards: &[Card]) -> bool {
    matches!(cards, [a, b] if a.rank == Rank::Ace && b.rank == Rank::Ace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Rank, Suit};

    /// Build a hand from ranks alone, cycling suits so tests stay terse.
    fn hand(ranks: &[Rank]) -> Vec<Card> {
        ranks
            .iter()
            .zip(Suit::ALL.iter().cycle())
            .map(|(&rank, &suit)| Card::new(rank, suit))
            .collect()
    }

    fn value(ranks: &[Rank]) -> HandValue {
        HandValue::of(&hand(ranks))
    }

    #[test]
    fn empty_hand_is_hard_zero() {
        let v = value(&[]);
        assert_eq!(v.total(), 0);
        assert!(!v.is_soft());
        assert!(!v.is_bust());
    }

    #[test]
    fn hard_totals_without_aces() {
        let v = value(&[Rank::Ten, Rank::Seven]);
        assert_eq!(v.total(), 17);
        assert!(!v.is_soft());
        assert!(!v.is_bust());
    }

    #[test]
    fn single_ace_hand_is_soft() {
        let v = value(&[Rank::Ace, Rank::Six]);
        assert_eq!(v.total(), 17);
        assert!(v.is_soft());
    }

    #[test]
    fn ace_pair_is_soft_twelve() {
        let v = value(&[Rank::Ace, Rank::Ace]);
        assert_eq!(v.total(), 12);
        assert!(v.is_soft());
    }

    #[test]
    fn ace_ace_nine_is_soft_twenty_one() {
        // A + A + 9 = 1 + 11 + 9: one ace still counts as 11, so soft 21.
        let v = value(&[Rank::Ace, Rank::Ace, Rank::Nine]);
        assert_eq!(v.total(), 21);
        assert!(v.is_soft());
    }

    #[test]
    fn ace_demotes_to_one_when_eleven_would_bust() {
        let v = value(&[Rank::Ace, Rank::Six, Rank::Ten]);
        assert_eq!(v.total(), 17);
        assert!(!v.is_soft());
    }

    #[test]
    fn many_aces_use_at_most_one_eleven() {
        // A + A + A + A = 14 (11 + 1 + 1 + 1), soft.
        let v = value(&[Rank::Ace, Rank::Ace, Rank::Ace, Rank::Ace]);
        assert_eq!(v.total(), 14);
        assert!(v.is_soft());

        // A + A + A + 9 = 12 hard: promoting any ace would bust.
        let v = value(&[Rank::Ace, Rank::Ace, Rank::Ace, Rank::Nine]);
        assert_eq!(v.total(), 12);
        assert!(!v.is_soft());
    }

    #[test]
    fn bust_hands_report_minimum_total() {
        let v = value(&[Rank::Ten, Rank::Nine, Rank::Five]);
        assert_eq!(v.total(), 24);
        assert!(v.is_bust());
        assert!(!v.is_soft());

        // Aces all count as 1 in a bust hand.
        let v = value(&[Rank::Ten, Rank::Nine, Rank::Five, Rank::Ace]);
        assert_eq!(v.total(), 25);
        assert!(v.is_bust());
    }

    #[test]
    fn twenty_one_at_exactly_two_cards_is_blackjack() {
        assert!(is_blackjack(&hand(&[Rank::Ace, Rank::King])));
        assert!(is_blackjack(&hand(&[Rank::Ten, Rank::Ace])));
    }

    #[test]
    fn twenty_one_in_three_cards_is_not_blackjack() {
        let cards = hand(&[Rank::Seven, Rank::Seven, Rank::Seven]);
        assert_eq!(HandValue::of(&cards).total(), 21);
        assert!(!is_blackjack(&cards));
    }

    #[test]
    fn two_card_non_twenty_one_is_not_blackjack() {
        assert!(!is_blackjack(&hand(&[Rank::Ten, Rank::Ten])));
        assert!(!is_blackjack(&hand(&[Rank::Ace, Rank::Nine])));
    }

    #[test]
    fn rank_pairs_require_identical_ranks() {
        assert!(is_rank_pair(&hand(&[Rank::Eight, Rank::Eight])));
        assert!(is_rank_pair(&hand(&[Rank::Ace, Rank::Ace])));
        assert!(!is_rank_pair(&hand(&[Rank::King, Rank::Queen])));
        assert!(!is_rank_pair(&hand(&[Rank::Eight, Rank::Nine])));
    }

    #[test]
    fn mixed_ten_value_cards_are_a_value_pair_but_not_a_rank_pair() {
        let king_queen = hand(&[Rank::King, Rank::Queen]);
        assert!(!is_rank_pair(&king_queen));
        assert!(is_value_pair(&king_queen));

        let ten_jack = hand(&[Rank::Ten, Rank::Jack]);
        assert!(!is_rank_pair(&ten_jack));
        assert!(is_value_pair(&ten_jack));
    }

    #[test]
    fn every_rank_pair_is_a_value_pair() {
        for rank in Rank::ALL {
            let cards = hand(&[rank, rank]);
            assert!(is_rank_pair(&cards));
            assert!(is_value_pair(&cards));
        }
    }

    #[test]
    fn pair_predicates_reject_non_two_card_hands() {
        assert!(!is_rank_pair(&hand(&[Rank::Eight])));
        assert!(!is_rank_pair(&hand(&[
            Rank::Eight,
            Rank::Eight,
            Rank::Eight
        ])));
        assert!(!is_value_pair(&hand(&[Rank::King])));
        assert!(!is_value_pair(&hand(&[Rank::King, Rank::Queen, Rank::Ten])));
    }

    #[test]
    fn ace_pair_predicate_matches_only_two_aces() {
        assert!(is_ace_pair(&hand(&[Rank::Ace, Rank::Ace])));
        assert!(!is_ace_pair(&hand(&[Rank::Ace, Rank::King])));
        assert!(!is_ace_pair(&hand(&[Rank::Ace, Rank::Ace, Rank::Ace])));
    }
}
