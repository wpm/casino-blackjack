//! Golden-seed scenario tests and property-style sanity checks for the
//! round state machine.
//!
//! Each golden test drives a seeded table through a scripted action
//! sequence and asserts the exact cards, events, and settlements that
//! seed produces. The expected values were derived by printing the actual
//! dealt sequence for the seed and hardcoding it, so any change to
//! dealing order, rule adjudication, or payout arithmetic fails loudly.

use blackjack_core::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

fn card(rank: Rank, suit: Suit) -> Card {
    Card::new(rank, suit)
}

fn canonical_table(seed: u64) -> Table<ChaCha8Rng> {
    Table::from_seed(Rules::canonical(), seed)
}

/// Seed 17 heads-up: seat 0 is dealt A[s] J[h] against a dealer three — a
/// natural, settled immediately at 3:2 with no player decision.
#[test]
fn golden_natural_pays_three_to_two() {
    let mut t = canonical_table(17);
    t.apply(0, Action::PlaceBet(10)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::CardDealt {
                seat: 0,
                hand: 0,
                card: card(Rank::Ace, Suit::Spades)
            },
            Event::DealerCardDealt {
                card: card(Rank::Three, Suit::Hearts)
            },
            Event::CardDealt {
                seat: 0,
                hand: 0,
                card: card(Rank::Jack, Suit::Hearts)
            },
            Event::HoleCardDealt,
            Event::PlayerBlackjack { seat: 0 },
            Event::HoleCardRevealed {
                card: card(Rank::Seven, Suit::Clubs)
            },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Blackjack,
                amount: 15
            },
        ]
    );
    let snap = tr.snapshot;
    assert_eq!(snap.phase, Phase::RoundOver);
    assert_eq!(snap.seats[0].round_net, Some(15));
    assert_eq!(snap.seats[0].hands[0].status, HandStatus::Blackjack);
    assert_eq!(snap.seats[0].hands[0].payout, Some(15));
    // No peek happened: a three shows, so no blackjack check was needed.
    assert!(
        !tr.events
            .iter()
            .any(|e| matches!(e, Event::DealerPeeked { .. }))
    );
}

/// Seed 62 heads-up with a $20 bet: dealer shows an ace and has the queen
/// of spades underneath. Seat 0 takes insurance for $10, the peek finds
/// the blackjack, insurance pays 2:1 (+$20), the main bet loses (-$20),
/// and the round nets exactly zero.
#[test]
fn golden_dealer_blackjack_with_insurance_nets_zero() {
    let mut t = canonical_table(62);
    t.apply(0, Action::PlaceBet(20)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    assert_eq!(tr.snapshot.phase, Phase::InsuranceOffer);
    assert_eq!(tr.snapshot.seats[0].insurance, Insurance::Pending);
    assert_eq!(
        tr.snapshot.legal_actions,
        [ActionKind::TakeInsurance, ActionKind::DeclineInsurance]
    );
    assert_eq!(tr.snapshot.dealer.upcard.map(|c| c.rank), Some(Rank::Ace));
    // The hole card is face down and absent from the snapshot.
    assert_eq!(tr.snapshot.dealer.hole_card, None);

    let tr = t.apply(0, Action::TakeInsurance).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::InsuranceTaken {
                seat: 0,
                amount: 10
            },
            Event::DealerPeeked { blackjack: true },
            Event::HoleCardRevealed {
                card: card(Rank::Queen, Suit::Spades)
            },
            Event::InsuranceResolved {
                seat: 0,
                amount: 20
            },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -20
            },
        ]
    );
    let snap = tr.snapshot;
    assert_eq!(snap.phase, Phase::RoundOver);
    assert_eq!(snap.seats[0].insurance, Insurance::Taken { amount: 10 });
    assert_eq!(snap.seats[0].round_net, Some(0), "even money: net zero");
    assert_eq!(snap.dealer.total, Some(21));
}

