//! The session arc: buy-in, table life, walk away, cash out, game over —
//! the complete life of a session, created anew each launch.
//!
//! [`SessionArc`] wraps a [`TableLife`] with the one thing the engine
//! deliberately leaves a layer up: the human's money as physical chips.
//! The [`Rack`]'s chips are the wire truth — a bet withdraws real chips
//! from the rack onto the felt, doubles and splits and insurance commit
//! more, and settlement pushes chips back with payout semantics
//! ([`payout_chips`]). Per-round conservation holds by construction: the
//! rack's delta over a round equals the seat's `round_net`.
//!
//! The arc is platform-free so both backends run it verbatim: the Tauri
//! shell holds one behind a mutex, and the browser dev fallback holds
//! one in a thread-local. Nothing here is ever persisted — a session
//! lives and dies with its process.

use std::ops::RangeInclusive;

use blackjack_core::{
    Action, Awaiting, ChipStack, Denomination, Insurance, Pace, Phase, Rack, Rules, Snapshot,
    TableLife, Transition, payout_chips,
};
use rand::{Rng, RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::BackendError;

/// Buy-in floor in dollars.
const BUY_IN_MIN: u32 = 220;
/// Number of $5 steps above the floor (so the ceiling is $380).
const BUY_IN_STEPS: u32 = 32;
/// AI-only rounds played before the human's first view, so the shoe is
/// burned in and the table is mid-life on arrival.
const WARMUP_ROUNDS: RangeInclusive<u32> = 2..=6;
/// Stream-splitting constant for deriving the session RNG from the seed
/// (companion to the one `TableLife::from_seed` uses for table life).
const SESSION_STREAM: u64 = 0xD1B5_4A32_D192_ED03;

/// Where a session stands in its life. Terminal states are terminal:
/// there is no way back to `Playing` but a new process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// The human holds a seat (or is about to bet) at a living table.
    Playing,
    /// The human walked away; the rack colored up to this many dollars.
    /// The only dollar figure in the game outside the placard.
    CashedOut {
        /// The cash value carried away from the table.
        dollars: u32,
    },
    /// A settlement left the rack below the table minimum between
    /// rounds: no bet is possible, the seat is vacated, the game is
    /// over. No dollar figure — busted is busted.
    GameOver,
}

/// The wire reply to every session call: what happened, everything the
/// UI needs to render and gate, and where the session stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    /// The events this call caused (possibly empty) and the complete
    /// table state after it.
    pub transition: Transition,
    /// The human's rack — real chips, depleted by bets, refilled by
    /// settlements. The UI's staging is limited to exactly these chips.
    pub rack: ChipStack,
    /// Whether to keep pumping [`advance`](SessionArc::advance) or rest
    /// for human input.
    pub awaiting: Awaiting,
    /// The current dealer's suggested animation delays.
    pub pace: Pace,
    /// Where the session stands. Anything but
    /// [`SessionStatus::Playing`] stops the drive loop.
    pub status: SessionStatus,
}

/// A randomized buy-in: the chips the human sits down with.
///
/// The amount is uniform over $220–$380 in $5 steps — around $300 at the
/// canonical $10 table, roughly thirty minimum bets, but never the same
/// stack twice. The cut varies too, because nobody's pocket is clean
/// change: one in four buy-ins is the cage's tidy minimal cut, and the
/// rest lean red ($5), lean green ($25), or carry a black $100 chip with
/// workable change around it. No readouts anywhere — eyeballing this
/// stack is the first act of play.
pub fn buy_in<R: RngCore>(rng: &mut R) -> ChipStack {
    let amount = BUY_IN_MIN + 5 * rng.random_range(0..=BUY_IN_STEPS);
    match rng.random_range(0..4u32) {
        // The cage's minimal cut.
        0 => ChipStack::change(amount),
        // Red-heavy: about half the value in $5 chips.
        1 => {
            let fives = amount / 2 / 5;
            let mut stack = ChipStack::change(amount - fives * 5);
            stack.add_chips(Denomination::Five, fives);
            stack
        }
        // Green-led: about half the value in $25 chips, change around it.
        2 => {
            let greens = amount / 2 / 25;
            let mut stack = ChipStack::change(amount - greens * 25);
            stack.add_chips(Denomination::TwentyFive, greens);
            stack
        }
        // One black $100 chip, the rest cut with plenty of red to play.
        _ => {
            let mut stack = ChipStack::new();
            stack.add_chips(Denomination::Hundred, 1);
            let rest = amount - 100;
            let fives = rest / 2 / 5;
            stack.merge(ChipStack::change(rest - fives * 5));
            stack.add_chips(Denomination::Five, fives);
            stack
        }
    }
}

/// One live session: a living table, the human's rack of real chips,
/// and the session's place in its arc. See the module docs.
#[derive(Debug, Clone)]
pub struct SessionArc {
    life: TableLife<ChaCha8Rng>,
    /// The human's bankroll as physical chips.
    rack: Rack,
    /// The human's chips currently committed to the felt this round:
    /// the main bet plus any double, split, or insurance chips. Empty
    /// between rounds — which is exactly when leaving is possible.
    felt: ChipStack,
    status: SessionStatus,
}

