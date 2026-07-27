//! Bet staging: the UI-local pile of chips building up in the betting
//! circle before it is posted as one `PlaceBet`.
//!
//! All pure functions over [`ChipStack`]s. The invariants:
//!
//! - the staged total never exceeds the table maximum (adds that would
//!   overshoot are refused, chip returned);
//! - only denominations actually present in the stage can be removed —
//!   the stage never "makes change";
//! - a stage is postable ([`stage_ready`]) only in `[min_bet, max_bet]`.

use blackjack_core::{ChipStack, Denomination};

/// Add one chip of `denomination` to the stage, refusing (returning the
/// stage unchanged) if that would push past `max_bet`.
pub fn stage_add(staged: &ChipStack, denomination: Denomination, max_bet: u32) -> ChipStack {
    let mut next = staged.clone();
    if staged.total() + denomination.value() <= max_bet {
        next.add_chips(denomination, 1);
    }
    next
}

/// Remove one chip of `denomination` from the stage, if present.
pub fn stage_remove(staged: &ChipStack, denomination: Denomination) -> ChipStack {
    let mut next = staged.clone();
    let _ = next.remove_chips(denomination, 1);
    next
}

/// The stage's top chip — the smallest denomination present, since
/// piles cut largest-on-the-bottom — or `None` for an empty stage.
pub fn stage_top(staged: &ChipStack) -> Option<Denomination> {
    Denomination::ALL.into_iter().find(|&d| staged.count(d) > 0)
}

/// Nudge the staged amount by one table-minimum increment (up or down)
/// and recut it into chips.
///
/// Going up from an empty stage lands on the minimum; coming down from
/// (or through) the minimum clears the stage; the maximum caps the top.
pub fn stage_nudge(staged: &ChipStack, up: bool, min_bet: u32, max_bet: u32) -> ChipStack {
    let total = staged.total();
    let next = if up {
        (total + min_bet).clamp(min_bet, max_bet)
    } else if total > min_bet {
        (total - min_bet).max(min_bet)
    } else {
        0
    };
    ChipStack::change(next)
}

/// Set the stage to `units` table minimums (the number-key shortcut),
/// capped at the maximum, cut into chips.
pub fn stage_units(units: u32, min_bet: u32, max_bet: u32) -> ChipStack {
    ChipStack::change((units * min_bet).min(max_bet))
}

/// Whether a staged amount is postable under the table limits.
pub fn stage_ready(total: u32, min_bet: u32, max_bet: u32) -> bool {
    (min_bet..=max_bet).contains(&total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u32 = 10;
    const MAX: u32 = 500;

    #[test]
    fn adds_clamp_at_the_table_maximum() {
        let mut staged = ChipStack::new();
        for _ in 0..4 {
            staged = stage_add(&staged, Denomination::Hundred, MAX);
        }
        assert_eq!(staged.total(), 400);
        // One more black chip fits exactly.
        staged = stage_add(&staged, Denomination::Hundred, MAX);
        assert_eq!(staged.total(), 500);
        // Anything more is refused, even a white chip.
        let refused = stage_add(&staged, Denomination::One, MAX);
        assert_eq!(refused.total(), 500);
        assert_eq!(refused, staged);
    }

    #[test]
    fn removes_only_present_denominations() {
        let mut staged = ChipStack::new();
        staged.add_chips(Denomination::TwentyFive, 2);
        // Removing a denomination that is not there changes nothing —
        // the stage never makes change.
        assert_eq!(stage_remove(&staged, Denomination::Five), staged);
        let less = stage_remove(&staged, Denomination::TwentyFive);
        assert_eq!(less.total(), 25);
    }

    #[test]
    fn the_top_chip_is_the_smallest_denomination() {
        let mut staged = ChipStack::new();
        staged.add_chips(Denomination::Hundred, 1);
        staged.add_chips(Denomination::Five, 2);
        assert_eq!(stage_top(&staged), Some(Denomination::Five));
        assert_eq!(stage_top(&ChipStack::new()), None);
    }

    #[test]
    fn nudges_snap_to_the_limits() {
        // Up from empty: the minimum.
        let staged = stage_nudge(&ChipStack::new(), true, MIN, MAX);
        assert_eq!(staged.total(), MIN);
        // Up again: one increment more.
        let staged = stage_nudge(&staged, true, MIN, MAX);
        assert_eq!(staged.total(), 2 * MIN);
        // Down twice: back through the minimum to empty.
        let staged = stage_nudge(&staged, false, MIN, MAX);
        assert_eq!(staged.total(), MIN);
        let staged = stage_nudge(&staged, false, MIN, MAX);
        assert_eq!(staged.total(), 0);
        // Down from an off-increment stage never lands below the
        // minimum while chips remain.
        let odd = ChipStack::change(15);
        assert_eq!(stage_nudge(&odd, false, MIN, MAX).total(), MIN);
        // Up saturates at the maximum.
        let big = ChipStack::change(MAX);
        assert_eq!(stage_nudge(&big, true, MIN, MAX).total(), MAX);
    }

    #[test]
    fn unit_shortcuts_cap_at_the_maximum() {
        assert_eq!(stage_units(1, MIN, MAX).total(), 10);
        assert_eq!(stage_units(9, MIN, MAX).total(), 90);
        assert_eq!(stage_units(99, MIN, MAX).total(), MAX);
    }

    #[test]
    fn readiness_is_exactly_the_table_limits() {
        assert!(!stage_ready(0, MIN, MAX));
        assert!(!stage_ready(MIN - 1, MIN, MAX));
        assert!(stage_ready(MIN, MIN, MAX));
        assert!(stage_ready(MAX, MIN, MAX));
        assert!(!stage_ready(MAX + 1, MIN, MAX));
    }
}
