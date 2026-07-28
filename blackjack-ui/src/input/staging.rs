//! Bet staging: the UI-local pile of chips building up in the betting
//! circle before it is posted as one `PlaceBet`.
//!
//! All pure functions over [`ChipStack`]s. Every function takes the
//! human's real rack (from the session view): the stage is cut from
//! those chips and nothing else. The invariants:
//!
//! - the staged total never exceeds the table maximum **or the rack** —
//!   adds and cuts beyond either are refused;
//! - a chip can be dragged in only while an unstaged chip of that
//!   denomination remains in the rack ([`rack_after_staging`]);
//! - only denominations actually present in the stage can be dragged
//!   back out — the stage never "makes change" on removal;
//! - keyboard cuts ([`stage_nudge`], [`stage_units`]) withdraw from the
//!   rack's actual chips, breaking a larger chip when they must, exactly
//!   as posting the bet would ([`Rack::place_bet`](
//!   blackjack_core::Rack::place_bet));
//! - a stage is postable ([`stage_ready`]) only in `[min_bet, max_bet]`.
//!
//! When the rack cannot cover the table minimum, no stage can ever
//! become postable — betting is impossible, and the session layer turns
//! that into game over.

use blackjack_core::{ChipStack, Denomination};

/// The rack as the player sees it while staging: the real rack minus
/// the chips already staged. This is what renders on the rail and what
/// [`grab_at`](super::gesture::grab_at) offers for grabbing.
///
/// When the stage holds exactly chips the rack has, they are removed
/// chip for chip. A keyboard cut may have broken a large chip
/// ([`stage_nudge`]); then the view falls back to withdrawing the staged
/// value, showing the change the break produced.
pub fn rack_after_staging(rack: &ChipStack, staged: &ChipStack) -> ChipStack {
    let mut rest = rack.clone();
    if staged
        .iter()
        .all(|(denomination, count)| rest.remove_chips(denomination, count).is_ok())
    {
        return rest;
    }
    let mut rest = rack.clone();
    match rest.withdraw(staged.total()) {
        Ok(_) => rest,
        // A stage worth more than the rack cannot arise through this
        // module; render the untouched rack rather than panicking.
        Err(_) => rack.clone(),
    }
}