impl SessionArc {
    /// Open a session seeded from `seed`: canonical rules, the human at
    /// the center seat, a randomized buy-in, and a randomized number of
    /// AI-only warm-up rounds already played. Fully deterministic per
    /// seed.
    pub fn from_seed(seed: u64) -> SessionArc {
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ SESSION_STREAM);
        let chips = buy_in(&mut rng);
        let warmup = rng.random_range(WARMUP_ROUNDS);
        SessionArc::with_buy_in(seed, chips, warmup)
    }

    /// [`SessionArc::from_seed`] with the buy-in and warm-up length
    /// pinned, for tests that need a poor player or a known table depth.
    pub fn with_buy_in(seed: u64, chips: ChipStack, warmup_rounds: u32) -> SessionArc {
        let rules = Rules::canonical();
        let human_seat = rules.seats / 2;
        let mut life = TableLife::from_seed(rules, human_seat, seed);
        // Arrive mid-life: the table has been playing without us.
        life.human_sit_out();
        let mut beats = 0u32;
        while life.rounds_played() < u64::from(warmup_rounds) {
            beats += 1;
            assert!(beats < 1_000_000, "warm-up stopped making progress");
            life.advance();
        }
        life.human_sit_in();
        SessionArc {
            life,
            rack: Rack::from_chips(chips),
            felt: ChipStack::new(),
            status: SessionStatus::Playing,
        }
    }

    /// Completed rounds since the table opened (warm-up included).
    pub fn rounds_played(&self) -> u64 {
        self.life.rounds_played()
    }

    /// The current state, changing nothing (events empty).
    pub fn view(&self) -> SessionView {
        self.make_view(Vec::new(), self.life.snapshot())
    }

    /// One engine beat. A no-op returning the current view once the
    /// session is over (the table's life goes on, but not on this
    /// screen) or while the engine waits on the human.
    pub fn advance(&mut self) -> SessionView {
        if self.status != SessionStatus::Playing {
            return self.view();
        }
        let step = self.life.advance();
        self.settle_if_due(&step.snapshot);
        self.refresh_status(&step.snapshot);
        self.make_view(step.events, step.snapshot)
    }

    /// Submit one action for the human seat, re-gating legality
    /// server-side: the engine validates the action as always, and any
    /// chips the action commits must actually be in the rack. On
    /// success the committed chips physically move rack → felt (or back,
    /// if a bet was replaced smaller).
    pub fn human_action(&mut self, action: Action) -> Result<SessionView, BackendError> {
        if self.status != SessionStatus::Playing {
            return Err(BackendError::SessionOver);
        }
        let seat = self.life.human_seat();
        let before = self.life.snapshot();
        let cost = chip_cost(&before, seat, action);
        if cost > self.rack.total() {
            return Err(BackendError::InsufficientChips {
                requested: cost,
                available: self.rack.total(),
            });
        }
        let step = self
            .life
            .human_apply(action)
            .map_err(BackendError::Rejected)?;
        let delta = i64::from(stake(&step.snapshot, seat)) - i64::from(stake(&before, seat));
        if delta > 0 {
            let chips = self
                .rack
                .place_bet(delta as u32)
                .expect("the chip cost was pre-gated against the rack");
            self.felt.merge(chips);
        } else if delta < 0 {
            let returned = self
                .felt
                .withdraw((-delta) as u32)
                .expect("the felt always covers its own stake");
            self.rack.receive(returned);
        }
        self.settle_if_due(&step.snapshot);
        self.refresh_status(&step.snapshot);
        Ok(self.make_view(step.events, step.snapshot))
    }

    /// Leave the table. Valid only between rounds with nothing on the
    /// felt — a live hand (or even a posted bet) is never abandoned.
    /// The seat empties like any departing stranger's, the rack colors
    /// up, and the session ends at its cash value.
    pub fn walk_away(&mut self) -> Result<SessionView, BackendError> {
        if self.status != SessionStatus::Playing {
            return Err(BackendError::SessionOver);
        }
        let between = matches!(
            self.life.snapshot().phase,
            Phase::Betting | Phase::RoundOver
        );
        if !between || !self.felt.is_empty() {
            return Err(BackendError::NotBetweenRounds);
        }
        self.life.human_sit_out();
        self.rack.color_up();
        self.status = SessionStatus::CashedOut {
            dollars: self.rack.total(),
        };
        Ok(self.view())
    }

    fn make_view(&self, events: Vec<blackjack_core::Event>, snapshot: Snapshot) -> SessionView {
        SessionView {
            transition: Transition { snapshot, events },
            rack: self.rack.chips().clone(),
            awaiting: self.life.awaiting(),
            pace: self.life.pace(),
            status: self.status,
        }
    }

    /// Once the round settles, push the human's chips back across the
    /// felt: on a win the bet comes back mirrored plus the winnings cut
    /// beside it ([`payout_chips`]); on a loss the tray keeps what it
    /// keeps and returns the remainder as change. The rack's delta over
    /// the round is exactly the seat's `round_net`.
    fn settle_if_due(&mut self, snapshot: &Snapshot) {
        if self.felt.is_empty() || snapshot.phase != Phase::RoundOver {
            return;
        }
        let seat = self.life.human_seat();
        let Some(net) = snapshot.seats[seat].round_net else {
            return;
        };
        let felt = std::mem::take(&mut self.felt);
        let returned = i64::from(felt.total()) + i64::from(net);
        debug_assert!(returned >= 0, "a round never loses more than its stake");
        if net >= 0 {
            let winnings = payout_chips(&felt, net as u32);
            self.rack.receive(felt);
            self.rack.receive(winnings);
        } else {
            self.rack.receive(ChipStack::change(returned as u32));
        }
    }

    /// Between rounds, a rack below the table minimum means no bet is
    /// possible: the seat is vacated exactly like any departing
    /// stranger's and the session is over. Never evaluated mid-hand — a
    /// live hand always plays out first (`felt` is non-empty until it
    /// settles).
    fn refresh_status(&mut self, snapshot: &Snapshot) {
        if self.status != SessionStatus::Playing || !self.felt.is_empty() {
            return;
        }
        let between = matches!(snapshot.phase, Phase::Betting | Phase::RoundOver);
        if between && self.rack.total() < self.life.rules().min_bet {
            self.life.human_sit_out();
            self.status = SessionStatus::GameOver;
        }
    }
}

