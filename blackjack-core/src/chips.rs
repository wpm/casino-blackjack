//! Chips: the physical money of the table — denominations, stacks, change
//! making, payouts as chips, and racks.
//!
//! The engine settles rounds in whole-dollar net deltas
//! ([`Event::HandSettled`](crate::Event::HandSettled) and friends); this
//! module turns those numbers into the chips that physically move across
//! the felt. Everything is integer `u32` dollars end to end, so fractional
//! amounts — the $22.50 a true 3:2 payout of a $15 bet would owe — are
//! impossible by construction: the engine rounds fractional payouts down
//! to whole dollars before they ever reach the chip layer, and no API here
//! can even ask for fifty cents.

use std::fmt;
use std::iter;
use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};

/// A casino chip denomination, in the standard American colors.
///
/// Denominations order by face value, smallest first, and each value
/// divides the next ($1, $5, $25, $100, $500), which is why greedy
/// largest-first change making is exact and minimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Denomination {
    /// $1, white.
    One,
    /// $5, red.
    Five,
    /// $25, green.
    TwentyFive,
    /// $100, black.
    Hundred,
    /// $500, purple.
    FiveHundred,
}

impl Denomination {
    /// All five denominations, in ascending [`Ord`] (face value) order.
    pub const ALL: [Denomination; 5] = [
        Denomination::One,
        Denomination::Five,
        Denomination::TwentyFive,
        Denomination::Hundred,
        Denomination::FiveHundred,
    ];

    /// The chip's face value in whole dollars.
    pub fn value(self) -> u32 {
        match self {
            Denomination::One => 1,
            Denomination::Five => 5,
            Denomination::TwentyFive => 25,
            Denomination::Hundred => 100,
            Denomination::FiveHundred => 500,
        }
    }

    /// The chip's standard casino color, for frontends to key rendering on.
    pub fn color(self) -> ChipColor {
        match self {
            Denomination::One => ChipColor::White,
            Denomination::Five => ChipColor::Red,
            Denomination::TwentyFive => ChipColor::Green,
            Denomination::Hundred => ChipColor::Black,
            Denomination::FiveHundred => ChipColor::Purple,
        }
    }

    /// Position in [`Denomination::ALL`].
    fn index(self) -> usize {
        self as usize
    }
}

/// The standard color of a chip denomination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChipColor {
    /// $1 chips.
    White,
    /// $5 chips.
    Red,
    /// $25 chips.
    Green,
    /// $100 chips.
    Black,
    /// $500 chips.
    Purple,
}

impl ChipColor {
    /// The color as a lowercase CSS-friendly name.
    pub fn name(self) -> &'static str {
        match self {
            ChipColor::White => "white",
            ChipColor::Red => "red",
            ChipColor::Green => "green",
            ChipColor::Black => "black",
            ChipColor::Purple => "purple",
        }
    }
}

impl fmt::Display for ChipColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Why a chip operation failed.
///
/// A failed operation never mutates the stack: the caller gets one of
/// these and the chips are exactly as they were.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChipError {
    /// A withdrawal asked for more value than the stack holds.
    InsufficientValue {
        /// The dollars requested.
        requested: u32,
        /// The dollars available.
        available: u32,
    },
    /// A removal asked for more chips of a denomination than the stack
    /// holds.
    InsufficientChips {
        /// The denomination removed from.
        denomination: Denomination,
        /// The chips requested.
        requested: u32,
        /// The chips available.
        available: u32,
    },
}

impl fmt::Display for ChipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChipError::InsufficientValue {
                requested,
                available,
            } => {
                write!(f, "cannot withdraw ${requested} from a ${available} stack")
            }
            ChipError::InsufficientChips {
                denomination,
                requested,
                available,
            } => {
                write!(
                    f,
                    "cannot remove {requested} {denomination:?} chips from a stack holding {available}"
                )
            }
        }
    }
}

impl std::error::Error for ChipError {}

