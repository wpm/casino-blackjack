//! Table life: AI opponents who come and go, dealers who rotate, and the
//! orchestrator that runs it all around one human seat.
//!
//! [`TableLife`] owns a [`Table`] plus everything that makes it feel
//! inhabited — a roster of hidden AI players with bankrolls and moods,
//! and a dealer with a pace. The frontend drives it with two calls:
//! [`TableLife::advance`] performs the next piece of non-human work (an
//! AI bet, an AI decision, the deal, settlement, arrivals between
//! rounds), and [`TableLife::human_apply`] forwards the human's own
//! action. Both return a [`Step`] whose [`Awaiting`] says whether to keep
//! advancing or wait for human input, and whose [`Pace`] carries the
//! current dealer's suggested delays — this crate never sleeps.
//!
//! The human's bankroll, buy-in, and departure live a layer up (the
//! session shell): here the human seat is just one seat among others,
//! reserved and forwarded to.

use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::action::{Action, ActionError, ActionKind};
use crate::event::Event;
use crate::persona::{self, Archetype};
use crate::rules::Rules;
use crate::snapshot::{Phase, SeatSnapshot, Snapshot};
use crate::table::Table;

/// Most AI opponents ever seated at once.
const MAX_OPPONENTS: usize = 5;
/// Rounds a new arrival always stays before departure rolls begin.
const DEPARTURE_GRACE_ROUNDS: u32 = 8;
/// Per-round departure probability once the grace period has passed.
const DEPARTURE_PROBABILITY: f64 = 0.10;
/// Dealer tenure in rounds, rolled anew for each dealer.
const DEALER_TENURE_ROUNDS: std::ops::RangeInclusive<u32> = 12..=24;

/// Per-round arrival probability, tuned against the departure rate so the
/// steady state is typically two to five opponents.
fn arrival_probability(opponents: usize) -> f64 {
    match opponents {
        0 => 1.0,
        1 => 0.6,
        2 => 0.35,
        3 => 0.25,
        4 => 0.12,
        _ => 0.0,
    }
}

/// Suggested delays, in milliseconds, for animating events — the current
/// dealer's pace. Purely advisory: the engine never sleeps, the UI decides
/// what to do with these numbers. Pace changes when the dealer does and is
/// never user-controllable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pace {
    /// Delay per card dealt.
    pub card_ms: u32,
    /// Pause before each player or dealer decision.
    pub decision_ms: u32,
    /// Pause on the hole-card reveal.
    pub reveal_ms: u32,
    /// Delay per hand settled.
    pub settlement_ms: u32,
    /// Breather between rounds.
    pub between_rounds_ms: u32,
}

/// A dealer's temperament, expressed only through [`Pace`]. Internal:
/// the UI sees numbers, never a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PacePersona {
    Brisk,
    Measured,
    Leisurely,
}

impl PacePersona {
    const ALL: [PacePersona; 3] = [
        PacePersona::Brisk,
        PacePersona::Measured,
        PacePersona::Leisurely,
    ];

    fn pace(self) -> Pace {
        match self {
            PacePersona::Brisk => Pace {
                card_ms: 220,
                decision_ms: 350,
                reveal_ms: 450,
                settlement_ms: 300,
                between_rounds_ms: 1200,
            },
            PacePersona::Measured => Pace {
                card_ms: 400,
                decision_ms: 700,
                reveal_ms: 700,
                settlement_ms: 500,
                between_rounds_ms: 2200,
            },
            PacePersona::Leisurely => Pace {
                card_ms: 650,
                decision_ms: 1200,
                reveal_ms: 1000,
                settlement_ms: 800,
                between_rounds_ms: 3500,
            },
        }
    }
}

/// What [`TableLife`] needs next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Awaiting {
    /// Engine work is pending: call [`TableLife::advance`] again.
    Engine,
    /// The human may bet (via [`TableLife::human_apply`] with
    /// [`Action::PlaceBet`]) or sit the round out (via
    /// [`TableLife::human_sit_out`]).
    HumanBet,
    /// The insurance decision rests with the human seat.
    HumanInsurance,
    /// The play decision rests with the human seat; consult
    /// [`Snapshot::legal_actions`].
    HumanTurn,
}

