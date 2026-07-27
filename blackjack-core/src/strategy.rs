//! Basic strategy for the canonical table: six decks, dealer stands on
//! soft 17, double after split, late surrender.
//!
//! The table is encoded as data — match arms over (hand, upcard) cells —
//! and resolved against what the current hand may legally do, so the
//! returned action is always playable: when a cell says "double else hit"
//! and doubling is unavailable, the fallback is returned. Basic strategy
//! never takes insurance; callers offering insurance should decline.

use crate::action::ActionKind;
use crate::card::Card;
use crate::hand::{HandValue, is_value_pair};

/// The basic-strategy action for a hand against a dealer upcard, under the
/// canonical rules ([`Rules::canonical`](crate::Rules::canonical)).
///
/// `can_double`, `can_split`, and `can_surrender` say what the table will
/// legally accept for this hand right now (see
/// [`Snapshot::legal_actions`](crate::Snapshot::legal_actions)); cells
/// whose first choice is unavailable resolve to their fallback. The result
/// is always one of `Hit`, `Stand`, `Double`, `Split`, or `Surrender`, and
/// never an unavailable one.
pub fn basic_strategy(
    cards: &[Card],
    upcard: Card,
    can_double: bool,
    can_split: bool,
    can_surrender: bool,
) -> ActionKind {
    let up = upcard.value(); // 2..=11, ace counted as 11.

    // Pairs first. Splitting is by blackjack value, matching the engine:
    // any two ten-value cards are a (never split) pair.
    if can_split && is_value_pair(cards) {
        let split = match cards[0].value() {
            // Aces and eights, always.
            11 | 8 => true,
            // Tens stand pat; fives play as a hard 10.
            10 | 5 => false,
            9 => matches!(up, 2..=6 | 8 | 9),
            7 => (2..=7).contains(&up),
            6 => (2..=6).contains(&up),
            // 4-4 splits only where double-after-split makes it worthwhile.
            4 => matches!(up, 5 | 6),
            2 | 3 => (2..=7).contains(&up),
            _ => false,
        };
        if split {
            return ActionKind::Split;
        }
    }

    let value = HandValue::of(cards);
    let total = value.total();

    // Late surrender: hard 16 against 9, ten, or ace; hard 15 against ten.
    // (Eights against those cards were already split above.)
    if can_surrender
        && !value.is_soft()
        && ((total == 16 && matches!(up, 9..=11)) || (total == 15 && up == 10))
    {
        return ActionKind::Surrender;
    }

    if value.is_soft() {
        return match total {
            19..=21 => ActionKind::Stand,
            18 => match up {
                3..=6 if can_double => ActionKind::Double,
                2..=8 => ActionKind::Stand,
                _ => ActionKind::Hit,
            },
            17 if (3..=6).contains(&up) && can_double => ActionKind::Double,
            15 | 16 if (4..=6).contains(&up) && can_double => ActionKind::Double,
            13 | 14 if (5..=6).contains(&up) && can_double => ActionKind::Double,
            _ => ActionKind::Hit,
        };
    }

    match total {
        17.. => ActionKind::Stand,
        13..=16 if (2..=6).contains(&up) => ActionKind::Stand,
        12 if (4..=6).contains(&up) => ActionKind::Stand,
        11 if can_double => ActionKind::Double,
        10 if up <= 9 && can_double => ActionKind::Double,
        9 if (3..=6).contains(&up) && can_double => ActionKind::Double,
        _ => ActionKind::Hit,
    }
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

    fn up(rank: Rank) -> Card {
        Card::new(rank, Suit::Spades)
    }

    /// The full first-decision context: double, split, and surrender all
    /// available, as on any fresh two-card hand.
    fn first_two(cards: &[Card], upcard: Rank) -> ActionKind {
        basic_strategy(cards, up(upcard), true, true, true)
    }

    #[test]
    fn sixteen_vs_ten_surrenders_else_hits() {
        let cards = hand(&[Rank::Ten, Rank::Six]);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Surrender);
        assert_eq!(first_two(&cards, Rank::Nine), ActionKind::Surrender);
        assert_eq!(first_two(&cards, Rank::Ace), ActionKind::Surrender);
        // After a hit (or on a split hand) surrender is gone: hit.
        assert_eq!(
            basic_strategy(&cards, up(Rank::Ten), true, true, false),
            ActionKind::Hit
        );
    }

    #[test]
    fn fifteen_surrenders_only_against_ten() {
        let cards = hand(&[Rank::Ten, Rank::Five]);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Surrender);
        assert_eq!(first_two(&cards, Rank::Nine), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Ace), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Six), ActionKind::Stand);
    }

    #[test]
    fn soft_sixteen_never_surrenders() {
        let cards = hand(&[Rank::Ace, Rank::Five]);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Hit);
    }

    #[test]
    fn eleven_doubles_against_everything_including_ace() {
        let cards = hand(&[Rank::Six, Rank::Five]);
        for upcard in Rank::ALL {
            assert_eq!(first_two(&cards, upcard), ActionKind::Double);
        }
        // Three cards to 11 cannot double: hit.
        let three = hand(&[Rank::Two, Rank::Four, Rank::Five]);
        assert_eq!(
            basic_strategy(&three, up(Rank::Ace), false, false, false),
            ActionKind::Hit
        );
    }

    #[test]
    fn ten_doubles_through_nine_and_hits_the_rest() {
        let cards = hand(&[Rank::Six, Rank::Four]);
        assert_eq!(first_two(&cards, Rank::Nine), ActionKind::Double);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Ace), ActionKind::Hit);
    }

    #[test]
    fn nine_doubles_against_three_through_six() {
        let cards = hand(&[Rank::Five, Rank::Four]);
        assert_eq!(first_two(&cards, Rank::Two), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Three), ActionKind::Double);
        assert_eq!(first_two(&cards, Rank::Six), ActionKind::Double);
        assert_eq!(first_two(&cards, Rank::Seven), ActionKind::Hit);
    }

    #[test]
    fn soft_nineteen_stands_even_against_six() {
        // A-8 vs 6: stand under S17 basic strategy.
        let cards = hand(&[Rank::Ace, Rank::Eight]);
        assert_eq!(first_two(&cards, Rank::Six), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Stand);
    }

    #[test]
    fn soft_eighteen_cells() {
        let cards = hand(&[Rank::Ace, Rank::Seven]);
        assert_eq!(first_two(&cards, Rank::Two), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Three), ActionKind::Double);
        assert_eq!(first_two(&cards, Rank::Six), ActionKind::Double);
        assert_eq!(first_two(&cards, Rank::Seven), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Eight), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Nine), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Ace), ActionKind::Hit);
        // When doubling is unavailable the 3-6 cells fall back to stand.
        assert_eq!(
            basic_strategy(&cards, up(Rank::Four), false, false, false),
            ActionKind::Stand
        );
    }

    #[test]
    fn soft_seventeen_and_below_double_or_hit() {
        assert_eq!(
            first_two(&hand(&[Rank::Ace, Rank::Six]), Rank::Three),
            ActionKind::Double
        );
        assert_eq!(
            first_two(&hand(&[Rank::Ace, Rank::Six]), Rank::Two),
            ActionKind::Hit
        );
        assert_eq!(
            first_two(&hand(&[Rank::Ace, Rank::Four]), Rank::Four),
            ActionKind::Double
        );
        assert_eq!(
            first_two(&hand(&[Rank::Ace, Rank::Two]), Rank::Five),
            ActionKind::Double
        );
        assert_eq!(
            first_two(&hand(&[Rank::Ace, Rank::Two]), Rank::Four),
            ActionKind::Hit
        );
    }

    #[test]
    fn eights_split_against_everything() {
        let cards = hand(&[Rank::Eight, Rank::Eight]);
        for upcard in Rank::ALL {
            assert_eq!(first_two(&cards, upcard), ActionKind::Split);
        }
        // With splitting exhausted, 16 vs ten surrenders (else hits).
        assert_eq!(
            basic_strategy(&cards, up(Rank::Ten), true, false, true),
            ActionKind::Surrender
        );
        assert_eq!(
            basic_strategy(&cards, up(Rank::Ten), false, false, false),
            ActionKind::Hit
        );
    }

    #[test]
    fn tens_never_split() {
        for cards in [
            hand(&[Rank::Ten, Rank::Ten]),
            hand(&[Rank::King, Rank::Queen]),
        ] {
            for upcard in Rank::ALL {
                assert_eq!(first_two(&cards, upcard), ActionKind::Stand);
            }
        }
    }

    #[test]
    fn aces_always_split_and_play_soft_twelve_otherwise() {
        let cards = hand(&[Rank::Ace, Rank::Ace]);
        for upcard in Rank::ALL {
            assert_eq!(first_two(&cards, upcard), ActionKind::Split);
        }
        assert_eq!(
            basic_strategy(&cards, up(Rank::Six), false, false, false),
            ActionKind::Hit
        );
    }

    #[test]
    fn nines_split_except_against_seven_ten_and_ace() {
        let cards = hand(&[Rank::Nine, Rank::Nine]);
        assert_eq!(first_two(&cards, Rank::Six), ActionKind::Split);
        assert_eq!(first_two(&cards, Rank::Seven), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Eight), ActionKind::Split);
        assert_eq!(first_two(&cards, Rank::Nine), ActionKind::Split);
        assert_eq!(first_two(&cards, Rank::Ten), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Ace), ActionKind::Stand);
    }

    #[test]
    fn low_pairs_follow_the_das_chart() {
        let sevens = hand(&[Rank::Seven, Rank::Seven]);
        assert_eq!(first_two(&sevens, Rank::Seven), ActionKind::Split);
        assert_eq!(first_two(&sevens, Rank::Eight), ActionKind::Hit);
        let sixes = hand(&[Rank::Six, Rank::Six]);
        assert_eq!(first_two(&sixes, Rank::Two), ActionKind::Split);
        assert_eq!(first_two(&sixes, Rank::Seven), ActionKind::Hit);
        let fives = hand(&[Rank::Five, Rank::Five]);
        assert_eq!(first_two(&fives, Rank::Nine), ActionKind::Double);
        assert_eq!(first_two(&fives, Rank::Ten), ActionKind::Hit);
        let fours = hand(&[Rank::Four, Rank::Four]);
        assert_eq!(first_two(&fours, Rank::Five), ActionKind::Split);
        assert_eq!(first_two(&fours, Rank::Four), ActionKind::Hit);
        let threes = hand(&[Rank::Three, Rank::Three]);
        assert_eq!(first_two(&threes, Rank::Seven), ActionKind::Split);
        assert_eq!(first_two(&threes, Rank::Eight), ActionKind::Hit);
        let twos = hand(&[Rank::Two, Rank::Two]);
        assert_eq!(first_two(&twos, Rank::Two), ActionKind::Split);
        assert_eq!(first_two(&twos, Rank::Eight), ActionKind::Hit);
    }

    #[test]
    fn twelve_hits_the_deuce_and_trey_and_stands_on_four_through_six() {
        let cards = hand(&[Rank::Ten, Rank::Two]);
        assert_eq!(first_two(&cards, Rank::Two), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Three), ActionKind::Hit);
        assert_eq!(first_two(&cards, Rank::Four), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Five), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Six), ActionKind::Stand);
        assert_eq!(first_two(&cards, Rank::Seven), ActionKind::Hit);
    }

    #[test]
    fn stiff_hands_stand_against_weak_upcards_and_hit_strong_ones() {
        let thirteen = hand(&[Rank::Ten, Rank::Three]);
        assert_eq!(first_two(&thirteen, Rank::Two), ActionKind::Stand);
        assert_eq!(first_two(&thirteen, Rank::Seven), ActionKind::Hit);
        let sixteen = hand(&[Rank::Ten, Rank::Six]);
        assert_eq!(first_two(&sixteen, Rank::Six), ActionKind::Stand);
        assert_eq!(first_two(&sixteen, Rank::Seven), ActionKind::Hit);
    }

    #[test]
    fn seventeen_and_up_always_stand() {
        let cards = hand(&[Rank::Ten, Rank::Seven]);
        for upcard in Rank::ALL {
            assert_eq!(first_two(&cards, upcard), ActionKind::Stand);
        }
    }

    #[test]
    fn the_returned_action_is_always_playable() {
        // Whatever the cell, a hand that can only hit or stand gets one of
        // those two.
        for a in Rank::ALL {
            for b in Rank::ALL {
                for upcard in Rank::ALL {
                    let cards = hand(&[a, b]);
                    let action = basic_strategy(&cards, up(upcard), false, false, false);
                    assert!(matches!(action, ActionKind::Hit | ActionKind::Stand));
                }
            }
        }
    }
}