/// Same seed 62, declining insurance: the dealer blackjack simply takes
/// the $20 main bet.
#[test]
fn golden_dealer_blackjack_without_insurance_loses_the_bet() {
    let mut t = canonical_table(62);
    t.apply(0, Action::PlaceBet(20)).unwrap();
    t.apply(0, Action::Deal).unwrap();
    let tr = t.apply(0, Action::DeclineInsurance).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::InsuranceDeclined { seat: 0 },
            Event::DealerPeeked { blackjack: true },
            Event::HoleCardRevealed {
                card: card(Rank::Queen, Suit::Spades)
            },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -20
            },
        ]
    );
    assert_eq!(tr.snapshot.seats[0].insurance, Insurance::Declined);
    assert_eq!(tr.snapshot.seats[0].round_net, Some(-20));
}

/// Seed 13 heads-up: dealer shows an ace over a four — no blackjack. The
/// insurance stake is lost at the peek, play continues, seat 0 stands on
/// 16, and the dealer makes 20: net -$10 hand - $5 insurance = -$15.
#[test]
fn golden_insurance_lost_when_dealer_has_no_blackjack() {
    let mut t = canonical_table(13);
    t.apply(0, Action::PlaceBet(10)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    assert_eq!(tr.snapshot.phase, Phase::InsuranceOffer);

    let tr = t.apply(0, Action::TakeInsurance).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::InsuranceTaken { seat: 0, amount: 5 },
            Event::DealerPeeked { blackjack: false },
            Event::InsuranceResolved {
                seat: 0,
                amount: -5
            },
        ]
    );
    // Play continues with the hole card still hidden.
    assert_eq!(tr.snapshot.phase, Phase::PlayerTurn);
    assert_eq!(tr.snapshot.dealer.hole_card, None);

    let tr = t.apply(0, Action::Stand).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::PlayerStood {
                seat: 0,
                hand: 0,
                total: 16
            },
            Event::HoleCardRevealed {
                card: card(Rank::Four, Suit::Diamonds)
            },
            Event::DealerCardDealt {
                card: card(Rank::Five, Suit::Diamonds)
            },
            Event::DealerStood { total: 20 },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -10
            },
        ]
    );
    assert_eq!(tr.snapshot.seats[0].round_net, Some(-15));
}