/// Dollars a seat currently has riding: the posted bet while betting,
/// then every hand bet plus any insurance stake once cards are out.
fn stake(snapshot: &Snapshot, seat: usize) -> u32 {
    let seat = &snapshot.seats[seat];
    if seat.hands.is_empty() {
        return seat.bet.unwrap_or(0);
    }
    let hands: u32 = seat.hands.iter().map(|hand| hand.bet).sum();
    let insurance = match seat.insurance {
        Insurance::Taken { amount } => amount,
        _ => 0,
    };
    hands + insurance
}

/// The additional dollars `action` would commit to the felt, for
/// pre-gating against the rack. Zero for actions that move no chips
/// (and for actions the engine will reject anyway).
fn chip_cost(snapshot: &Snapshot, seat: usize, action: Action) -> u32 {
    match action {
        Action::PlaceBet(amount) => amount.saturating_sub(snapshot.seats[seat].bet.unwrap_or(0)),
        Action::Double | Action::Split => snapshot
            .active
            .filter(|active| active.seat == seat)
            .and_then(|active| snapshot.seats[seat].hands.get(active.hand))
            .map_or(0, |hand| hand.bet),
        Action::TakeInsurance => snapshot.seats[seat].bet.unwrap_or(0) / 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_ins_stay_in_bounds_with_varied_cuts() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let mut totals = std::collections::HashSet::new();
        let mut saw_black = false;
        let mut saw_untidy = false;
        for _ in 0..200 {
            let stack = buy_in(&mut rng);
            let total = stack.total();
            assert!((220..=380).contains(&total), "buy-in ${total} out of range");
            assert_eq!(total % 5, 0, "buy-in ${total} is not a $5 step");
            assert_eq!(
                stack.count(Denomination::One),
                0,
                "no white chips at buy-in"
            );
            totals.insert(total);
            saw_black |= stack.count(Denomination::Hundred) > 0;
            saw_untidy |= stack != ChipStack::change(total);
        }
        assert!(totals.len() > 10, "buy-in amounts barely vary");
        assert!(saw_black, "no buy-in ever carried a black chip");
        assert!(saw_untidy, "every buy-in was the cage's minimal cut");
    }

    #[test]
    fn sessions_arrive_mid_life_and_replay_per_seed() {
        for seed in [0, 1, 42] {
            let a = SessionArc::from_seed(seed);
            let b = SessionArc::from_seed(seed);
            assert!(
                (2..=6).contains(&a.rounds_played()),
                "seed {seed}: warm-up played {} rounds",
                a.rounds_played()
            );
            assert_eq!(a.view(), b.view(), "seed {seed}: views diverged");
            let snapshot = &a.view().transition.snapshot;
            assert!(
                snapshot.shoe.cards_dealt > 0,
                "seed {seed}: the shoe was not burned in"
            );
            assert_eq!(a.view().status, SessionStatus::Playing);
        }
        assert_ne!(
            SessionArc::from_seed(3).view(),
            SessionArc::from_seed(4).view(),
            "different seeds produced identical sessions"
        );
    }

    #[test]
    fn session_view_serde_round_trips() {
        let arc = SessionArc::from_seed(11);
        let view = arc.view();
        let json = serde_json::to_string(&view).unwrap();
        let back: SessionView = serde_json::from_str(&json).unwrap();
        assert_eq!(view, back);
        for status in [
            SessionStatus::Playing,
            SessionStatus::CashedOut { dollars: 435 },
            SessionStatus::GameOver,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(
                serde_json::from_str::<SessionStatus>(&json).unwrap(),
                status
            );
        }
    }
}
