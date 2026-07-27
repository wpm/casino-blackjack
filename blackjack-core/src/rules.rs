//! Table rules: every knob a casino posts on the felt, frozen per shoe.

use serde::{Deserialize, Serialize};

/// How the dealer plays a soft 17.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Soft17 {
    /// Dealer stands on all 17s ("S17").
    Stand,
    /// Dealer hits soft 17 ("H17").
    Hit,
}

/// The posted payout for a natural blackjack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlackjackPayout {
    /// 3:2, the traditional full payout.
    ThreeToTwo,
    /// 6:5, the short payout.
    SixToFive,
}

impl BlackjackPayout {
    /// Winnings for a natural on `bet`, excluding the returned stake,
    /// rounded down to a whole dollar.
    ///
    /// Bets that keep the payout integral (even-dollar bets at 3:2, $5
    /// multiples at 6:5) never round; any fractional payout rounds down,
    /// as a real cage would drop the odd fifty cents.
    pub fn winnings(self, bet: u32) -> u32 {
        match self {
            BlackjackPayout::ThreeToTwo => bet * 3 / 2,
            BlackjackPayout::SixToFive => bet * 6 / 5,
        }
    }
}

/// The complete rule set for a table, frozen for the life of a shoe.
///
/// Everything the engine needs to adjudicate a round lives here; frontends
/// read these values out of every [`Snapshot`](crate::Snapshot) and never
/// hardcode a rule. Seats are numbered `0..seats`, with seat 0 dealt first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    /// Number of standard decks in the shoe.
    pub decks: usize,
    /// Fraction of the shoe dealt before the cut card signals a reshuffle.
    pub penetration: f64,
    /// Whether the dealer stands on or hits soft 17.
    pub soft_17: Soft17,
    /// Payout for a natural blackjack.
    pub blackjack_payout: BlackjackPayout,
    /// Double down on any first two cards. When false, doubling is allowed
    /// only on hard totals of 9, 10, and 11.
    pub double_on_any_two: bool,
    /// Allow doubling down on a hand created by a split.
    pub double_after_split: bool,
    /// Maximum hands a single seat may hold through splitting; 4 means
    /// "resplit to four hands". Must be at least 1 (1 disables splitting).
    pub max_split_hands: usize,
    /// Split aces receive exactly one card each and stand automatically.
    pub split_aces_one_card: bool,
    /// Late surrender: give up half the bet as the first action on the
    /// initial two cards, only after the dealer has checked for blackjack.
    pub late_surrender: bool,
    /// Offer insurance when the dealer shows an ace.
    pub insurance_offered: bool,
    /// Minimum main bet in whole dollars.
    pub min_bet: u32,
    /// Maximum main bet in whole dollars.
    pub max_bet: u32,
    /// Number of player positions at the table. Seats without a bet sit out.
    pub seats: usize,
}

impl Rules {
    /// The canonical table this app ships with: six decks, 75% penetration,
    /// dealer stands on soft 17, blackjack pays 3:2, double on any two,
    /// double after split, resplit to four hands, split aces get one card,
    /// late surrender, insurance offered, $10–$500 limits, seven seats.
    pub fn canonical() -> Rules {
        Rules {
            decks: 6,
            penetration: 0.75,
            soft_17: Soft17::Stand,
            blackjack_payout: BlackjackPayout::ThreeToTwo,
            double_on_any_two: true,
            double_after_split: true,
            max_split_hands: 4,
            split_aces_one_card: true,
            late_surrender: true,
            insurance_offered: true,
            min_bet: 10,
            max_bet: 500,
            seats: 7,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_rules_match_the_posted_table() {
        let rules = Rules::canonical();
        assert_eq!(rules.decks, 6);
        assert_eq!(rules.penetration, 0.75);
        assert_eq!(rules.soft_17, Soft17::Stand);
        assert_eq!(rules.blackjack_payout, BlackjackPayout::ThreeToTwo);
        assert!(rules.double_on_any_two);
        assert!(rules.double_after_split);
        assert_eq!(rules.max_split_hands, 4);
        assert!(rules.split_aces_one_card);
        assert!(rules.late_surrender);
        assert!(rules.insurance_offered);
        assert_eq!(rules.min_bet, 10);
        assert_eq!(rules.max_bet, 500);
        assert_eq!(rules.seats, 7);
    }

    #[test]
    fn three_to_two_pays_exactly_on_even_bets_and_rounds_down_on_odd() {
        let p = BlackjackPayout::ThreeToTwo;
        assert_eq!(p.winnings(10), 15);
        assert_eq!(p.winnings(20), 30);
        assert_eq!(p.winnings(500), 750);
        // 15 * 1.5 = 22.5 rounds down to 22.
        assert_eq!(p.winnings(15), 22);
        assert_eq!(p.winnings(1), 1);
    }

    #[test]
    fn six_to_five_pays_exactly_on_five_dollar_multiples_and_rounds_down_otherwise() {
        let p = BlackjackPayout::SixToFive;
        assert_eq!(p.winnings(10), 12);
        assert_eq!(p.winnings(25), 30);
        // 12 * 1.2 = 14.4 rounds down to 14.
        assert_eq!(p.winnings(12), 14);
    }

    #[test]
    fn rules_serde_round_trips() {
        let rules = Rules::canonical();
        let json = serde_json::to_string(&rules).unwrap();
        let back: Rules = serde_json::from_str(&json).unwrap();
        assert_eq!(rules, back);
    }
}