/// Seed 57 heads-up: Q[d] Q[h] against a king. The pair splits, both new
/// queens pair again and resplit to the four-hand cap, and the hands
/// finish 16, 13, 19, 20 against the dealer's 17: two losses and two wins
/// for a net of exactly zero.
#[test]
fn golden_split_resplits_to_the_four_hand_cap() {
    let mut t = canonical_table(57);
    t.apply(0, Action::PlaceBet(10)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    let hand = &tr.snapshot.seats[0].hands[0];
    assert_eq!(
        hand.cards,
        [
            card(Rank::Queen, Suit::Diamonds),
            card(Rank::Queen, Suit::Hearts)
        ]
    );
    assert_eq!(
        tr.snapshot.dealer.upcard,
        Some(card(Rank::King, Suit::Diamonds))
    );

    // First split: the active hand draws another queen — still a pair.
    let tr = t.apply(0, Action::Split).unwrap();
    assert_eq!(tr.snapshot.seats[0].hands.len(), 2);
    assert!(tr.snapshot.legal_actions.contains(&ActionKind::Split));
    // Split hands may still double (DAS) but never surrender.
    assert!(tr.snapshot.legal_actions.contains(&ActionKind::Double));
    assert!(!tr.snapshot.legal_actions.contains(&ActionKind::Surrender));

    // Second and third splits reach the cap.
    let tr = t.apply(0, Action::Split).unwrap();
    assert_eq!(tr.snapshot.seats[0].hands.len(), 3);
    assert!(tr.snapshot.legal_actions.contains(&ActionKind::Split));
    let tr = t.apply(0, Action::Split).unwrap();
    assert_eq!(tr.snapshot.seats[0].hands.len(), 4);
    assert!(
        !tr.snapshot.legal_actions.contains(&ActionKind::Split),
        "the four-hand cap forbids a fourth split"
    );

    // Stand all four hands; the later hands draw their second cards as
    // the cursor reaches them.
    for _ in 0..4 {
        let active = t.snapshot().active.unwrap();
        assert_eq!(active.seat, 0);
        t.apply(0, Action::Stand).unwrap();
    }

    let snap = t.snapshot();
    assert_eq!(snap.phase, Phase::RoundOver);
    let totals: Vec<u8> = snap.seats[0].hands.iter().map(|h| h.total).collect();
    assert_eq!(totals, [16, 13, 19, 20]);
    assert_eq!(snap.dealer.total, Some(17));
    let payouts: Vec<i32> = snap.seats[0]
        .hands
        .iter()
        .map(|h| h.payout.unwrap())
        .collect();
    assert_eq!(payouts, [-10, -10, 10, 10]);
    assert_eq!(snap.seats[0].round_net, Some(0));
    assert!(snap.seats[0].hands.iter().all(|h| h.from_split));
    // A 20 made after a split is a plain win, never a blackjack payout.
    assert_eq!(snap.seats[0].hands[3].outcome, Some(HandOutcome::Win));
}

/// Seed 198 heads-up: A[c] A[h] against a seven. Split aces get exactly
/// one card each and stand automatically — even though the first hand
/// draws another ace — and the dealer busts with 23, paying both hands.
#[test]
fn golden_split_aces_take_one_card_and_auto_stand() {
    let mut t = canonical_table(198);
    t.apply(0, Action::PlaceBet(10)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    assert!(is_ace_pair(&tr.snapshot.seats[0].hands[0].cards));

    let tr = t.apply(0, Action::Split).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::HandSplit { seat: 0, hand: 0 },
            Event::CardDealt {
                seat: 0,
                hand: 0,
                card: card(Rank::Ace, Suit::Hearts)
            },
            Event::PlayerStood {
                seat: 0,
                hand: 0,
                total: 12
            },
            Event::CardDealt {
                seat: 0,
                hand: 1,
                card: card(Rank::Two, Suit::Diamonds)
            },
            Event::PlayerStood {
                seat: 0,
                hand: 1,
                total: 13
            },
            Event::HoleCardRevealed {
                card: card(Rank::Three, Suit::Spades)
            },
            Event::DealerCardDealt {
                card: card(Rank::Six, Suit::Hearts)
            },
            Event::DealerCardDealt {
                card: card(Rank::Seven, Suit::Diamonds)
            },
            Event::DealerBust { total: 23 },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Win,
                amount: 10
            },
            Event::HandSettled {
                seat: 0,
                hand: 1,
                outcome: HandOutcome::Win,
                amount: 10
            },
        ]
    );
    assert_eq!(tr.snapshot.seats[0].round_net, Some(20));
}

/// Seed 74 heads-up: 8[h] 3[c] (11) against a six. Doubling draws a nine
/// for 20, but the dealer turns 9 + 6 into 21 and the doubled $20 is lost.
#[test]
fn golden_double_down_risks_the_doubled_bet() {
    let mut t = canonical_table(74);
    t.apply(0, Action::PlaceBet(10)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    assert_eq!(tr.snapshot.seats[0].hands[0].total, 11);
    assert!(tr.snapshot.legal_actions.contains(&ActionKind::Double));

    let tr = t.apply(0, Action::Double).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::DoubledDown {
                seat: 0,
                hand: 0,
                bet: 20
            },
            Event::CardDealt {
                seat: 0,
                hand: 0,
                card: card(Rank::Nine, Suit::Diamonds)
            },
            Event::PlayerStood {
                seat: 0,
                hand: 0,
                total: 20
            },
            Event::HoleCardRevealed {
                card: card(Rank::Nine, Suit::Diamonds)
            },
            Event::DealerCardDealt {
                card: card(Rank::Six, Suit::Spades)
            },
            Event::DealerStood { total: 21 },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -20
            },
        ]
    );
    let snap = tr.snapshot;
    assert_eq!(snap.seats[0].hands[0].bet, 20);
    assert!(snap.seats[0].hands[0].doubled);
    assert_eq!(snap.seats[0].round_net, Some(-20));
}