/// A stack of chips: a multiset of [`Denomination`]s.
///
/// A `ChipStack` is a plain value — a bet on the felt, a payout being
/// pushed across it, the contents of a [`Rack`]. Two stacks are equal when
/// they hold the same chips, not merely the same total.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChipStack {
    /// Chip counts, indexed in [`Denomination::ALL`] order.
    counts: [u32; 5],
}

impl ChipStack {
    /// An empty stack.
    pub fn new() -> ChipStack {
        ChipStack::default()
    }

    /// Make change for `amount` dollars: the greedy largest-first
    /// breakdown, which is exact and uses the fewest possible chips
    /// because every denomination divides the next.
    ///
    /// `amount` is whole `u32` dollars, so a fractional request — the
    /// $2.50 half-dollar a true 3:2 payout of an odd bet would owe — is
    /// impossible by construction. There is nothing to round here: the
    /// engine has already rounded every fractional payout down to whole
    /// dollars ([`BlackjackPayout::winnings`](crate::BlackjackPayout::winnings),
    /// surrender, insurance).
    pub fn change(amount: u32) -> ChipStack {
        let mut stack = ChipStack::new();
        let mut remaining = amount;
        for denomination in Denomination::ALL.into_iter().rev() {
            stack.counts[denomination.index()] = remaining / denomination.value();
            remaining %= denomination.value();
        }
        stack
    }

    /// The stack's total value in whole dollars.
    pub fn total(&self) -> u32 {
        Denomination::ALL
            .into_iter()
            .map(|d| self.counts[d.index()] * d.value())
            .sum()
    }

    /// How many chips of `denomination` the stack holds.
    pub fn count(&self, denomination: Denomination) -> u32 {
        self.counts[denomination.index()]
    }

    /// The number of physical chips in the stack.
    pub fn chip_count(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// Whether the stack holds no chips at all.
    pub fn is_empty(&self) -> bool {
        self.counts.iter().all(|&n| n == 0)
    }

    /// Add `count` chips of `denomination`.
    pub fn add_chips(&mut self, denomination: Denomination, count: u32) {
        self.counts[denomination.index()] += count;
    }

    /// Remove `count` chips of `denomination`, failing (without mutation)
    /// if the stack holds fewer.
    pub fn remove_chips(
        &mut self,
        denomination: Denomination,
        count: u32,
    ) -> Result<(), ChipError> {
        let available = self.counts[denomination.index()];
        if count > available {
            return Err(ChipError::InsufficientChips {
                denomination,
                requested: count,
                available,
            });
        }
        self.counts[denomination.index()] -= count;
        Ok(())
    }

    /// Pour `other` into this stack.
    pub fn merge(&mut self, other: ChipStack) {
        for denomination in Denomination::ALL {
            self.counts[denomination.index()] += other.counts[denomination.index()];
        }
    }

    /// Withdraw exactly `amount` dollars in chips, failing (without
    /// mutation) if the stack is worth less.
    ///
    /// Chips come out greedily, largest first. When the stack cannot pay
    /// exactly with the chips on hand (say a lone black chip owing $10),
    /// a chip is broken — exchanged at par for the next denomination down,
    /// as a player makes change with the cage — until the amount can be
    /// cut. The withdrawal and the remaining stack always sum to the
    /// original total.
    pub fn withdraw(&mut self, amount: u32) -> Result<ChipStack, ChipError> {
        if self.total() < amount {
            return Err(ChipError::InsufficientValue {
                requested: amount,
                available: self.total(),
            });
        }
        let mut taken = ChipStack::new();
        let mut remaining = amount;
        loop {
            // Greedy pass, largest first. Afterwards every denomination
            // still in the stack is worth more than what remains owed.
            for denomination in Denomination::ALL.into_iter().rev() {
                let index = denomination.index();
                let take = (remaining / denomination.value()).min(self.counts[index]);
                self.counts[index] -= take;
                taken.counts[index] += take;
                remaining -= take * denomination.value();
            }
            if remaining == 0 {
                return Ok(taken);
            }
            // Break the smallest chip larger than the remainder into the
            // next denomination down (a par exchange with the cage). One
            // exists: the stack still holds value, and everything smaller
            // than the remainder was exhausted by the greedy pass.
            let broken = Denomination::ALL
                .into_iter()
                .find(|d| d.value() > remaining && self.counts[d.index()] > 0)
                .expect("a solvent stack always has a chip left to break");
            let lower = Denomination::ALL[broken.index() - 1];
            self.counts[broken.index()] -= 1;
            self.counts[lower.index()] += broken.value() / lower.value();
        }
    }

    /// Color up: the same total in the fewest possible chips.
    ///
    /// This is exactly [`ChipStack::change`] of the total — greedy change
    /// is minimal for these denominations. Used when a player leaves the
    /// table and to keep AI stacks tidy.
    pub fn color_up(&self) -> ChipStack {
        ChipStack::change(self.total())
    }

    /// The denominations present, smallest first, with their counts.
    /// Denominations with no chips are skipped.
    pub fn iter(&self) -> impl Iterator<Item = (Denomination, u32)> + '_ {
        Denomination::ALL.into_iter().filter_map(|denomination| {
            let count = self.counts[denomination.index()];
            (count > 0).then_some((denomination, count))
        })
    }