/// One step of table life: what happened, the state it left behind, the
/// dealer's pace for animating it, and what the orchestrator needs next.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Events this step caused, in order — table events plus life events
    /// (arrivals, departures, dealer changes). Possibly empty.
    pub events: Vec<Event>,
    /// The complete table state after the step.
    pub snapshot: Snapshot,
    /// The current dealer's suggested delays for animating `events`.
    pub pace: Pace,
    /// Whether to keep advancing or wait for human input.
    pub awaiting: Awaiting,
}

/// One AI opponent at the table. Entirely hidden from the UI.
#[derive(Debug, Clone)]
struct AiPlayer {
    archetype: Archetype,
    /// Whole dollars on hand; bets, doubles, splits, and insurance never
    /// exceed it. Felted players leave between rounds.
    bankroll: u32,
    /// The player's habitual bet, chosen at arrival.
    base_bet: u32,
    /// Consecutive winning rounds (positive) or losing rounds (negative).
    streak: i32,
    /// Rounds played since sitting down, for departure rolls.
    rounds_at_table: u32,
}

/// The living table: a [`Table`] plus AI opponents, a rotating dealer,
/// and one reserved human seat.
///
/// All randomness beyond the shoe flows from the injected life RNG, so a
/// seeded [`TableLife`] replays identically: same arrivals, same
/// departures, same bets, same plays. See the module docs for the driving
/// loop.
#[derive(Debug, Clone)]
pub struct TableLife<R: RngCore> {
    table: Table<R>,
    rng: R,
    /// Per-seat AI roster; the human seat's entry is always `None`.
    roster: Vec<Option<AiPlayer>>,
    human_seat: usize,
    /// While set, betting no longer waits on the human seat. Cleared by a
    /// successful human bet.
    human_sitting_out: bool,
    persona: PacePersona,
    rounds_until_dealer_change: u32,
    /// Whether between-round housekeeping (arrivals, departures, dealer
    /// rotation) has run for the current betting phase.
    housekeeping_done: bool,
    rounds_played: u64,
}

impl<R: RngCore> TableLife<R> {
    /// Open a living table. `table_rng` drives the shoe, `life_rng`
    /// everything else (arrivals, archetypes, bets, dealer changes).
    ///
    /// # Panics
    ///
    /// Panics if `human_seat` is not a valid seat under `rules`, or if the
    /// rules themselves are inconsistent (see [`Table::new`]).
    pub fn new(rules: Rules, human_seat: usize, table_rng: R, mut life_rng: R) -> TableLife<R> {
        assert!(
            human_seat < rules.seats,
            "human seat {human_seat} is out of range for a {}-seat table",
            rules.seats
        );
        let table = Table::new(rules, table_rng);
        let seats = table.rules().seats;
        let persona = PacePersona::ALL[life_rng.random_range(0..PacePersona::ALL.len())];
        let rounds_until_dealer_change = life_rng.random_range(DEALER_TENURE_ROUNDS);
        TableLife {
            table,
            rng: life_rng,
            roster: vec![None; seats],
            human_seat,
            human_sitting_out: false,
            persona,
            rounds_until_dealer_change,
            housekeeping_done: false,
            rounds_played: 0,
        }
    }

    /// The frozen rules the table plays by.
    pub fn rules(&self) -> &Rules {
        self.table.rules()
    }

    /// The seat reserved for the human player.
    pub fn human_seat(&self) -> usize {
        self.human_seat
    }

    /// The seats currently occupied by AI opponents, in seat order.
    pub fn opponent_seats(&self) -> Vec<usize> {
        self.roster
            .iter()
            .enumerate()
            .filter_map(|(seat, player)| player.as_ref().map(|_| seat))
            .collect()
    }

    /// Completed rounds since the table opened.
    pub fn rounds_played(&self) -> u64 {
        self.rounds_played
    }

    /// The current dealer's pace.
    pub fn pace(&self) -> Pace {
        self.persona.pace()
    }

    /// The complete visible state of the table right now.
    pub fn snapshot(&self) -> Snapshot {
        self.table.snapshot()
    }

