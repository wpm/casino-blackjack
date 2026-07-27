//! Host-side tests for the session-arc plumbing the UI builds on: the
//! bankroll generator's bounds and cuts, and the view→staging rack flow
//! (the session view's real chips feeding the input layer's staging).

use blackjack_core::{Awaiting, ChipStack, Denomination};
use blackjack_protocol::{SessionArc, SessionStatus, buy_in};
use blackjack_ui::input::staging::{rack_after_staging, stage_add, stage_ready, stage_units};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;

#[test]
fn buy_ins_land_in_the_documented_band_in_five_dollar_steps() {
    let mut rng = ChaCha8Rng::seed_from_u64(99);
    for _ in 0..500 {
        let stack = buy_in(&mut rng);
        let total = stack.total();
        assert!((220..=380).contains(&total), "buy-in ${total} out of band");
        assert_eq!(total % 5, 0, "buy-in ${total} is not a $5 step");
        assert!(!stack.is_empty());
    }
}

#[test]
fn buy_in_cuts_vary_not_always_clean_change() {
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    let mut tidy = 0u32;
    let mut with_black = 0u32;
    let mut red_heavy = 0u32;
    const N: u32 = 400;
    for _ in 0..N {
        let stack = buy_in(&mut rng);
        if stack == ChipStack::change(stack.total()) {
            tidy += 1;
        }
        if stack.count(Denomination::Hundred) > 0 {
            with_black += 1;
        }
        if stack.count(Denomination::Five) >= 15 {
            red_heavy += 1;
        }
    }
    assert!(tidy > 0, "the cage's clean cut never appeared");
    assert!(tidy < N, "every stack was clean change");
    assert!(with_black > 0, "no stack ever carried a black chip");
    assert!(red_heavy > 0, "no stack was ever $5-heavy");
}

#[test]
fn the_session_views_rack_feeds_staging_directly() {
    // A deterministic session waiting on the human's first bet.
    let mut session = SessionArc::from_seed(12);
    let mut view = session.view();
    let mut beats = 0u32;
    while view.awaiting == Awaiting::Engine && view.status == SessionStatus::Playing {
        beats += 1;
        assert!(beats < 100_000, "session never rested on the human");
        view = session.advance();
    }
    assert_eq!(view.awaiting, Awaiting::HumanBet);

    // The view's rack is the staging truth: cuts come from those chips
    // and deplete the rendered rack chip for chip.
    let rack = view.rack.clone();
    let min = view.transition.snapshot.rules.min_bet;
    let max = view.transition.snapshot.rules.max_bet;
    let staged = stage_units(1, min, max, &rack);
    assert!(stage_ready(staged.total(), min, max));
    let visible = rack_after_staging(&rack, &staged);
    assert_eq!(visible.total(), rack.total() - staged.total());

    // Staging can never exceed the rack: adds refuse once the last chip
    // of a denomination is staged.
    let denomination = rack.iter().next().unwrap().0;
    let mut staged = ChipStack::new();
    for _ in 0..rack.count(denomination) {
        let next = stage_add(&staged, denomination, u32::MAX, &rack);
        assert_ne!(next, staged, "an available chip was refused");
        staged = next;
    }
    let refused = stage_add(&staged, denomination, u32::MAX, &rack);
    assert_eq!(refused, staged, "staging exceeded the rack");
}
