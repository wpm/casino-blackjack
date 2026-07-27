//! Property tests for the chip layer and its integration with the round
//! engine: change making, color-up, chip conservation across settlements,
//! settlement bounds, and determinism.

use std::collections::BTreeMap;

use proptest::prelude::*;

use blackjack_core::{
    Action, ActionKind, ChipStack, Denomination, Event, HandOutcome, Phase, Rack, Rules, Snapshot,
    Table,
};

/// Starting bankroll per betting seat: covers the worst case of three
/// rounds each losing four doubled max-bet split hands plus insurance.
const BANKROLL: u32 = 20_000;

/// Starting house tray, comfortably covering every payout the driver can
/// produce.
const TRAY: u32 = 100_000;

/// One settled round as observed from outside the engine.
#[derive(Debug, Clone, PartialEq)]
struct RoundLog {
    /// Events from the deal through settlement, in order.
    events: Vec<Event>,
    /// The snapshot at settlement (phase `RoundOver`).
    snapshot: Snapshot,
    /// Each betting seat's total chips wagered onto the felt this round
    /// (main bets, doubles, splits, insurance).
    wagered: BTreeMap<usize, u32>,
    /// Each betting seat's rack total before betting.
    rack_before: BTreeMap<usize, u32>,
    /// Each betting seat's rack total after settlement.
    rack_after: BTreeMap<usize, u32>,
    /// The tray total after settlement.
    tray_after: u32,
}

/// A full scripted session as observed from outside the engine.
#[derive(Debug, Clone, PartialEq)]
struct SessionLog {
    rounds: Vec<RoundLog>,
    /// Final chip contents of every rack, by seat.
    final_racks: BTreeMap<usize, ChipStack>,
    /// Final chip contents of the tray.
    final_tray: ChipStack,
}

/// Drive a seeded table through `rounds` rounds. The same seats place the
/// same bets each round; every decision (insurance, play) is picked from
/// the engine's own `legal_actions` by consuming `script` bytes, so every
/// scripted action is legal by construction. Chips move through per-seat
/// racks and a house tray exactly as they would across a real felt.
fn run_script(seed: u64, bets: &BTreeMap<usize, u32>, script: &[u8], rounds: usize) -> SessionLog {
    let mut table = Table::from_seed(Rules::canonical(), seed);
    let mut racks: BTreeMap<usize, Rack> = bets
        .keys()
        .map(|&seat| (seat, Rack::with_bankroll(BANKROLL)))
        .collect();
    let mut tray = Rack::with_bankroll(TRAY);
    let mut cursor = 0usize;
    let mut next_byte = move || {
        let byte = script[cursor % script.len()];
        cursor += 1;
        byte as usize
    };
    let mut log = SessionLog {
        rounds: Vec::new(),
        final_racks: BTreeMap::new(),
        final_tray: ChipStack::new(),
    };

    for _ in 0..rounds {
        let rack_before: BTreeMap<usize, u32> =
            racks.iter().map(|(&s, r)| (s, r.total())).collect();
        // Betting: chips leave each rack for the felt.
        let mut felt: BTreeMap<usize, ChipStack> = BTreeMap::new();
        for (&seat, &bet) in bets {
            let chips = racks.get_mut(&seat).unwrap().place_bet(bet).unwrap();
            assert_eq!(chips.total(), bet);
            felt.insert(seat, chips);
            table.apply(seat, Action::PlaceBet(bet)).unwrap();
        }
        let first_seat = *bets.keys().next().unwrap();
        let mut transition = table.apply(first_seat, Action::Deal).unwrap();
        let mut events = transition.events.clone();

        // Play every decision the engine asks for, choosing among its own
        // legal actions and moving matching chips onto the felt.
        while transition.snapshot.phase != Phase::RoundOver {
            let active = transition.snapshot.active.unwrap();
            let seat = active.seat;
            let legal = transition.snapshot.legal_actions.clone();
            let kind = legal[next_byte() % legal.len()];
            let action = match kind {
                ActionKind::TakeInsurance => {
                    let stake = bets[&seat] / 2;
                    let chips = racks.get_mut(&seat).unwrap().place_bet(stake).unwrap();
                    felt.get_mut(&seat).unwrap().merge(chips);
                    Action::TakeInsurance
                }
                ActionKind::DeclineInsurance => Action::DeclineInsurance,
                ActionKind::Hit => Action::Hit,
                ActionKind::Stand => Action::Stand,
                ActionKind::Double => {
                    // Doubling matches the hand's current bet with new chips.
                    let hand_bet = transition.snapshot.seats[seat].hands[active.hand].bet;
                    let chips = racks.get_mut(&seat).unwrap().place_bet(hand_bet).unwrap();
                    felt.get_mut(&seat).unwrap().merge(chips);
                    Action::Double
                }
                ActionKind::Split => {
                    // Splitting puts a second bet of the same size down.
                    let hand_bet = transition.snapshot.seats[seat].hands[active.hand].bet;
                    let chips = racks.get_mut(&seat).unwrap().place_bet(hand_bet).unwrap();
                    felt.get_mut(&seat).unwrap().merge(chips);
                    Action::Split
                }
                ActionKind::Surrender => Action::Surrender,
                ActionKind::PlaceBet | ActionKind::Deal | ActionKind::NextRound => {
                    unreachable!("not offered mid-round")
                }
            };
            transition = table.apply(seat, action).unwrap();
            events.extend(transition.events.iter().cloned());
        }

        // Settlement: the tray sweeps each seat's felt chips and returns
        // the stake plus the seat's net.
        let snapshot = transition.snapshot.clone();
        let wagered: BTreeMap<usize, u32> =
            felt.iter().map(|(&s, chips)| (s, chips.total())).collect();
        for (seat, chips) in felt {
            let net = snapshot.seats[seat].round_net.unwrap();
            let returned = tray.settle(chips, net).unwrap();
            racks.get_mut(&seat).unwrap().receive(returned);
        }
        log.rounds.push(RoundLog {
            events,
            snapshot,
            wagered,
            rack_before,
            rack_after: racks.iter().map(|(&s, r)| (s, r.total())).collect(),
            tray_after: tray.total(),
        });
        table.apply(first_seat, Action::NextRound).unwrap();
    }

    log.final_racks = racks
        .into_iter()
        .map(|(seat, rack)| (seat, rack.chips().clone()))
        .collect();
    log.final_tray = tray.chips().clone();
    log
}