    /// What the orchestrator needs next, without doing any work.
    pub fn awaiting(&self) -> Awaiting {
        match self.table.phase() {
            Phase::Betting => {
                if !self.housekeeping_done {
                    return Awaiting::Engine;
                }
                let snapshot = self.table.snapshot();
                if self
                    .opponent_seats()
                    .iter()
                    .any(|&seat| snapshot.seats[seat].bet.is_none())
                {
                    return Awaiting::Engine;
                }
                let human_bet = snapshot.seats[self.human_seat].bet.is_some();
                if !self.human_sitting_out && !human_bet {
                    return Awaiting::HumanBet;
                }
                if snapshot.seats.iter().any(|seat| seat.bet.is_some()) {
                    Awaiting::Engine
                } else {
                    // Nobody at all is betting (possible only on a table
                    // with no AI seats): only the human can start a round.
                    Awaiting::HumanBet
                }
            }
            Phase::InsuranceOffer => {
                let active = self
                    .table
                    .snapshot()
                    .active
                    .expect("insurance offer has an active seat");
                if active.seat == self.human_seat {
                    Awaiting::HumanInsurance
                } else {
                    Awaiting::Engine
                }
            }
            Phase::PlayerTurn => {
                let active = self
                    .table
                    .snapshot()
                    .active
                    .expect("player turn has an active seat");
                if active.seat == self.human_seat {
                    Awaiting::HumanTurn
                } else {
                    Awaiting::Engine
                }
            }
            Phase::RoundOver => Awaiting::Engine,
        }
    }

    /// Perform the next piece of non-human work: between rounds, one
    /// housekeeping pass or one AI bet or the deal; during play, one AI
    /// decision; after settlement, the bankroll bookkeeping and the reset
    /// for the next round.
    ///
    /// Each call does at most one beat of work so the UI can animate
    /// between calls using [`Step::pace`]. When the returned
    /// [`Step::awaiting`] is [`Awaiting::Engine`], call again; otherwise
    /// wait for the human. Calling while waiting on the human is harmless
    /// and returns an empty step.
    pub fn advance(&mut self) -> Step {
        let mut events = Vec::new();
        match self.table.phase() {
            Phase::Betting => self.step_betting(&mut events),
            Phase::InsuranceOffer => self.step_insurance(&mut events),
            Phase::PlayerTurn => self.step_player_turn(&mut events),
            Phase::RoundOver => self.step_round_over(&mut events),
        }
        self.step(events)
    }

    /// Forward `action` from the human seat to the table.
    ///
    /// On success the table advances exactly as [`Table::apply`] would; on
    /// failure the typed error is returned and nothing changes. A
    /// successful bet clears any [`TableLife::human_sit_out`]. The
    /// table-level actions ([`Action::Deal`], [`Action::NextRound`]) are
    /// the orchestrator's job and are redirected through
    /// [`TableLife::advance`].
    pub fn human_apply(&mut self, action: Action) -> Result<Step, ActionError> {
        if matches!(action, Action::Deal | Action::NextRound) {
            return Ok(self.advance());
        }
        let transition = self.table.apply(self.human_seat, action)?;
        if matches!(action, Action::PlaceBet(_)) {
            self.human_sitting_out = false;
        }
        Ok(Step {
            events: transition.events,
            snapshot: transition.snapshot,
            pace: self.persona.pace(),
            awaiting: self.awaiting(),
        })
    }

    /// Mark the human seat as sitting out, so betting no longer waits on
    /// it. Sticky across rounds until the human places a bet again — the
    /// session layer uses this while the human is away (or broke) and the
    /// table plays on around the empty seat.
    pub fn human_sit_out(&mut self) {
        self.human_sitting_out = true;
    }

    /// Whether the human seat is currently sitting out.
    pub fn human_sitting_out(&self) -> bool {
        self.human_sitting_out
    }

    fn step(&self, events: Vec<Event>) -> Step {
        Step {
            events,
            snapshot: self.table.snapshot(),
            pace: self.persona.pace(),
            awaiting: self.awaiting(),
        }
    }

    // ------------------------------------------------------------------
    // Per-phase engine beats.
    // ------------------------------------------------------------------

