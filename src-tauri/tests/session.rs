//! Drives complete session arcs through the Tauri command layer — the
//! same plain functions the one-line `#[tauri::command]` wrappers call —
//! asserting on the returned views: buy-in, AI warm-up, chip
//! conservation, walk-away, and game over.

use blackjack_core::{Action, Awaiting, ChipStack, Phase, Snapshot};
use blackjack_protocol::{BackendError, SessionStatus, SessionView};
use casino_blackjack_lib::session::{
    SessionState, advance, human_action, start_session_with_buy_in, start_session_with_seed, view,
    walk_away,
};

/// The human's seat under canonical rules: the center of seven.
const HUMAN: usize = 3;

/// Pump `advance` exactly as the client drive loop does: while the view
/// awaits the engine and the session is playing. Returns the resting
/// view.
fn pump(state: &SessionState) -> SessionView {
    let mut current = view(state).unwrap();
    let mut beats = 0u32;
    while current.awaiting == Awaiting::Engine && current.status == SessionStatus::Playing {
        beats += 1;
        assert!(beats < 100_000, "the pump stopped making progress");
        current = advance(state).unwrap();
    }
    current
}

/// Play the human's decisions in one round to completion — decline
/// insurance, hit small totals, stand otherwise — returning the view at
/// the next rest (the following round's bet, or a terminal status) and
/// the seat's settled `round_net`.
fn play_round_out(state: &SessionState) -> (SessionView, i32) {
    let mut current = view(state).unwrap();
    let mut net = None;
    let mut beats = 0u32;
    loop {
        beats += 1;
        assert!(beats < 1_000, "the round never settled");
        if let Some(seat_net) = current.transition.snapshot.seats[HUMAN].round_net {
            net.get_or_insert(seat_net);
        }
        if current.status != SessionStatus::Playing {
            break;
        }
        current = match current.awaiting {
            Awaiting::Engine => advance(state).unwrap(),
            Awaiting::HumanInsurance => human_action(state, Action::DeclineInsurance).unwrap(),
            Awaiting::HumanTurn => {
                let snapshot = &current.transition.snapshot;
                let active = snapshot.active.expect("human turn has an active hand");
                assert_eq!(active.seat, HUMAN);
                let hand = &snapshot.seats[HUMAN].hands[active.hand];
                let action = if hand.total <= 11 {
                    Action::Hit
                } else {
                    Action::Stand
                };
                human_action(state, action).unwrap()
            }
            Awaiting::HumanBet => break,
        };
    }
    (current, net.expect("the round settled with a net"))
}

fn human_bet(snapshot: &Snapshot) -> Option<u32> {
    snapshot.seats[HUMAN].bet
}

#[test]
fn calls_before_start_session_report_no_session() {
    let state = SessionState::default();
    assert_eq!(view(&state).unwrap_err(), BackendError::NoSession);
    assert_eq!(advance(&state).unwrap_err(), BackendError::NoSession);
    assert_eq!(
        human_action(&state, Action::PlaceBet(10)).unwrap_err(),
        BackendError::NoSession
    );
    assert_eq!(walk_away(&state).unwrap_err(), BackendError::NoSession);
}

#[test]
fn sessions_open_mid_life_and_replay_deterministically_per_seed() {
    let a = SessionState::default();
    let b = SessionState::default();
    let opened_a = start_session_with_seed(&a, 5).unwrap();
    let opened_b = start_session_with_seed(&b, 5).unwrap();
    // Identical opening views: same buy-in chips, same warmed-up table.
    assert_eq!(opened_a, opened_b);
    assert!(opened_a.transition.events.is_empty());
    assert_eq!(opened_a.status, SessionStatus::Playing);
    // The shoe is burned in from AI-only warm-up rounds.
    assert!(opened_a.transition.snapshot.shoe.cards_dealt > 0);
    // The buy-in is real chips in the documented band.
    let total = opened_a.rack.total();
    assert!((220..=380).contains(&total), "buy-in ${total} out of range");
    assert_eq!(total % 5, 0);
    // Pumping both sessions identically keeps them identical.
    assert_eq!(pump(&a), pump(&b));
    // A different seed is a different universe.
    let c = SessionState::default();
    let opened_c = start_session_with_seed(&c, 6).unwrap();
    assert_ne!(opened_a, opened_c);
}