/// Add one chip of `denomination` to the stage, refusing (returning the
/// stage unchanged) if that would push past `max_bet` or if no unstaged
/// chip of that denomination remains in `rack`.
pub fn stage_add(
    staged: &ChipStack,
    denomination: Denomination,
    max_bet: u32,
    rack: &ChipStack,
) -> ChipStack {
    let mut next = staged.clone();
    let available = rack_after_staging(rack, staged).count(denomination) > 0;
    if available && staged.total() + denomination.value() <= max_bet {
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

/// Cut exactly `target` dollars from the rack's actual chips (breaking
/// a larger chip when needed, as the cage would). `target` must not
/// exceed the rack; an impossible cut stages nothing.
fn stage_cut(rack: &ChipStack, target: u32) -> ChipStack {
    let mut remaining = rack.clone();
    remaining.withdraw(target).unwrap_or_default()
}

/// Nudge the staged amount by one table-minimum increment (up or down)
/// and recut it from the rack's chips.
///
/// Going up from an empty stage lands on the minimum; coming down from
/// (or through) the minimum clears the stage; the maximum — or the
/// rack's total, whichever is smaller — caps the top. A rack below the
/// table minimum can never nudge up to a postable stage.
pub fn stage_nudge(
    staged: &ChipStack,
    up: bool,
    min_bet: u32,
    max_bet: u32,
    rack: &ChipStack,
) -> ChipStack {
    let cap = max_bet.min(rack.total());
    let total = staged.total();
    let target = if up {
        if cap < min_bet {
            0
        } else {
            (total + min_bet).clamp(min_bet, cap)
        }
    } else if total > min_bet {
        (total - min_bet).max(min_bet).min(cap)
    } else {
        0
    };
    stage_cut(rack, target)
}

/// Set the stage to `units` table minimums (the number-key shortcut),
/// capped at the maximum and at the rack's total, cut from the rack's
/// chips. A rack below the table minimum stages nothing.
pub fn stage_units(units: u32, min_bet: u32, max_bet: u32, rack: &ChipStack) -> ChipStack {
    let target = (units * min_bet).min(max_bet).min(rack.total());
    if target < min_bet {
        return ChipStack::new();
    }
    stage_cut(rack, target)
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

    /// A roomy rack that never constrains: the pre-#12 behavior.
    fn deep_rack() -> ChipStack {
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::One, 50);
        rack.add_chips(Denomination::Five, 50);
        rack.add_chips(Denomination::TwentyFive, 50);
        rack.add_chips(Denomination::Hundred, 50);
        rack
    }

    #[test]
    fn adds_clamp_at_the_table_maximum() {
        let rack = deep_rack();
        let mut staged = ChipStack::new();
        for _ in 0..4 {
            staged = stage_add(&staged, Denomination::Hundred, MAX, &rack);
        }
        assert_eq!(staged.total(), 400);
        // One more black chip fits exactly.
        staged = stage_add(&staged, Denomination::Hundred, MAX, &rack);
        assert_eq!(staged.total(), 500);
        // Anything more is refused, even a white chip.
        let refused = stage_add(&staged, Denomination::One, MAX, &rack);
        assert_eq!(refused.total(), 500);
        assert_eq!(refused, staged);
    }

    #[test]
    fn adds_stop_at_the_rack_chips() {
        // Two red chips in the rack: the third add is refused.
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::Five, 2);
        let mut staged = ChipStack::new();
        staged = stage_add(&staged, Denomination::Five, MAX, &rack);
        staged = stage_add(&staged, Denomination::Five, MAX, &rack);
        assert_eq!(staged.total(), 10);
        let refused = stage_add(&staged, Denomination::Five, MAX, &rack);
        assert_eq!(refused, staged);
        // A denomination the rack never held is refused outright.
        let refused = stage_add(&staged, Denomination::Hundred, MAX, &rack);
        assert_eq!(refused, staged);
    }

    #[test]
    fn the_visible_rack_depletes_as_the_stage_grows() {
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::Five, 3);
        rack.add_chips(Denomination::TwentyFive, 1);
        let mut staged = ChipStack::new();
        staged.add_chips(Denomination::Five, 2);
        let visible = rack_after_staging(&rack, &staged);
        assert_eq!(visible.count(Denomination::Five), 1);
        assert_eq!(visible.count(Denomination::TwentyFive), 1);
        assert_eq!(visible.total(), rack.total() - staged.total());
        // An empty stage leaves the rack untouched.
        assert_eq!(rack_after_staging(&rack, &ChipStack::new()), rack);
    }

    #[test]
    fn the_visible_rack_survives_broken_change() {
        // The rack is one black chip; a keyboard cut staged $10 as two
        // red chips the rack never held. The view shows the $90 of
        // change the break produced, not a panic.
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::Hundred, 1);
        let staged = stage_units(1, MIN, MAX, &rack);
        assert_eq!(staged.total(), 10);
        assert_eq!(staged.count(Denomination::Five), 2);
        let visible = rack_after_staging(&rack, &staged);
        assert_eq!(visible.total(), 90);
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
        let rack = deep_rack();
        // Up from empty: the minimum.
        let staged = stage_nudge(&ChipStack::new(), true, MIN, MAX, &rack);
        assert_eq!(staged.total(), MIN);
        // Up again: one increment more.
        let staged = stage_nudge(&staged, true, MIN, MAX, &rack);
        assert_eq!(staged.total(), 2 * MIN);
        // Down twice: back through the minimum to empty.
        let staged = stage_nudge(&staged, false, MIN, MAX, &rack);
        assert_eq!(staged.total(), MIN);
        let staged = stage_nudge(&staged, false, MIN, MAX, &rack);
        assert_eq!(staged.total(), 0);
        // Down from an off-increment stage never lands below the
        // minimum while chips remain.
        let odd = ChipStack::change(15);
        assert_eq!(stage_nudge(&odd, false, MIN, MAX, &rack).total(), MIN);
        // Up saturates at the maximum.
        let big = ChipStack::change(MAX);
        assert_eq!(stage_nudge(&big, true, MIN, MAX, &rack).total(), MAX);
    }

    #[test]
    fn nudges_saturate_at_the_rack() {
        // $35 in the rack: nudging up walks 10, 20, 30, 35 and stops.
        let rack = ChipStack::change(35);
        let mut staged = ChipStack::new();
        for expected in [10, 20, 30, 35, 35] {
            staged = stage_nudge(&staged, true, MIN, MAX, &rack);
            assert_eq!(staged.total(), expected);
        }
        // A rack below the minimum can never nudge up a postable stage.
        let broke = ChipStack::change(5);
        assert!(stage_nudge(&ChipStack::new(), true, MIN, MAX, &broke).is_empty());
    }

    #[test]
    fn unit_shortcuts_cap_at_the_maximum_and_the_rack() {
        let rack = deep_rack();
        assert_eq!(stage_units(1, MIN, MAX, &rack).total(), 10);
        assert_eq!(stage_units(9, MIN, MAX, &rack).total(), 90);
        assert_eq!(stage_units(99, MIN, MAX, &rack).total(), MAX);
        // The rack caps below the table maximum.
        let thin = ChipStack::change(45);
        assert_eq!(stage_units(9, MIN, MAX, &thin).total(), 45);
        // A rack below the minimum stages nothing at all.
        let broke = ChipStack::change(5);
        assert!(stage_units(9, MIN, MAX, &broke).is_empty());
    }

    #[test]
    fn keyboard_cuts_come_from_the_rack_chips() {
        // A green-only rack cuts $50 as two greens, not as change the
        // rack does not hold.
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::TwentyFive, 4);
        let staged = stage_units(5, MIN, MAX, &rack);
        assert_eq!(staged.total(), 50);
        assert_eq!(staged.count(Denomination::TwentyFive), 2);
        assert_eq!(staged.chip_count(), 2);
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