    fn step_betting(&mut self, events: &mut Vec<Event>) {
        if !self.housekeeping_done {
            self.housekeeping(events);
            self.housekeeping_done = true;
            return;
        }
        let snapshot = self.table.snapshot();
        // One AI bet per beat.
        for seat in self.opponent_seats() {
            if snapshot.seats[seat].bet.is_none() {
                let rules = self.table.rules();
                let (min, max) = (rules.min_bet, rules.max_bet);
                let player = self.roster[seat].as_ref().expect("seat is occupied");
                let amount = persona::bet_size(
                    player.archetype,
                    player.base_bet,
                    player.streak,
                    player.bankroll,
                    min,
                    max,
                );
                let transition = self
                    .table
                    .apply(seat, Action::PlaceBet(amount))
                    .expect("AI bets are clamped to the table limits");
                events.extend(transition.events);
                return;
            }
        }
        // All AI bets are in. Wait for the human if they are still due.
        if !self.human_sitting_out && snapshot.seats[self.human_seat].bet.is_none() {
            return;
        }
        if snapshot.seats.iter().any(|seat| seat.bet.is_some()) {
            let transition = self
                .table
                .apply(0, Action::Deal)
                .expect("a bet is on the felt");
            events.extend(transition.events);
        }
    }

    fn step_insurance(&mut self, events: &mut Vec<Event>) {
        let snapshot = self.table.snapshot();
        let seat = snapshot
            .active
            .expect("insurance offer has an active seat")
            .seat;
        if seat == self.human_seat {
            return;
        }
        let player = self.roster[seat]
            .as_ref()
            .expect("active AI seat is occupied");
        let stake = snapshot.seats[seat].bet.expect("insured seat has a bet") / 2;
        let affordable = committed(&snapshot.seats[seat]) + stake <= player.bankroll;
        let take = affordable && persona::decide_insurance(player.archetype, &mut self.rng);
        let action = if take {
            Action::TakeInsurance
        } else {
            Action::DeclineInsurance
        };
        let transition = self
            .table
            .apply(seat, action)
            .expect("insurance decisions are always legal for the active seat");
        events.extend(transition.events);
    }

    fn step_player_turn(&mut self, events: &mut Vec<Event>) {
        let snapshot = self.table.snapshot();
        let active = snapshot.active.expect("player turn has an active seat");
        if active.seat == self.human_seat {
            return;
        }
        let player = self.roster[active.seat]
            .as_ref()
            .expect("active AI seat is occupied");
        let seat_snapshot = &snapshot.seats[active.seat];
        let hand = &seat_snapshot.hands[active.hand];
        let upcard = snapshot.dealer.upcard.expect("upcard is dealt during play");
        // Doubles and splits each cost another hand bet; drop them from
        // the legal set when the player cannot cover it.
        let headroom = player.bankroll.saturating_sub(committed(seat_snapshot));
        let legal: Vec<ActionKind> = snapshot
            .legal_actions
            .iter()
            .copied()
            .filter(|kind| match kind {
                ActionKind::Double | ActionKind::Split => hand.bet <= headroom,
                _ => true,
            })
            .collect();
        let action = persona::decide(player.archetype, hand, upcard, &legal, &mut self.rng);
        let transition = self
            .table
            .apply(active.seat, action)
            .expect("AI decisions are drawn from the legal actions");
        events.extend(transition.events);
    }

    fn step_round_over(&mut self, events: &mut Vec<Event>) {
        let snapshot = self.table.snapshot();
        for seat in self.opponent_seats() {
            let player = self.roster[seat].as_mut().expect("seat is occupied");
            if let Some(net) = snapshot.seats[seat].round_net {
                player.bankroll = (i64::from(player.bankroll) + i64::from(net)).max(0) as u32;
                if net > 0 {
                    player.streak = player.streak.max(0) + 1;
                } else if net < 0 {
                    player.streak = player.streak.min(0) - 1;
                }
            }
            player.rounds_at_table += 1;
        }
        let transition = self
            .table
            .apply(0, Action::NextRound)
            .expect("NextRound is legal when the round is over");
        events.extend(transition.events);
        self.rounds_played += 1;
        self.rounds_until_dealer_change = self.rounds_until_dealer_change.saturating_sub(1);
        self.housekeeping_done = false;
    }

