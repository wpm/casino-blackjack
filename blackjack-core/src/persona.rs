//! AI player personas: hidden archetypes with distinct, imperfect styles.
//!
//! Everything here is crate-private on purpose. Archetypes are flavor the
//! player senses across rounds — the timid tourist, the reckless
//! plunger — never a label the UI shows or a knob anyone configures.
//! Every decision is a pure function of (archetype, hand, upcard, legal
//! actions, RNG), so play is fully deterministic under a seeded generator.

use rand::RngCore;

use crate::action::{Action, ActionKind};
use crate::card::Card;
use crate::hand::is_value_pair;
use crate::snapshot::HandSnapshot;
use crate::strategy::basic_strategy;

/// A hidden play style. Which archetype sits in which seat is never
/// exposed outside the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Archetype {
    /// Plays perfect basic strategy for the canonical rules.
    ByTheBook,
    /// Never doubles or splits, stands too early against strong upcards,
    /// and buys insurance out of fear.
    Timid,
    /// Hits too long, splits tens, and doubles wildly.
    Reckless,
    /// Plays folk rules: never hit against a "bust card", always double
    /// eleven, only split aces and eights, never surrender.
    Superstitious,
}

impl Archetype {
    /// Every archetype, for arrival rolls.
    pub(crate) const ALL: [Archetype; 4] = [
        Archetype::ByTheBook,
        Archetype::Timid,
        Archetype::Reckless,
        Archetype::Superstitious,
    ];
}

fn has(legal: &[ActionKind], kind: ActionKind) -> bool {
    legal.contains(&kind)
}

/// Decide the play for `hand` against `upcard`.
///
/// `legal` must be the table's legal actions for the hand, already
/// filtered for what the player can afford (a double or split costs
/// another bet). The returned action is always drawn from `legal`; hit
/// and stand are always available during a player turn.
pub(crate) fn decide<R: RngCore>(
    archetype: Archetype,
    hand: &HandSnapshot,
    upcard: Card,
    legal: &[ActionKind],
    rng: &mut R,
) -> Action {
    let up = upcard.value();
    match archetype {
        Archetype::ByTheBook => {
            let kind = basic_strategy(
                &hand.cards,
                upcard,
                has(legal, ActionKind::Double),
                has(legal, ActionKind::Split),
                has(legal, ActionKind::Surrender),
            );
            match kind {
                ActionKind::Stand => Action::Stand,
                ActionKind::Double => Action::Double,
                ActionKind::Split => Action::Split,
                ActionKind::Surrender => Action::Surrender,
                _ => Action::Hit,
            }
        }
        Archetype::Timid => {
            // No doubles, no splits, no surrender; just careful standing.
            // Against a strong upcard the book says draw to 17, but nerves
            // give out at 14; against a weak one, any 12 freezes.
            let stand = if hand.soft {
                hand.total >= 18
            } else if up >= 7 {
                hand.total >= 14
            } else {
                hand.total >= 12
            };
            if stand { Action::Stand } else { Action::Hit }
        }
        Archetype::Reckless => {
            // Any pair splits — tens included.
            if has(legal, ActionKind::Split) {
                return Action::Split;
            }
            if has(legal, ActionKind::Double) {
                let by_the_gut = if hand.soft {
                    hand.total <= 18
                } else {
                    (8..=11).contains(&hand.total)
                };
                // And every so often a stiff hand doubles on a hunch.
                let wild = !hand.soft
                    && (12..=16).contains(&hand.total)
                    && rng.next_u32().is_multiple_of(4);
                if by_the_gut || wild {
                    return Action::Double;
                }
            }
            // Draw far too long: hard 17 is "one more card" territory.
            let stand = if hand.soft {
                hand.total >= 19
            } else {
                hand.total >= 18
            };
            if stand { Action::Stand } else { Action::Hit }
        }
        Archetype::Superstitious => {
            // "Only ever split aces and eights."
            if has(legal, ActionKind::Split)
                && is_value_pair(&hand.cards)
                && matches!(hand.cards[0].value(), 11 | 8)
            {
                return Action::Split;
            }
            // "Always double eleven — it's lucky."
            if has(legal, ActionKind::Double) && !hand.soft && hand.total == 11 {
                return Action::Double;
            }
            // "Never hit against a bust card": any 12 stands vs 2-6.
            // Surrender is bad luck, so it never happens.
            let stand = if hand.soft {
                hand.total >= 18
            } else if (2..=6).contains(&up) {
                hand.total >= 12
            } else {
                hand.total >= 17
            };
            if stand { Action::Stand } else { Action::Hit }
        }
    }
}