/// Strategy: one to three betting seats with legal canonical bets.
fn bets_strategy() -> impl Strategy<Value = BTreeMap<usize, u32>> {
    prop::collection::btree_map(0usize..7, 10u32..=500, 1..=3)
}

/// Strategy: a non-empty action script.
fn script_strategy() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..200)
}

proptest! {
    /// Change making is exact for every whole-dollar amount.
    #[test]
    fn change_total_round_trips(amount in 0u32..=1_000_000) {
        prop_assert_eq!(ChipStack::change(amount).total(), amount);
    }

    /// Coloring up preserves value and never adds chips.
    #[test]
    fn color_up_preserves_total_and_never_adds_chips(
        counts in prop::collection::vec(0u32..=100, 5)
    ) {
        let mut stack = ChipStack::new();
        for (denomination, &count) in Denomination::ALL.iter().zip(&counts) {
            stack.add_chips(*denomination, count);
        }
        let tidy = stack.color_up();
        prop_assert_eq!(tidy.total(), stack.total());
        prop_assert!(tidy.chip_count() <= stack.chip_count());
        // Coloring up twice changes nothing: change is already minimal.
        prop_assert_eq!(tidy.color_up(), tidy);
    }

    /// A withdrawal splits a stack into two stacks that conserve value.
    #[test]
    fn withdraw_conserves_value(
        counts in prop::collection::vec(0u32..=50, 5),
        amount in 0u32..=5_000,
    ) {
        let mut stack = ChipStack::new();
        for (denomination, &count) in Denomination::ALL.iter().zip(&counts) {
            stack.add_chips(*denomination, count);
        }
        let before = stack.total();
        match stack.withdraw(amount) {
            Ok(taken) => {
                prop_assert!(before >= amount);
                prop_assert_eq!(taken.total(), amount);
                prop_assert_eq!(stack.total(), before - amount);
            }
            Err(_) => prop_assert!(before < amount),
        }
    }

    /// Chip conservation: across every settled round, every dollar lost by
    /// a player is gained by the house and vice versa — the total chip
    /// value across all racks and the tray never changes, and each seat's
    /// bankroll delta equals its settled net, which equals the sum of its
    /// hand settlements plus insurance.
    #[test]
    fn chips_are_conserved_across_rounds(
        seed in any::<u64>(),
        bets in bets_strategy(),
        script in script_strategy(),
        rounds in 1usize..=3,
    ) {
        let log = run_script(seed, &bets, &script, rounds);
        let grand_total = TRAY + BANKROLL * bets.len() as u32;
        for round in &log.rounds {
            let racks: u32 = round.rack_after.values().sum();
            prop_assert_eq!(round.tray_after + racks, grand_total);
            for (&seat, &bet) in &bets {
                let net = round.snapshot.seats[seat].round_net.unwrap();
                // Bankroll delta over the round is exactly the settled net.
                let delta =
                    i64::from(round.rack_after[&seat]) - i64::from(round.rack_before[&seat]);
                prop_assert_eq!(delta, i64::from(net));
                // The settled net is the sum of the seat's hand settlements
                // plus its insurance result.
                let event_net: i64 = round
                    .events
                    .iter()
                    .map(|event| match event {
                        Event::HandSettled { seat: s, amount, .. }
                        | Event::InsuranceResolved { seat: s, amount } if *s == seat => {
                            i64::from(*amount)
                        }
                        _ => 0,
                    })
                    .sum();
                prop_assert_eq!(event_net, i64::from(net));
                // Sanity: the felt never held less than the main bet.
                prop_assert!(round.wagered[&seat] >= bet);
            }
        }
        let final_racks: u32 = log.final_racks.values().map(ChipStack::total).sum();
        prop_assert_eq!(log.final_tray.total() + final_racks, grand_total);
    }

    /// Settlement bounds: no hand pays more than the rules allow for its
    /// bet, no hand loses more than its bet, and a seat's round net stays
    /// within what its hands and insurance stake could possibly swing.
    #[test]
    fn settlements_stay_within_rule_bounds(
        seed in any::<u64>(),
        bets in bets_strategy(),
        script in script_strategy(),
        rounds in 1usize..=3,
    ) {
        let log = run_script(seed, &bets, &script, rounds);
        for round in &log.rounds {
            let rules = &round.snapshot.rules;
            for event in &round.events {
                let Event::HandSettled { seat, hand, outcome, amount } = *event else {
                    continue;
                };
                let hand = &round.snapshot.seats[seat].hands[hand];
                let bet = i64::from(hand.bet);
                let amount = i64::from(amount);
                // The most any hand can win is the blackjack payout on its
                // bet; the most it can lose is the bet itself.
                let max_win = i64::from(rules.blackjack_payout.winnings(hand.bet));
                prop_assert!(amount <= max_win);
                prop_assert!(amount >= -bet);
                match outcome {
                    HandOutcome::Blackjack => {
                        prop_assert!(!hand.doubled);
                        prop_assert_eq!(amount, max_win);
                    }
                    HandOutcome::Win => prop_assert_eq!(amount, bet),
                    HandOutcome::Push => prop_assert_eq!(amount, 0),
                    HandOutcome::Lose | HandOutcome::Bust => prop_assert_eq!(amount, -bet),
                    HandOutcome::Surrender => prop_assert_eq!(amount, -(bet - bet / 2)),
                }
            }
            for (&seat, &bet) in &bets {
                let seat_snapshot = &round.snapshot.seats[seat];
                let net = i64::from(seat_snapshot.round_net.unwrap());
                let stake = i64::from(bet / 2); // Maximum insurance stake.
                let max_win: i64 = seat_snapshot
                    .hands
                    .iter()
                    .map(|h| i64::from(rules.blackjack_payout.winnings(h.bet)))
                    .sum::<i64>()
                    + 2 * stake;
                let max_loss: i64 =
                    seat_snapshot.hands.iter().map(|h| i64::from(h.bet)).sum::<i64>() + stake;
                prop_assert!(net <= max_win);
                prop_assert!(net >= -max_loss);
                // A seat with `h` hands never wagers more than `h` doubled
                // bets plus insurance, and every wager left the rack.
                let wagered = i64::from(round.wagered[&seat]);
                prop_assert!(wagered <= max_loss);
            }
        }
    }

    /// Determinism: the same seed, bets, and script produce identical
    /// events, snapshots, and chip flows, down to the exact chips in every
    /// rack.
    #[test]
    fn same_seed_and_script_give_identical_chip_flows(
        seed in any::<u64>(),
        bets in bets_strategy(),
        script in script_strategy(),
        rounds in 1usize..=3,
    ) {
        let first = run_script(seed, &bets, &script, rounds);
        let second = run_script(seed, &bets, &script, rounds);
        prop_assert_eq!(first, second);
    }
}