    // ------------------------------------------------------------------
    // Between-round life: dealer rotation, departures, arrivals.
    // ------------------------------------------------------------------

    fn housekeeping(&mut self, events: &mut Vec<Event>) {
        // Dealer rotation.
        if self.rounds_until_dealer_change == 0 {
            self.persona = PacePersona::ALL[self.rng.random_range(0..PacePersona::ALL.len())];
            self.rounds_until_dealer_change = self.rng.random_range(DEALER_TENURE_ROUNDS);
            events.push(Event::DealerChanged);
        }
        // Departures: felted players must leave; the rest drift away once
        // they have been here a while.
        let min_bet = self.table.rules().min_bet;
        for seat in self.opponent_seats() {
            let player = self.roster[seat].as_ref().expect("seat is occupied");
            let felted = player.bankroll < min_bet;
            let wandering = player.rounds_at_table >= DEPARTURE_GRACE_ROUNDS
                && self.rng.random_bool(DEPARTURE_PROBABILITY);
            if felted || wandering {
                self.roster[seat] = None;
                events.push(Event::PlayerDeparted { seat });
            }
        }
        // Arrivals. The very first round seats an opening crowd; after
        // that, walk-ups arrive at a rate tuned against departures.
        if self.rounds_played == 0 {
            let target = self.rng.random_range(2..=4);
            while self.opponent_seats().len() < target && self.arrive(events) {}
        } else {
            let opponents = self.opponent_seats().len();
            if opponents < MAX_OPPONENTS && self.rng.random_bool(arrival_probability(opponents)) {
                self.arrive(events);
            }
        }
    }

    /// Seat a new AI player at a random empty non-human seat. Returns
    /// false when no seat is free (or the table is at the opponent cap).
    fn arrive(&mut self, events: &mut Vec<Event>) -> bool {
        if self.opponent_seats().len() >= MAX_OPPONENTS {
            return false;
        }
        let empty: Vec<usize> = (0..self.roster.len())
            .filter(|&seat| seat != self.human_seat && self.roster[seat].is_none())
            .collect();
        if empty.is_empty() {
            return false;
        }
        let seat = empty[self.rng.random_range(0..empty.len())];
        let archetype = Archetype::ALL[self.rng.random_range(0..Archetype::ALL.len())];
        let min_bet = self.table.rules().min_bet;
        let bankroll = min_bet * self.rng.random_range(30..=100);
        let base_bet = min_bet * self.rng.random_range(1..=3);
        self.roster[seat] = Some(AiPlayer {
            archetype,
            bankroll,
            base_bet,
            streak: 0,
            rounds_at_table: 0,
        });
        events.push(Event::PlayerArrived { seat });
        true
    }
}

impl TableLife<ChaCha8Rng> {
    /// Open a living table seeded from `seed`, for deterministic tests and
    /// replays: the same rules, human seat, and seed always produce the
    /// same shoe, roster evolution, and AI play.
    pub fn from_seed(rules: Rules, human_seat: usize, seed: u64) -> TableLife<ChaCha8Rng> {
        // Two independent streams derived from one seed: one for the
        // shoe, one for table life.
        TableLife::new(
            rules,
            human_seat,
            ChaCha8Rng::seed_from_u64(seed),
            ChaCha8Rng::seed_from_u64(seed ^ 0x9E37_79B9_7F4A_7C15),
        )
    }
}

