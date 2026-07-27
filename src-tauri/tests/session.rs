//! Drives full rounds through the Tauri command layer — the same plain
//! functions the one-line `#[tauri::command]` wrappers call — asserting
//! on the returned snapshots and events.

use blackjack_core::{Action, ActionError, ActionKind, Event, Phase, Rules};
use blackjack_protocol::BackendError;
use casino_blackjack_lib::session::{
    SessionState, snapshot, start_session, start_session_with_seed, submit_action,
};

#[test]
fn calls_before_start_session_report_no_session() {
    let state = SessionState::default();
    assert_eq!(snapshot(&state).unwrap_err(), BackendError::NoSession);
    assert_eq!(
        submit_action(&state, 0, Action::PlaceBet(10)).unwrap_err(),
        BackendError::NoSession
    );
}

#[test]
fn start_session_opens_betting_under_canonical_rules() {
    let state = SessionState::default();
    let opened = start_session(&state).unwrap();
    assert_eq!(opened.snapshot.phase, Phase::Betting);
    assert_eq!(opened.snapshot.rules, Rules::canonical());
    assert_eq!(opened.snapshot.legal_actions, [ActionKind::PlaceBet]);
    assert!(opened.events.is_empty());
    // snapshot() reads back the identical state without changing it.
    let read = snapshot(&state).unwrap();
    assert_eq!(read.snapshot, opened.snapshot);
    assert!(read.events.is_empty());
}

#[test]
fn starting_a_new_session_replaces_the_old_table() {
    let state = SessionState::default();
    start_session_with_seed(&state, 1).unwrap();
    submit_action(&state, 0, Action::PlaceBet(10)).unwrap();
    let fresh = start_session_with_seed(&state, 2).unwrap();
    assert_eq!(fresh.snapshot.phase, Phase::Betting);
    assert_eq!(fresh.snapshot.seats[0].bet, None);
}

#[test]
fn rejected_actions_surface_the_engine_error_and_mutate_nothing() {
    let state = SessionState::default();
    start_session_with_seed(&state, 7).unwrap();
    let before = snapshot(&state).unwrap().snapshot;
    let err = submit_action(&state, 0, Action::PlaceBet(5)).unwrap_err();
    assert_eq!(
        err,
        BackendError::Rejected(ActionError::BetOutOfRange {
            bet: 5,
            min: 10,
            max: 500
        })
    );
    assert_eq!(snapshot(&state).unwrap().snapshot, before);
}

/// Bet, deal, and play every decision the engine rests on (declining
/// insurance, hitting small totals, standing otherwise) until the round
/// settles, then clear the felt — all through the command layer.
#[test]
fn a_scripted_round_runs_from_betting_to_settlement() {
    let state = SessionState::default();
    start_session_with_seed(&state, 0).unwrap();

    let bet = submit_action(&state, 0, Action::PlaceBet(25)).unwrap();
    assert_eq!(
        bet.events,
        [Event::BetPlaced {
            seat: 0,
            amount: 25
        }]
    );
    assert_eq!(bet.snapshot.seats[0].bet, Some(25));
    assert_eq!(
        bet.snapshot.legal_actions,
        [ActionKind::PlaceBet, ActionKind::Deal]
    );

    let mut t = submit_action(&state, 0, Action::Deal).unwrap();
    assert_eq!(t.snapshot.seats[0].hands[0].cards.len(), 2);
    assert!(t.snapshot.dealer.upcard.is_some());
    assert!(t.snapshot.dealer.hole_card_dealt);
    assert_eq!(t.snapshot.dealer.hole_card, None, "hole card stays hidden");

    let mut log = t.events.clone();
    let mut guard = 0;
    while t.snapshot.phase != Phase::RoundOver {
        guard += 1;
        assert!(guard < 32, "round failed to settle");
        let active = t
            .snapshot
            .active
            .expect("a resting round has an active hand");
        let action = match t.snapshot.phase {
            Phase::InsuranceOffer => Action::DeclineInsurance,
            Phase::PlayerTurn => {
                let hand = &t.snapshot.seats[active.seat].hands[active.hand];
                if hand.total <= 11 {
                    Action::Hit
                } else {
                    Action::Stand
                }
            }
            phase => panic!("unexpected resting phase {phase:?}"),
        };
        t = submit_action(&state, active.seat, action).unwrap();
        log.extend(t.events.iter().cloned());
    }

    // Settlement was reported as events and recorded in the snapshot.
    assert!(
        log.iter()
            .any(|e| matches!(e, Event::HandSettled { seat: 0, .. }))
    );
    assert!(log.iter().any(|e| matches!(
        e,
        Event::HoleCardRevealed { .. } | Event::DealerPeeked { blackjack: true }
    )));
    let seat = &t.snapshot.seats[0];
    let net = seat.round_net.expect("a settled seat has a round net");
    let hand_net: i32 = seat
        .hands
        .iter()
        .map(|h| h.payout.expect("a settled hand has a payout"))
        .sum();
    assert_eq!(net, hand_net, "seat net must equal the sum of hand payouts");
    assert!(seat.hands.iter().all(|h| h.outcome.is_some()));
    assert!(
        t.snapshot.dealer.total.is_some(),
        "the dealer's total is public once the round settles"
    );
    assert_eq!(t.snapshot.legal_actions, [ActionKind::NextRound]);

    // NextRound clears the felt back to betting through the same layer.
    let next = submit_action(&state, 0, Action::NextRound).unwrap();
    assert_eq!(next.snapshot.phase, Phase::Betting);
    assert!(next.snapshot.seats[0].hands.is_empty());
    assert_eq!(next.snapshot.seats[0].bet, None);
}

/// The same script settles every seed: the command layer never wedges no
/// matter how the cards fall (insurance rounds, dealer blackjacks, splits
/// are all reachable across these seeds).
#[test]
fn scripted_rounds_settle_across_many_seeds() {
    for seed in 0..25 {
        let state = SessionState::default();
        start_session_with_seed(&state, seed).unwrap();
        submit_action(&state, 0, Action::PlaceBet(10)).unwrap();
        submit_action(&state, 3, Action::PlaceBet(50)).unwrap();
        let mut t = submit_action(&state, 0, Action::Deal).unwrap();
        let mut guard = 0;
        while t.snapshot.phase != Phase::RoundOver {
            guard += 1;
            assert!(guard < 64, "seed {seed}: round failed to settle");
            let active = t.snapshot.active.expect("active hand while resting");
            let action = match t.snapshot.phase {
                Phase::InsuranceOffer => Action::DeclineInsurance,
                Phase::PlayerTurn => {
                    let hand = &t.snapshot.seats[active.seat].hands[active.hand];
                    if hand.total <= 11 {
                        Action::Hit
                    } else {
                        Action::Stand
                    }
                }
                phase => panic!("seed {seed}: unexpected phase {phase:?}"),
            };
            t = submit_action(&state, active.seat, action).unwrap();
        }
        for seat in [0, 3] {
            assert!(
                t.snapshot.seats[seat].round_net.is_some(),
                "seed {seed}: seat {seat} did not settle"
            );
        }
        let next = submit_action(&state, 0, Action::NextRound).unwrap();
        assert_eq!(next.snapshot.phase, Phase::Betting);
    }
}