/// Whether the archetype buys insurance when offered (and affordable).
pub(crate) fn decide_insurance<R: RngCore>(archetype: Archetype, rng: &mut R) -> bool {
    match archetype {
        // The book says never; the plunger thinks it's a sucker bet too.
        Archetype::ByTheBook | Archetype::Reckless => false,
        // Anything to feel safe.
        Archetype::Timid => true,
        // Depends how the ace feels tonight.
        Archetype::Superstitious => rng.next_u32().is_multiple_of(3),
    }
}

/// The archetype's bet for the coming round, clamped to the table limits
/// and the player's bankroll.
///
/// `base` is the player's habitual bet (chosen at arrival), `streak` the
/// current run: positive for consecutive winning rounds, negative for
/// losing ones, zero after a push. Callers must only ask players who can
/// still cover the table minimum (`bankroll >= min`).
pub(crate) fn bet_size(
    archetype: Archetype,
    base: u32,
    streak: i32,
    bankroll: u32,
    min: u32,
    max: u32,
) -> u32 {
    let desired = match archetype {
        // Flat, always.
        Archetype::ByTheBook => base,
        // Scared money: halve the bet for every consecutive loss.
        Archetype::Timid => {
            if streak < 0 {
                base >> (-streak).min(4) as u32
            } else {
                base
            }
        }
        // Press it: double the bet for every consecutive win.
        Archetype::Reckless => {
            if streak > 0 {
                base.saturating_mul(1 << streak.min(5) as u32)
            } else {
                base
            }
        }
        // Ride hot streaks up, retreat to the minimum when cold.
        Archetype::Superstitious => match streak {
            1.. => base.saturating_mul(streak as u32 + 1),
            0 => base,
            _ => min,
        },
    };
    desired.clamp(min, max.min(bankroll).max(min))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::{Rank, Suit};
    use crate::snapshot::HandStatus;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    fn rng() -> ChaCha8Rng {
        ChaCha8Rng::seed_from_u64(7)
    }

    fn up(rank: Rank) -> Card {
        Card::new(rank, Suit::Spades)
    }

    fn snapshot(ranks: &[Rank]) -> HandSnapshot {
        let cards: Vec<Card> = ranks
            .iter()
            .zip(Suit::ALL.iter().cycle())
            .map(|(&rank, &suit)| Card::new(rank, suit))
            .collect();
        let value = crate::hand::HandValue::of(&cards);
        HandSnapshot {
            cards,
            bet: 10,
            doubled: false,
            from_split: false,
            status: HandStatus::Playing,
            total: value.total(),
            soft: value.is_soft(),
            outcome: None,
            payout: None,
        }
    }

    const FULL: [ActionKind; 5] = [
        ActionKind::Hit,
        ActionKind::Stand,
        ActionKind::Double,
        ActionKind::Split,
        ActionKind::Surrender,
    ];
    const HIT_STAND: [ActionKind; 2] = [ActionKind::Hit, ActionKind::Stand];

    #[test]
    fn by_the_book_plays_the_canonical_cells() {
        let mut r = rng();
        let a = Archetype::ByTheBook;
        let sixteen = snapshot(&[Rank::Ten, Rank::Six]);
        assert_eq!(
            decide(a, &sixteen, up(Rank::Ten), &FULL, &mut r),
            Action::Surrender
        );
        assert_eq!(
            decide(a, &sixteen, up(Rank::Ten), &HIT_STAND, &mut r),
            Action::Hit
        );
        let eleven = snapshot(&[Rank::Six, Rank::Five]);
        assert_eq!(
            decide(a, &eleven, up(Rank::Ace), &FULL, &mut r),
            Action::Double
        );
        let eights = snapshot(&[Rank::Eight, Rank::Eight]);
        assert_eq!(
            decide(a, &eights, up(Rank::Ace), &FULL, &mut r),
            Action::Split
        );
        let soft19 = snapshot(&[Rank::Ace, Rank::Eight]);
        assert_eq!(
            decide(a, &soft19, up(Rank::Six), &FULL, &mut r),
            Action::Stand
        );
    }

    #[test]
    fn timid_never_doubles_or_splits_and_stands_too_early() {
        let mut r = rng();
        let a = Archetype::Timid;
        // A pair of eights against a six: the book splits, nerves stand.
        let eights = snapshot(&[Rank::Eight, Rank::Eight]);
        assert_eq!(
            decide(a, &eights, up(Rank::Six), &FULL, &mut r),
            Action::Stand
        );
        // Eleven against a six: the book doubles, nerves just hit.
        let eleven = snapshot(&[Rank::Six, Rank::Five]);
        assert_eq!(
            decide(a, &eleven, up(Rank::Six), &FULL, &mut r),
            Action::Hit
        );
        // Fourteen against a ten: the book draws to 17, nerves stand.
        let fourteen = snapshot(&[Rank::Ten, Rank::Four]);
        assert_eq!(
            decide(a, &fourteen, up(Rank::Ten), &HIT_STAND, &mut r),
            Action::Stand
        );
        // Thirteen against a ten still hits...
        let thirteen = snapshot(&[Rank::Ten, Rank::Three]);
        assert_eq!(
            decide(a, &thirteen, up(Rank::Ten), &HIT_STAND, &mut r),
            Action::Hit
        );
        // ...and any twelve freezes against a weak card, even the deuce.
        let twelve = snapshot(&[Rank::Ten, Rank::Two]);
        assert_eq!(
            decide(a, &twelve, up(Rank::Two), &HIT_STAND, &mut r),
            Action::Stand
        );
    }

    #[test]
    fn reckless_splits_tens_hits_seventeen_and_doubles_soft_hands() {
        let mut r = rng();
        let a = Archetype::Reckless;
        let tens = snapshot(&[Rank::King, Rank::Queen]);
        assert_eq!(
            decide(a, &tens, up(Rank::Ten), &FULL, &mut r),
            Action::Split
        );
        let seventeen = snapshot(&[Rank::Ten, Rank::Seven]);
        assert_eq!(
            decide(a, &seventeen, up(Rank::Ace), &HIT_STAND, &mut r),
            Action::Hit
        );
        let soft18 = snapshot(&[Rank::Ace, Rank::Seven]);
        assert_eq!(
            decide(
                a,
                &soft18,
                up(Rank::Ten),
                &[ActionKind::Hit, ActionKind::Stand, ActionKind::Double],
                &mut r
            ),
            Action::Double
        );
        let eighteen = snapshot(&[Rank::Ten, Rank::Eight]);
        assert_eq!(
            decide(a, &eighteen, up(Rank::Ace), &HIT_STAND, &mut r),
            Action::Stand
        );
    }

    #[test]
    fn superstitious_follows_the_folk_rules() {
        let mut r = rng();
        let a = Archetype::Superstitious;
        // "Never hit against a bust card": 12 vs 2 stands (book hits).
        let twelve = snapshot(&[Rank::Ten, Rank::Two]);
        assert_eq!(
            decide(a, &twelve, up(Rank::Two), &HIT_STAND, &mut r),
            Action::Stand
        );
        // Against a strong card, draw to 17 — never surrender.
        let sixteen = snapshot(&[Rank::Ten, Rank::Six]);
        assert_eq!(
            decide(a, &sixteen, up(Rank::Ten), &FULL, &mut r),
            Action::Hit
        );
        // Aces and eights split; nines never do (the book splits vs 5).
        let eights = snapshot(&[Rank::Eight, Rank::Eight]);
        assert_eq!(
            decide(a, &eights, up(Rank::Ten), &FULL, &mut r),
            Action::Split
        );
        let nines = snapshot(&[Rank::Nine, Rank::Nine]);
        assert_eq!(
            decide(a, &nines, up(Rank::Five), &FULL, &mut r),
            Action::Stand
        );
        // Eleven always doubles; soft 18 always stands, even vs an ace.
        let eleven = snapshot(&[Rank::Six, Rank::Five]);
        assert_eq!(
            decide(a, &eleven, up(Rank::Ace), &FULL, &mut r),
            Action::Double
        );
        let soft18 = snapshot(&[Rank::Ace, Rank::Seven]);
        assert_eq!(
            decide(a, &soft18, up(Rank::Ace), &HIT_STAND, &mut r),
            Action::Stand
        );
    }

    #[test]
    fn insurance_split_follows_temperament() {
        let mut r = rng();
        assert!(!decide_insurance(Archetype::ByTheBook, &mut r));
        assert!(!decide_insurance(Archetype::Reckless, &mut r));
        assert!(decide_insurance(Archetype::Timid, &mut r));
        // Superstitious insurance is a mood: over many offers both answers
        // occur, deterministically under the seed.
        let answers: Vec<bool> = (0..100)
            .map(|_| decide_insurance(Archetype::Superstitious, &mut r))
            .collect();
        assert!(answers.iter().any(|&b| b));
        assert!(answers.iter().any(|&b| !b));
    }

    #[test]
    fn bets_stay_within_limits_and_bankroll() {
        for archetype in Archetype::ALL {
            for streak in -6..=6 {
                for bankroll in [10, 35, 120, 100_000] {
                    for base in [10, 30, 90] {
                        let bet = bet_size(archetype, base, streak, bankroll, 10, 500);
                        assert!(bet >= 10, "{archetype:?} bet {bet} below table minimum");
                        assert!(bet <= 500, "{archetype:?} bet {bet} above table maximum");
                        assert!(
                            bet <= bankroll.max(10),
                            "{archetype:?} bet {bet} above bankroll {bankroll}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn bet_styles_are_distinct() {
        // Flat.
        assert_eq!(bet_size(Archetype::ByTheBook, 30, 4, 1000, 10, 500), 30);
        assert_eq!(bet_size(Archetype::ByTheBook, 30, -4, 1000, 10, 500), 30);
        // Scared money shrinks after losses but never presses.
        assert_eq!(bet_size(Archetype::Timid, 40, -1, 1000, 10, 500), 20);
        assert_eq!(bet_size(Archetype::Timid, 40, -2, 1000, 10, 500), 10);
        assert_eq!(bet_size(Archetype::Timid, 40, 3, 1000, 10, 500), 40);
        // The press doubles per win and caps at the table max.
        assert_eq!(bet_size(Archetype::Reckless, 20, 2, 1000, 10, 500), 80);
        assert_eq!(bet_size(Archetype::Reckless, 20, 9, 1000, 10, 500), 500);
        assert_eq!(bet_size(Archetype::Reckless, 20, -3, 1000, 10, 500), 20);
        // Streak swings both ways.
        assert_eq!(bet_size(Archetype::Superstitious, 20, 2, 1000, 10, 500), 60);
        assert_eq!(
            bet_size(Archetype::Superstitious, 20, -1, 1000, 10, 500),
            10
        );
    }
}