/// Dollars a seat has committed this round: every hand bet plus any
/// insurance stake.
fn committed(seat: &SeatSnapshot) -> u32 {
    let hands: u32 = seat.hands.iter().map(|hand| hand.bet).sum();
    let insurance = match seat.insurance {
        crate::snapshot::Insurance::Taken { amount } => amount,
        _ => 0,
    };
    hands + insurance
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUMAN: usize = 3;

    fn life(seed: u64) -> TableLife<ChaCha8Rng> {
        TableLife::from_seed(Rules::canonical(), HUMAN, seed)
    }

    /// Drive `rounds` complete rounds with the human sitting out,
    /// returning every event in order.
    fn run_rounds(life: &mut TableLife<ChaCha8Rng>, rounds: u64) -> Vec<Event> {
        life.human_sit_out();
        let target = life.rounds_played() + rounds;
        let mut events = Vec::new();
        let mut beats = 0u64;
        while life.rounds_played() < target {
            beats += 1;
            assert!(beats < 1_000_000, "simulation stopped making progress");
            let step = life.advance();
            assert_eq!(
                step.awaiting,
                Awaiting::Engine,
                "a sitting-out human must never be waited on"
            );
            assert!(
                step.snapshot.seats[HUMAN].bet.is_none(),
                "the human seat must never bet on its own"
            );
            events.extend(step.events);
        }
        events
    }

    #[test]
    fn population_stays_in_bounds_with_arrivals_and_departures() {
        for seed in [1, 2, 42] {
            let mut life = life(seed);
            life.human_sit_out();
            let mut arrivals = 0usize;
            let mut departures = 0usize;
            let mut per_round_counts: Vec<usize> = Vec::new();
            let mut beats = 0u64;
            while life.rounds_played() < 400 {
                beats += 1;
                assert!(beats < 1_000_000, "simulation stopped making progress");
                let before = life.rounds_played();
                let step = life.advance();
                for event in &step.events {
                    match event {
                        Event::PlayerArrived { .. } => arrivals += 1,
                        Event::PlayerDeparted { .. } => departures += 1,
                        _ => {}
                    }
                }
                let opponents = life.opponent_seats().len();
                assert!(
                    opponents <= MAX_OPPONENTS,
                    "seed {seed}: too many opponents"
                );
                assert!(
                    life.opponent_seats().iter().all(|&s| s != HUMAN),
                    "an AI player took the human seat"
                );
                if life.rounds_played() > before {
                    per_round_counts.push(opponents);
                }
            }
            assert!(arrivals > 5, "seed {seed}: arrivals never happened");
            assert!(departures > 5, "seed {seed}: departures never happened");
            // Steady state: typically two to five opponents. Skip the
            // opening rounds while the crowd builds.
            let settled = &per_round_counts[50..];
            let mean = settled.iter().sum::<usize>() as f64 / settled.len() as f64;
            assert!(
                (2.0..=5.0).contains(&mean),
                "seed {seed}: mean opponents {mean} outside the typical band"
            );
            assert!(
                settled.iter().all(|&n| n >= 1),
                "seed {seed}: the table went completely empty at steady state"
            );
        }
    }

    #[test]
    fn same_seed_replays_identically() {
        let mut a = life(9);
        let mut b = life(9);
        let events_a = run_rounds(&mut a, 150);
        let events_b = run_rounds(&mut b, 150);
        assert_eq!(events_a, events_b);
        assert_eq!(a.opponent_seats(), b.opponent_seats());
        assert_eq!(a.snapshot(), b.snapshot());
        assert_eq!(a.pace(), b.pace());
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = life(10);
        let mut b = life(11);
        let events_a = run_rounds(&mut a, 50);
        let events_b = run_rounds(&mut b, 50);
        assert_ne!(events_a, events_b);
    }

    #[test]
    fn dealers_rotate_every_so_often() {
        let mut l = life(5);
        let events = run_rounds(&mut l, 300);
        let changes = events
            .iter()
            .filter(|e| matches!(e, Event::DealerChanged))
            .count();
        // Tenure is 12-24 rounds, so 300 rounds sees roughly 12-25 changes.
        assert!(
            (5..=40).contains(&changes),
            "expected periodic dealer changes, saw {changes}"
        );
        let pace = l.pace();
        assert!(pace.card_ms > 0 && pace.between_rounds_ms > 0);
        assert!(
            PacePersona::ALL.iter().any(|p| p.pace() == pace),
            "pace must always be one of the dealer personas"
        );
    }

    #[test]
    fn ai_bets_respect_the_table_limits() {
        let mut l = life(21);
        let events = run_rounds(&mut l, 100);
        let rules = Rules::canonical();
        let mut bets = 0;
        for event in events {
            if let Event::BetPlaced { seat, amount } = event {
                assert_ne!(seat, HUMAN);
                assert!(amount >= rules.min_bet, "AI bet {amount} below minimum");
                assert!(amount <= rules.max_bet, "AI bet {amount} above maximum");
                bets += 1;
            }
        }
        assert!(bets > 100, "expected plenty of AI bets");
    }

    #[test]
    fn felted_players_leave_at_the_next_break() {
        let mut l = life(33);
        run_rounds(&mut l, 3);
        // Break a player by hand; the next housekeeping pass must show
        // them out.
        let seat = l.opponent_seats()[0];
        l.roster[seat].as_mut().unwrap().bankroll = 0;
        assert!(
            !l.housekeeping_done,
            "a fresh round starts with housekeeping"
        );
        let step = l.advance();
        assert!(
            step.events.contains(&Event::PlayerDeparted { seat }),
            "felted player did not leave: {:?}",
            step.events
        );
        assert!(!l.opponent_seats().contains(&seat));
    }

    #[test]
    fn the_human_seat_is_driven_from_outside() {
        let mut l = life(77);
        let mut human_rounds = 0u64;
        let mut beats = 0u64;
        while l.rounds_played() < 20 {
            beats += 1;
            assert!(beats < 1_000_000, "simulation stopped making progress");
            let awaiting = l.awaiting();
            match awaiting {
                Awaiting::Engine => {
                    l.advance();
                }
                Awaiting::HumanBet => {
                    let step = l.human_apply(Action::PlaceBet(10)).unwrap();
                    assert!(step.events.contains(&Event::BetPlaced {
                        seat: HUMAN,
                        amount: 10
                    }));
                    human_rounds += 1;
                }
                Awaiting::HumanInsurance => {
                    l.human_apply(Action::DeclineInsurance).unwrap();
                }
                Awaiting::HumanTurn => {
                    let snapshot = l.snapshot();
                    assert_eq!(snapshot.active.unwrap().seat, HUMAN);
                    l.human_apply(Action::Stand).unwrap();
                }
            }
        }
        assert_eq!(
            human_rounds, 20,
            "the human should be asked to bet each round"
        );
    }

    #[test]
    fn human_actions_are_validated_by_the_table() {
        let mut l = life(78);
        // Betting has not even opened for the human yet (housekeeping
        // pending), but bet validation is the table's as usual.
        while l.awaiting() == Awaiting::Engine {
            l.advance();
        }
        assert_eq!(l.awaiting(), Awaiting::HumanBet);
        let err = l.human_apply(Action::PlaceBet(5)).unwrap_err();
        assert_eq!(
            err,
            ActionError::BetOutOfRange {
                bet: 5,
                min: 10,
                max: 500
            }
        );
        // Hitting during betting is refused and changes nothing.
        assert!(matches!(
            l.human_apply(Action::Hit),
            Err(ActionError::WrongPhase { .. })
        ));
        assert_eq!(l.awaiting(), Awaiting::HumanBet);
    }

    #[test]
    fn sitting_out_is_sticky_until_the_next_bet() {
        let mut l = life(80);
        run_rounds(&mut l, 5);
        assert!(l.human_sitting_out());
        // A completed round leaves the table back in betting, so rejoining
        // is one bet away.
        assert_eq!(l.snapshot().phase, Phase::Betting);
        l.human_apply(Action::PlaceBet(25)).unwrap();
        assert!(!l.human_sitting_out());
    }

    #[test]
    fn table_level_actions_route_through_advance() {
        let mut l = life(81);
        // Deal/NextRound from the human are redirected to the
        // orchestrator's own sequencing rather than hitting the table raw.
        let step = l.human_apply(Action::Deal).unwrap();
        // The first beat is housekeeping: the opening crowd arrives.
        assert!(
            step.events
                .iter()
                .any(|e| matches!(e, Event::PlayerArrived { .. }))
        );
    }

    #[test]
    fn step_serde_round_trips() {
        let mut l = life(90);
        let step = l.advance();
        let json = serde_json::to_string(&step).unwrap();
        let back: Step = serde_json::from_str(&json).unwrap();
        assert_eq!(step, back);
    }

    #[test]
    #[should_panic(expected = "human seat")]
    fn out_of_range_human_seat_panics() {
        let _ = TableLife::from_seed(Rules::canonical(), 7, 0);
    }
}