#[test]
fn the_pump_rests_exactly_on_human_states_and_advancing_there_is_harmless() {
    let state = SessionState::default();
    start_session_with_seed(&state, 1).unwrap();
    let resting = pump(&state);
    // With a solvent rack the first rest is always the human's bet.
    assert_eq!(resting.awaiting, Awaiting::HumanBet);
    assert_eq!(resting.transition.snapshot.phase, Phase::Betting);
    assert_eq!(human_bet(&resting.transition.snapshot), None);
    // Advancing while the engine waits on the human changes nothing.
    let idle = advance(&state).unwrap();
    assert!(idle.transition.events.is_empty());
    assert_eq!(idle.awaiting, Awaiting::HumanBet);
    assert_eq!(idle.transition.snapshot, resting.transition.snapshot);
    assert_eq!(idle.rack, resting.rack);
}

#[test]
fn a_round_conserves_chips_exactly_rack_delta_equals_round_net() {
    let state = SessionState::default();
    start_session_with_seed(&state, 2).unwrap();
    let resting = pump(&state);
    assert_eq!(resting.awaiting, Awaiting::HumanBet);
    let before = resting.rack.total();

    // The bet moves real chips out of the rack immediately.
    let bet = human_action(&state, Action::PlaceBet(25)).unwrap();
    assert_eq!(bet.rack.total(), before - 25);
    assert_eq!(human_bet(&bet.transition.snapshot), Some(25));

    let (after, net) = play_round_out(&state);
    assert_eq!(
        i64::from(after.rack.total()),
        i64::from(before) + i64::from(net),
        "rack delta must equal the seat's round net"
    );
    // Life goes on: the next round is waiting on the human again.
    assert_eq!(after.awaiting, Awaiting::HumanBet);
}

#[test]
fn rounds_conserve_chips_across_many_seeds() {
    for seed in 0..15 {
        let state = SessionState::default();
        start_session_with_seed(&state, seed).unwrap();
        let mut resting = pump(&state);
        // Three consecutive rounds, betting the minimum each time.
        for round in 0..3 {
            assert_eq!(resting.awaiting, Awaiting::HumanBet, "seed {seed}");
            let before = resting.rack.total();
            human_action(&state, Action::PlaceBet(10)).unwrap();
            let (after, net) = play_round_out(&state);
            assert_eq!(
                i64::from(after.rack.total()),
                i64::from(before) + i64::from(net),
                "seed {seed} round {round}: chips leaked"
            );
            assert_eq!(after.status, SessionStatus::Playing, "seed {seed}");
            resting = after;
        }
    }
}

#[test]
fn bets_beyond_the_rack_are_rejected_server_side() {
    let state = SessionState::default();
    start_session_with_seed(&state, 3).unwrap();
    let resting = pump(&state);
    let available = resting.rack.total();
    // The table maximum ($500) always exceeds the buy-in band ($220-380),
    // so a table-legal bet can still be more money than the human has.
    let err = human_action(&state, Action::PlaceBet(500)).unwrap_err();
    assert_eq!(
        err,
        BackendError::InsufficientChips {
            requested: 500,
            available,
        }
    );
    // Nothing moved; a coverable bet still works.
    let unchanged = view(&state).unwrap();
    assert_eq!(unchanged.rack.total(), available);
    assert_eq!(human_bet(&unchanged.transition.snapshot), None);
    let bet = human_action(&state, Action::PlaceBet(10)).unwrap();
    assert_eq!(bet.rack.total(), available - 10);
}

#[test]
fn walking_away_between_rounds_colors_up_and_cashes_out() {
    let state = SessionState::default();
    start_session_with_seed(&state, 4).unwrap();
    let resting = pump(&state);
    assert_eq!(resting.awaiting, Awaiting::HumanBet);
    let dollars = resting.rack.total();

    let out = walk_away(&state).unwrap();
    assert_eq!(out.status, SessionStatus::CashedOut { dollars });
    // The dealer colored up: same value, fewest chips.
    assert_eq!(out.rack, ChipStack::change(dollars));
    assert_eq!(out.rack.total(), dollars);

    // The session is over: no more play, ever. Advancing is a quiet
    // no-op (the table's life goes on, but not on this screen).
    assert_eq!(
        human_action(&state, Action::PlaceBet(10)).unwrap_err(),
        BackendError::SessionOver
    );
    assert_eq!(walk_away(&state).unwrap_err(), BackendError::SessionOver);
    let idle = advance(&state).unwrap();
    assert!(idle.transition.events.is_empty());
    assert_eq!(idle.status, SessionStatus::CashedOut { dollars });
}