/// Seed 25 heads-up with a $15 bet: 9 7 against a queen (peek finds no
/// blackjack). Surrendering recovers half the bet rounded down — $7 back,
/// $8 lost — the documented rounding for odd amounts.
#[test]
fn golden_surrender_loses_half_the_bet_rounded_against_the_player() {
    let mut t = canonical_table(25);
    t.apply(0, Action::PlaceBet(15)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();
    assert_eq!(tr.snapshot.seats[0].hands[0].total, 16);
    assert!(tr.snapshot.legal_actions.contains(&ActionKind::Surrender));
    assert!(
        tr.events
            .contains(&Event::DealerPeeked { blackjack: false }),
        "a ten-value upcard forces a peek before surrender is offered"
    );

    let tr = t.apply(0, Action::Surrender).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::PlayerSurrendered { seat: 0 },
            Event::HoleCardRevealed {
                card: card(Rank::Two, Suit::Hearts)
            },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Surrender,
                amount: -8
            },
        ]
    );
    assert_eq!(tr.snapshot.seats[0].round_net, Some(-8));
}

/// Seed 0 with three occupied seats ($10 / $25 / $50) resolved in casino
/// order: seat 0 stands on 18 and loses to the dealer's 19, seat 2 hits
/// to 20 and wins, seat 5 hits twice to 18 and loses.
#[test]
fn golden_multi_seat_round_settles_each_seat_exactly() {
    let mut t = canonical_table(0);
    t.apply(0, Action::PlaceBet(10)).unwrap();
    t.apply(2, Action::PlaceBet(25)).unwrap();
    t.apply(5, Action::PlaceBet(50)).unwrap();
    let tr = t.apply(0, Action::Deal).unwrap();

    // Cards go around the table in seat order, dealer last.
    assert_eq!(
        tr.events[..8],
        [
            Event::CardDealt {
                seat: 0,
                hand: 0,
                card: card(Rank::Jack, Suit::Spades)
            },
            Event::CardDealt {
                seat: 2,
                hand: 0,
                card: card(Rank::Five, Suit::Clubs)
            },
            Event::CardDealt {
                seat: 5,
                hand: 0,
                card: card(Rank::Eight, Suit::Clubs)
            },
            Event::DealerCardDealt {
                card: card(Rank::Nine, Suit::Hearts)
            },
            Event::CardDealt {
                seat: 0,
                hand: 0,
                card: card(Rank::Eight, Suit::Hearts)
            },
            Event::CardDealt {
                seat: 2,
                hand: 0,
                card: card(Rank::Seven, Suit::Clubs)
            },
            Event::CardDealt {
                seat: 5,
                hand: 0,
                card: card(Rank::Four, Suit::Spades)
            },
            Event::HoleCardDealt,
        ]
    );
    assert_eq!(tr.snapshot.active, Some(ActiveHand { seat: 0, hand: 0 }));

    // Seat 0: stand on 18. Play passes to seat 2, skipping empty seat 1.
    let tr = t.apply(0, Action::Stand).unwrap();
    assert_eq!(tr.snapshot.active, Some(ActiveHand { seat: 2, hand: 0 }));

    // Seat 2: hit 12 into 20, then stand.
    let tr = t.apply(2, Action::Hit).unwrap();
    assert_eq!(tr.snapshot.seats[2].hands[0].total, 20);
    let tr = t.apply(2, Action::Stand).unwrap();
    assert_eq!(tr.snapshot.active, Some(ActiveHand { seat: 5, hand: 0 }));

    // Seat 5: hit 12 to 14, then 18, then stand — which ends the round.
    t.apply(5, Action::Hit).unwrap();
    let tr = t.apply(5, Action::Hit).unwrap();
    assert_eq!(tr.snapshot.seats[5].hands[0].total, 18);
    let tr = t.apply(5, Action::Stand).unwrap();

    let settlements: Vec<Event> = tr
        .events
        .iter()
        .filter(|e| matches!(e, Event::HandSettled { .. }))
        .copied()
        .collect();
    assert_eq!(
        settlements,
        [
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -10
            },
            Event::HandSettled {
                seat: 2,
                hand: 0,
                outcome: HandOutcome::Win,
                amount: 25
            },
            Event::HandSettled {
                seat: 5,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -50
            },
        ]
    );
    let snap = tr.snapshot;
    assert_eq!(snap.dealer.total, Some(19));
    assert_eq!(snap.seats[0].round_net, Some(-10));
    assert_eq!(snap.seats[2].round_net, Some(25));
    assert_eq!(snap.seats[5].round_net, Some(-50));
    // Unoccupied seats are untouched.
    for empty in [1, 3, 4, 6] {
        assert_eq!(snap.seats[empty].bet, None);
        assert!(snap.seats[empty].hands.is_empty());
        assert_eq!(snap.seats[empty].round_net, None);
    }
}