    /// Every physical chip in the stack, one at a time, smallest first —
    /// for rendering each chip individually.
    pub fn chips(&self) -> impl Iterator<Item = Denomination> + '_ {
        self.iter()
            .flat_map(|(denomination, count)| iter::repeat_n(denomination, count as usize))
    }
}

impl Add for ChipStack {
    type Output = ChipStack;

    fn add(mut self, rhs: ChipStack) -> ChipStack {
        self.merge(rhs);
        self
    }
}

impl AddAssign for ChipStack {
    fn add_assign(&mut self, rhs: ChipStack) {
        self.merge(rhs);
    }
}

impl FromIterator<Denomination> for ChipStack {
    fn from_iter<I: IntoIterator<Item = Denomination>>(iter: I) -> ChipStack {
        let mut stack = ChipStack::new();
        for denomination in iter {
            stack.add_chips(denomination, 1);
        }
        stack
    }
}

/// The chips the dealer physically pushes toward the player for winning
/// `winnings` dollars on the bet sitting on the felt as `bet_stack`.
///
/// An even-money win is paid by mirroring the bet chip for chip, exactly
/// as a real dealer sizes the payout against the bet; any other amount
/// (a 3:2 natural, a rounded surrender return) is cut as minimal change.
/// The returned stack always totals exactly `winnings`.
pub fn payout_chips(bet_stack: &ChipStack, winnings: u32) -> ChipStack {
    if winnings == bet_stack.total() {
        bet_stack.clone()
    } else {
        ChipStack::change(winnings)
    }
}

/// A rack of chips: a player's bankroll, or the house tray.
///
/// The rack is the bridge between chips and rounds: chips leave it when a
/// bet (or insurance stake, double, or split) goes onto the felt, and come
/// back when a settlement pays out. It stays deliberately small — the
/// session arc builds on it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rack {
    chips: ChipStack,
}

impl Rack {
    /// An empty rack.
    pub fn new() -> Rack {
        Rack::default()
    }

    /// A rack holding `amount` dollars as minimal change.
    pub fn with_bankroll(amount: u32) -> Rack {
        Rack {
            chips: ChipStack::change(amount),
        }
    }

    /// A rack holding exactly `chips`.
    pub fn from_chips(chips: ChipStack) -> Rack {
        Rack { chips }
    }

    /// The chips in the rack.
    pub fn chips(&self) -> &ChipStack {
        &self.chips
    }

    /// The rack's total value in whole dollars.
    pub fn total(&self) -> u32 {
        self.chips.total()
    }

    /// Take `amount` dollars of chips out of the rack to place on the
    /// felt, breaking chips as needed. Fails (without mutation) if the
    /// rack is worth less than `amount`.
    pub fn place_bet(&mut self, amount: u32) -> Result<ChipStack, ChipError> {
        self.chips.withdraw(amount)
    }