#[test]
fn walking_away_mid_round_is_refused_and_the_hand_plays_out() {
    let state = SessionState::default();
    start_session_with_seed(&state, 7).unwrap();
    pump(&state);
    // A posted bet is chips on the felt: the round is committed.
    human_action(&state, Action::PlaceBet(25)).unwrap();
    assert_eq!(
        walk_away(&state).unwrap_err(),
        BackendError::NotBetweenRounds
    );
    // Still refused at every mid-round rest until the round settles.
    let mut current = view(&state).unwrap();
    let mut beats = 0u32;
    while human_bet(&current.transition.snapshot).is_some()
        && current.transition.snapshot.seats[HUMAN].round_net.is_none()
    {
        beats += 1;
        assert!(beats < 1_000, "round never settled");
        assert_eq!(
            walk_away(&state).unwrap_err(),
            BackendError::NotBetweenRounds,
            "walk-away must be refused while the hand is live"
        );
        current = match current.awaiting {
            Awaiting::Engine => advance(&state).unwrap(),
            Awaiting::HumanInsurance => human_action(&state, Action::DeclineInsurance).unwrap(),
            Awaiting::HumanTurn => human_action(&state, Action::Stand).unwrap(),
            Awaiting::HumanBet => break,
        };
    }
    // Settled: between rounds again, and now leaving is allowed.
    let resting = pump(&state);
    assert_eq!(resting.status, SessionStatus::Playing);
    let out = walk_away(&state).unwrap();
    assert!(matches!(out.status, SessionStatus::CashedOut { .. }));
}

#[test]
fn a_felted_rack_ends_the_game_between_rounds() {
    // A pinned $15 buy-in: one all-in losing round leaves the rack
    // below the $10 minimum. Bet everything each round until it ends;
    // seeded, so the run is deterministic.
    let state = SessionState::default();
    let opened = start_session_with_buy_in(&state, 0, ChipStack::change(15), 2).unwrap();
    assert_eq!(opened.rack.total(), 15);
    let mut resting = pump(&state);
    let mut rounds = 0u32;
    while resting.status == SessionStatus::Playing {
        rounds += 1;
        assert!(rounds < 200, "the all-in run never ended");
        assert_eq!(resting.awaiting, Awaiting::HumanBet);
        let all_in = resting.rack.total().min(500);
        human_action(&state, Action::PlaceBet(all_in)).unwrap();
        let (after, _net) = play_round_out(&state);
        resting = after;
    }
    assert_eq!(resting.status, SessionStatus::GameOver);
    // Below the minimum with no bet possible: the game ended between
    // rounds, never mid-hand.
    assert!(resting.rack.total() < 10);
    assert!(
        matches!(
            resting.transition.snapshot.phase,
            Phase::RoundOver | Phase::Betting
        ),
        "game over must be evaluated between rounds"
    );
    // No play after game over — and no dollar figure is part of the
    // status either.
    assert_eq!(
        human_action(&state, Action::PlaceBet(10)).unwrap_err(),
        BackendError::SessionOver
    );
    assert_eq!(walk_away(&state).unwrap_err(), BackendError::SessionOver);
    let idle = advance(&state).unwrap();
    assert!(idle.transition.events.is_empty());
    assert_eq!(idle.status, SessionStatus::GameOver);
}

#[test]
fn starting_a_new_session_replaces_the_old_one() {
    let state = SessionState::default();
    start_session_with_seed(&state, 1).unwrap();
    pump(&state);
    human_action(&state, Action::PlaceBet(10)).unwrap();
    let fresh = start_session_with_seed(&state, 2).unwrap();
    assert_eq!(fresh.status, SessionStatus::Playing);
    assert_eq!(human_bet(&fresh.transition.snapshot), None);
    // The fresh session is byte-identical to any other from its seed.
    let other = SessionState::default();
    assert_eq!(fresh, start_session_with_seed(&other, 2).unwrap());
}

#[test]
fn engine_illegal_actions_still_surface_as_rejections() {
    let state = SessionState::default();
    start_session_with_seed(&state, 8).unwrap();
    let resting = pump(&state);
    assert_eq!(resting.awaiting, Awaiting::HumanBet);
    // Below the table minimum: the engine's own validation, unchanged.
    let err = human_action(&state, Action::PlaceBet(5)).unwrap_err();
    assert!(
        matches!(err, BackendError::Rejected(_)),
        "expected an engine rejection, got {err:?}"
    );
    // Playing out of phase is refused and mutates nothing.
    let before = view(&state).unwrap();
    let err = human_action(&state, Action::Hit).unwrap_err();
    assert!(matches!(err, BackendError::Rejected(_)));
    assert_eq!(view(&state).unwrap(), before);
}