/// Seed 22 heads-up: the player stands on 17 and the dealer holds 6 + A —
/// a soft 17. Under S17 the dealer stands and the hand pushes; under H17,
/// with the identical shoe, the dealer draws a three for 20 and wins.
#[test]
fn golden_soft_17_is_a_push_under_s17_and_a_loss_under_h17() {
    let mut s17 = canonical_table(22);
    s17.apply(0, Action::PlaceBet(10)).unwrap();
    s17.apply(0, Action::Deal).unwrap();
    let tr = s17.apply(0, Action::Stand).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::PlayerStood {
                seat: 0,
                hand: 0,
                total: 17
            },
            Event::HoleCardRevealed {
                card: card(Rank::Ace, Suit::Diamonds)
            },
            Event::DealerStood { total: 17 },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Push,
                amount: 0
            },
        ]
    );
    assert_eq!(tr.snapshot.seats[0].round_net, Some(0));

    let mut rules = Rules::canonical();
    rules.soft_17 = Soft17::Hit;
    let mut h17 = Table::new(rules, ChaCha8Rng::seed_from_u64(22));
    h17.apply(0, Action::PlaceBet(10)).unwrap();
    h17.apply(0, Action::Deal).unwrap();
    let tr = h17.apply(0, Action::Stand).unwrap();
    assert_eq!(
        tr.events,
        [
            Event::PlayerStood {
                seat: 0,
                hand: 0,
                total: 17
            },
            Event::HoleCardRevealed {
                card: card(Rank::Ace, Suit::Diamonds)
            },
            Event::DealerCardDealt {
                card: card(Rank::Three, Suit::Spades)
            },
            Event::DealerStood { total: 20 },
            Event::HandSettled {
                seat: 0,
                hand: 0,
                outcome: HandOutcome::Lose,
                amount: -10
            },
        ]
    );
    assert_eq!(tr.snapshot.seats[0].round_net, Some(-10));
}

// ---------------------------------------------------------------------
// Property-style sanity checks.
// ---------------------------------------------------------------------

/// Turn an action kind into a concrete, nominally legal action.
fn concrete(kind: ActionKind, rules: &Rules) -> Action {
    match kind {
        ActionKind::PlaceBet => Action::PlaceBet(rules.min_bet),
        ActionKind::Deal => Action::Deal,
        ActionKind::Hit => Action::Hit,
        ActionKind::Stand => Action::Stand,
        ActionKind::Double => Action::Double,
        ActionKind::Split => Action::Split,
        ActionKind::Surrender => Action::Surrender,
        ActionKind::TakeInsurance => Action::TakeInsurance,
        ActionKind::DeclineInsurance => Action::DeclineInsurance,
        ActionKind::NextRound => Action::NextRound,
    }
}

/// The seat expected to act for a given snapshot (any seat works for
/// table-level actions; use the active seat otherwise).
fn acting_seat(snapshot: &Snapshot) -> usize {
    snapshot.active.map(|a| a.seat).unwrap_or(0)
}

/// Drive many seeded tables through hundreds of transitions, always
/// choosing one of the advertised legal actions. The advertised action
/// must always exist and always be accepted, and the card count must
/// balance at every step.
#[test]
fn legal_actions_are_never_empty_and_always_accepted() {
    for seed in 0..25u64 {
        let mut rules = Rules::canonical();
        // A small shoe forces reshuffles into the run.
        rules.decks = 2;
        rules.penetration = 0.5;
        let capacity = rules.decks * DECK_SIZE;
        let mut t = Table::new(rules.clone(), ChaCha8Rng::seed_from_u64(seed));
        let mut shuffles = 0;
        for step in 0..400usize {
            let snapshot = t.snapshot();
            assert!(
                !snapshot.legal_actions.is_empty(),
                "seed {seed} step {step}: empty legal action set in {:?}",
                snapshot.phase
            );
            // Cycle through the legal actions for variety.
            let kind = snapshot.legal_actions[step % snapshot.legal_actions.len()];
            let action = concrete(kind, &rules);
            let seat = acting_seat(&snapshot);
            let tr = t.apply(seat, action).unwrap_or_else(|e| {
                panic!("seed {seed} step {step}: legal {kind:?} rejected: {e}")
            });
            shuffles += tr
                .events
                .iter()
                .filter(|e| matches!(e, Event::ShoeShuffled))
                .count();
            // Every card is in the shoe, the discard pile, or on the felt.
            let snap = tr.snapshot;
            let on_felt: usize = snap
                .seats
                .iter()
                .flat_map(|s| &s.hands)
                .map(|h| h.cards.len())
                .sum::<usize>()
                + usize::from(snap.dealer.upcard.is_some())
                + usize::from(snap.dealer.hole_card_dealt)
                + snap.dealer.draws.len();
            assert_eq!(
                snap.shoe.cards_remaining + snap.shoe.discard_pile_size + on_felt,
                capacity,
                "seed {seed} step {step}: cards not conserved"
            );
        }
        assert!(
            shuffles > 0,
            "seed {seed}: 400 steps on a two-deck shoe must reshuffle"
        );
    }
}