    /// Pour chips into the rack — a settlement payout, a returned bet, or
    /// a buy-in.
    pub fn receive(&mut self, chips: ChipStack) {
        self.chips.merge(chips);
    }

    /// Settle a bet against this rack acting as the house tray: sweep the
    /// felt chips (`bet`) into the tray, then cut and return the chips
    /// owed back to the player — the stake plus `net`.
    ///
    /// `net` is a settlement delta from the engine
    /// ([`Event::HandSettled`](crate::Event::HandSettled) amounts, or a
    /// seat's `round_net` against its whole felt stake). Fails (without
    /// losing the swept bet) if the tray cannot cover the payout.
    ///
    /// # Panics
    ///
    /// Panics if `net` is below `-bet.total()` — the engine never settles
    /// a hand for more than its stake.
    pub fn settle(&mut self, bet: ChipStack, net: i32) -> Result<ChipStack, ChipError> {
        let stake = i64::from(bet.total());
        let returned = stake + i64::from(net);
        assert!(
            returned >= 0,
            "settlement net {net} exceeds the ${stake} stake"
        );
        self.chips.merge(bet);
        self.chips.withdraw(returned as u32)
    }

    /// Color up the rack in place: same total, fewest chips.
    pub fn color_up(&mut self) {
        self.chips = self.chips.color_up();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::BlackjackPayout;

    fn stack(chips: &[(Denomination, u32)]) -> ChipStack {
        let mut stack = ChipStack::new();
        for &(denomination, count) in chips {
            stack.add_chips(denomination, count);
        }
        stack
    }

    #[test]
    fn denominations_have_standard_values_and_colors() {
        let expected = [
            (Denomination::One, 1, ChipColor::White, "white"),
            (Denomination::Five, 5, ChipColor::Red, "red"),
            (Denomination::TwentyFive, 25, ChipColor::Green, "green"),
            (Denomination::Hundred, 100, ChipColor::Black, "black"),
            (Denomination::FiveHundred, 500, ChipColor::Purple, "purple"),
        ];
        for (denomination, value, color, name) in expected {
            assert_eq!(denomination.value(), value);
            assert_eq!(denomination.color(), color);
            assert_eq!(denomination.color().name(), name);
            assert_eq!(denomination.color().to_string(), name);
        }
    }

    #[test]
    fn denominations_order_by_value() {
        for pair in Denomination::ALL.windows(2) {
            assert!(pair[0] < pair[1]);
            assert!(pair[0].value() < pair[1].value());
        }
    }

    #[test]
    fn change_for_zero_is_empty() {
        let stack = ChipStack::change(0);
        assert!(stack.is_empty());
        assert_eq!(stack.total(), 0);
        assert_eq!(stack.chip_count(), 0);
    }

    #[test]
    fn change_breaks_down_greedily() {
        // $641 = $500 + $100 + $25 + 3 x $5 + $1.
        let stack = ChipStack::change(641);
        assert_eq!(stack.count(Denomination::FiveHundred), 1);
        assert_eq!(stack.count(Denomination::Hundred), 1);
        assert_eq!(stack.count(Denomination::TwentyFive), 1);
        assert_eq!(stack.count(Denomination::Five), 3);
        assert_eq!(stack.count(Denomination::One), 1);
        assert_eq!(stack.total(), 641);
        assert_eq!(stack.chip_count(), 7);
    }

    #[test]
    fn change_uses_ones_below_five() {
        let stack = ChipStack::change(4);
        assert_eq!(stack.count(Denomination::One), 4);
        assert_eq!(stack.chip_count(), 4);
    }

    #[test]
    fn every_canonical_blackjack_payout_is_physically_payable() {
        // At the canonical $10-$500 table, every legal $5-multiple bet's
        // 3:2 winnings are a whole-dollar amount (odd multiples like $15
        // round the fifty cents down before reaching the chip layer), and
        // the chips that move total exactly those winnings. The $2.50
        // problem cannot arise: amounts are u32 dollars by construction.
        for bet in (10..=500).step_by(5) {
            let winnings = BlackjackPayout::ThreeToTwo.winnings(bet);
            let paid = payout_chips(&ChipStack::change(bet), winnings);
            assert_eq!(paid.total(), winnings, "bet ${bet}");
            // Even bets pay a mathematically exact 3:2.
            if bet % 2 == 0 {
                assert_eq!(2 * winnings, 3 * bet, "bet ${bet}");
            }
        }
    }

    #[test]
    fn even_money_payouts_mirror_the_bet_stack() {
        // A $30 bet as green-and-red pays with the same cut, not change.
        let bet = stack(&[(Denomination::TwentyFive, 1), (Denomination::Five, 1)]);
        let paid = payout_chips(&bet, 30);
        assert_eq!(paid, bet);
        // A non-matching amount is cut as minimal change instead.
        let natural = payout_chips(&bet, 45);
        assert_eq!(natural.total(), 45);
        assert_eq!(natural.count(Denomination::TwentyFive), 1);
        assert_eq!(natural.count(Denomination::Five), 4);
    }

    #[test]
    fn color_up_exchanges_small_chips_for_large() {
        // 5 x $5 + 5 x $1 = $30 colors up to $25 + $5.
        let messy = stack(&[(Denomination::Five, 5), (Denomination::One, 5)]);
        let tidy = messy.color_up();
        assert_eq!(tidy.total(), 30);
        assert_eq!(tidy.chip_count(), 2);
        assert_eq!(tidy.count(Denomination::TwentyFive), 1);
        assert_eq!(tidy.count(Denomination::Five), 1);
    }

    #[test]
    fn withdraw_takes_largest_chips_first() {
        let mut rack = ChipStack::change(155);
        let taken = rack.withdraw(105).unwrap();
        assert_eq!(taken.total(), 105);
        assert_eq!(taken.count(Denomination::Hundred), 1);
        assert_eq!(taken.count(Denomination::Five), 1);
        assert_eq!(rack.total(), 50);
    }

    #[test]
    fn withdraw_breaks_chips_when_it_must() {
        // A lone black chip owing $10: break $100 into quarters, break a
        // quarter into nickels, pay two red chips.
        let mut rack = stack(&[(Denomination::Hundred, 1)]);
        let taken = rack.withdraw(10).unwrap();
        assert_eq!(taken.total(), 10);
        assert_eq!(taken.count(Denomination::Five), 2);
        assert_eq!(rack.total(), 90);
        assert_eq!(rack.count(Denomination::TwentyFive), 3);
        assert_eq!(rack.count(Denomination::Five), 3);
    }

    #[test]
    fn withdraw_beyond_the_stack_fails_without_mutation() {
        let mut rack = ChipStack::change(75);
        let before = rack.clone();
        assert_eq!(
            rack.withdraw(100),
            Err(ChipError::InsufficientValue {
                requested: 100,
                available: 75,
            })
        );
        assert_eq!(rack, before);
    }

    #[test]
    fn remove_beyond_the_count_fails_without_mutation() {
        let mut rack = stack(&[(Denomination::Five, 2)]);
        let before = rack.clone();
        assert_eq!(
            rack.remove_chips(Denomination::Five, 3),
            Err(ChipError::InsufficientChips {
                denomination: Denomination::Five,
                requested: 3,
                available: 2,
            })
        );
        assert_eq!(rack, before);
        rack.remove_chips(Denomination::Five, 2).unwrap();
        assert!(rack.is_empty());
    }

    #[test]
    fn stacks_merge_and_add() {
        let mut left = ChipStack::change(30);
        left.merge(ChipStack::change(70));
        assert_eq!(left.total(), 100);
        let sum = ChipStack::change(500) + ChipStack::change(125);
        assert_eq!(sum.total(), 625);
        let mut accumulated = ChipStack::new();
        accumulated += ChipStack::change(15);
        assert_eq!(accumulated.total(), 15);
    }

    #[test]
    fn iteration_walks_every_chip_smallest_first() {
        let stack = stack(&[(Denomination::TwentyFive, 1), (Denomination::Five, 2)]);
        assert_eq!(
            stack.iter().collect::<Vec<_>>(),
            [(Denomination::Five, 2), (Denomination::TwentyFive, 1)]
        );
        let chips: Vec<Denomination> = stack.chips().collect();
        assert_eq!(
            chips,
            [
                Denomination::Five,
                Denomination::Five,
                Denomination::TwentyFive
            ]
        );
        // Collecting the chips rebuilds the same stack.
        assert_eq!(chips.into_iter().collect::<ChipStack>(), stack);
    }

    #[test]
    fn rack_bets_and_receives() {
        let mut rack = Rack::with_bankroll(200);
        let bet = rack.place_bet(25).unwrap();
        assert_eq!(bet.total(), 25);
        assert_eq!(rack.total(), 175);
        rack.receive(ChipStack::change(50));
        assert_eq!(rack.total(), 225);
        assert_eq!(
            rack.place_bet(1000),
            Err(ChipError::InsufficientValue {
                requested: 1000,
                available: 225,
            })
        );
    }

    #[test]
    fn tray_settlement_pays_wins_and_keeps_losses() {
        let mut tray = Rack::with_bankroll(1000);
        // A won $25 bet: the player gets stake plus winnings back.
        let returned = tray.settle(ChipStack::change(25), 25).unwrap();
        assert_eq!(returned.total(), 50);
        assert_eq!(tray.total(), 975);
        // A lost $25 bet: the tray keeps it all.
        let returned = tray.settle(ChipStack::change(25), -25).unwrap();
        assert!(returned.is_empty());
        assert_eq!(tray.total(), 1000);
        // A surrendered $25 bet: net -13, so $12 comes back.
        let returned = tray.settle(ChipStack::change(25), -13).unwrap();
        assert_eq!(returned.total(), 12);
        assert_eq!(tray.total(), 1013);
        // A pushed bet comes straight back.
        let returned = tray.settle(ChipStack::change(25), 0).unwrap();
        assert_eq!(returned.total(), 25);
        assert_eq!(tray.total(), 1013);
    }

    #[test]
    #[should_panic(expected = "exceeds")]
    fn settlement_below_the_stake_panics() {
        let mut tray = Rack::with_bankroll(1000);
        let _ = tray.settle(ChipStack::change(25), -26);
    }

    #[test]
    fn rack_colors_up_in_place() {
        let mut rack = Rack::from_chips(
            [
                Denomination::One,
                Denomination::One,
                Denomination::One,
                Denomination::One,
                Denomination::One,
            ]
            .into_iter()
            .collect(),
        );
        rack.color_up();
        assert_eq!(rack.total(), 5);
        assert_eq!(rack.chips().count(Denomination::Five), 1);
        assert_eq!(rack.chips().chip_count(), 1);
    }

    #[test]
    fn chip_types_serde_round_trip() {
        let stack = ChipStack::change(641);
        let json = serde_json::to_string(&stack).unwrap();
        assert_eq!(serde_json::from_str::<ChipStack>(&json).unwrap(), stack);

        let rack = Rack::with_bankroll(500);
        let json = serde_json::to_string(&rack).unwrap();
        assert_eq!(serde_json::from_str::<Rack>(&json).unwrap(), rack);

        for denomination in Denomination::ALL {
            let json = serde_json::to_string(&denomination).unwrap();
            assert_eq!(
                serde_json::from_str::<Denomination>(&json).unwrap(),
                denomination
            );
        }

        let error = ChipError::InsufficientValue {
            requested: 10,
            available: 5,
        };
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(serde_json::from_str::<ChipError>(&json).unwrap(), error);
    }

    #[test]
    fn chip_errors_render_useful_messages() {
        assert_eq!(
            ChipError::InsufficientValue {
                requested: 100,
                available: 75,
            }
            .to_string(),
            "cannot withdraw $100 from a $75 stack"
        );
        assert_eq!(
            ChipError::InsufficientChips {
                denomination: Denomination::Five,
                requested: 3,
                available: 2,
            }
            .to_string(),
            "cannot remove 3 Five chips from a stack holding 2"
        );
    }
}