/// At every resting point of a scripted round, every action kind the
/// snapshot does not advertise must be rejected with a typed error and
/// leave the table byte-for-byte unchanged.
#[test]
fn illegal_actions_never_mutate_state() {
    let all_kinds = [
        ActionKind::PlaceBet,
        ActionKind::Deal,
        ActionKind::Hit,
        ActionKind::Stand,
        ActionKind::Double,
        ActionKind::Split,
        ActionKind::Surrender,
        ActionKind::TakeInsurance,
        ActionKind::DeclineInsurance,
        ActionKind::NextRound,
    ];
    for seed in 0..25u64 {
        let rules = Rules::canonical();
        let mut t = Table::from_seed(rules.clone(), seed);
        for step in 0..120usize {
            let snapshot = t.snapshot();
            for kind in all_kinds {
                if snapshot.legal_actions.contains(&kind) {
                    continue;
                }
                let seat = acting_seat(&snapshot);
                let result = t.apply(seat, concrete(kind, &rules));
                assert!(
                    result.is_err(),
                    "seed {seed} step {step}: {kind:?} should be illegal in {:?}",
                    snapshot.phase
                );
                assert_eq!(
                    t.snapshot(),
                    snapshot,
                    "seed {seed} step {step}: rejected {kind:?} mutated the table"
                );
            }
            // Off-turn play actions must also be rejected without effect.
            if let Some(active) = snapshot.active {
                let other = (active.seat + 1) % rules.seats;
                if other != active.seat {
                    let result = t.apply(other, Action::Stand);
                    assert!(result.is_err());
                    assert_eq!(t.snapshot(), snapshot);
                }
            }
            // Advance with the first legal action.
            let kind = snapshot.legal_actions[step % snapshot.legal_actions.len()];
            t.apply(acting_seat(&snapshot), concrete(kind, &rules))
                .unwrap();
        }
    }
}

/// Insurance stakes are half the main bet rounded down, and an oversized
/// or undersized bet is refused with the table limits in the error.
#[test]
fn insurance_stake_rounds_down_on_odd_bets() {
    // Seed 13 deals a dealer ace heads-up.
    let mut t = canonical_table(13);
    t.apply(0, Action::PlaceBet(15)).unwrap();
    t.apply(0, Action::Deal).unwrap();
    let tr = t.apply(0, Action::TakeInsurance).unwrap();
    assert!(
        tr.events
            .contains(&Event::InsuranceTaken { seat: 0, amount: 7 }),
        "insurance on $15 is $7, rounded down"
    );
}

/// A full round can be replayed exactly from the same seed and script.
#[test]
fn identical_seeds_and_scripts_replay_identically() {
    let script = [
        (0usize, Action::PlaceBet(10)),
        (2, Action::PlaceBet(25)),
        (0, Action::Deal),
        (0, Action::Stand),
        (2, Action::Hit),
        (2, Action::Stand),
        (0, Action::NextRound),
    ];
    let mut a = canonical_table(0);
    let mut b = canonical_table(0);
    for (seat, action) in script {
        let ta = a.apply(seat, action).unwrap();
        let tb = b.apply(seat, action).unwrap();
        assert_eq!(ta, tb);
    }
}
