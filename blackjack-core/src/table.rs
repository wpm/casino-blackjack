//! The table: a multi-seat blackjack round state machine that owns the
//! shoe and adjudicates every rule.
//!
//! A [`Table`] runs round after round against one shoe under one frozen
//! [`Rules`]. Every state change goes through [`Table::apply`], which
//! either rejects the action with a typed [`ActionError`] — mutating
//! nothing — or performs it and returns a [`Transition`]: the full new
//! [`Snapshot`] plus the ordered [`Event`]s that occurred.
//!
//! A round moves betting → dealing → insurance offer (dealer ace up) →
//! dealer peek (ace or ten up; American hole-card game) → player turns
//! (seat 0 first, hands left to right) → dealer turn → settlement. Only
//! the phases needing a decision rest ([`Phase`]); the rest happen
//! atomically inside `apply` and are reported as events.

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::action::{Action, ActionError, ActionKind};
use crate::card::Card;
use crate::event::{Event, HandOutcome};
use crate::hand::{HandValue, is_blackjack, is_value_pair};
use crate::rules::{Rules, Soft17};
use crate::shoe::Shoe;
use crate::snapshot::{
    ActiveHand, DealerSnapshot, HandSnapshot, HandStatus, Insurance, Phase, SeatSnapshot,
    ShoeStatus, Snapshot,
};

/// The result of a successful [`Table::apply`]: the new complete view of
/// the table and the ordered events that produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    /// The full table state after the action.
    pub snapshot: Snapshot,
    /// What happened, in order, for animation and the game log.
    pub events: Vec<Event>,
}

/// One player hand and its bet.
#[derive(Debug, Clone)]
struct HandState {
    cards: Vec<Card>,
    bet: u32,
    doubled: bool,
    from_split: bool,
    status: HandStatus,
    outcome: Option<HandOutcome>,
    payout: Option<i32>,
}

impl HandState {
    fn new(bet: u32) -> HandState {
        HandState {
            cards: Vec::with_capacity(8),
            bet,
            doubled: false,
            from_split: false,
            status: HandStatus::Playing,
            outcome: None,
            payout: None,
        }
    }

    fn value(&self) -> HandValue {
        HandValue::of(&self.cards)
    }
}

/// One seat's state for the current round.
#[derive(Debug, Clone)]
struct SeatState {
    bet: Option<u32>,
    insurance: Insurance,
    /// Net insurance result, folded into `round_net` at settlement.
    insurance_net: i32,
    hands: Vec<HandState>,
    round_net: Option<i32>,
}

impl SeatState {
    fn new() -> SeatState {
        SeatState {
            bet: None,
            insurance: Insurance::NotOffered,
            insurance_net: 0,
            hands: Vec::new(),
            round_net: None,
        }
    }

    fn clear(&mut self) {
        *self = SeatState::new();
    }
}

/// A multi-seat blackjack table: one shoe, one rule set, rounds forever.
///
/// The table is generic over the shoe's random number generator so tests
/// and replays can inject a seeded one; see [`Table::from_seed`]. Who
/// controls each seat is outside the engine's concern — a seat is active
/// or it is not, and [`Table::apply`] accepts actions for whichever seat
/// the phase says may act.
#[derive(Debug, Clone)]
pub struct Table<R: RngCore> {
    rules: Rules,
    shoe: Shoe<R>,
    seats: Vec<SeatState>,
    /// Dealer cards: index 0 the upcard, index 1 the hole card, then draws.
    dealer: Vec<Card>,
    hole_revealed: bool,
    phase: Phase,
    /// The seat/hand awaiting a decision (insurance or play).
    active: Option<(usize, usize)>,
}

impl<R: RngCore> Table<R> {
    /// Open a table: build and shuffle the shoe from `rules`, burn one
    /// card, and open betting.
    ///
    /// # Panics
    ///
    /// Panics if the rules are internally inconsistent: no seats, no
    /// decks, a bad penetration, `min_bet` of zero or above `max_bet`, or
    /// a split limit of zero.
    pub fn new(rules: Rules, rng: R) -> Table<R> {
        assert!(rules.seats > 0, "a table needs at least one seat");
        assert!(
            rules.min_bet > 0 && rules.min_bet <= rules.max_bet,
            "table limits must satisfy 0 < min_bet <= max_bet"
        );
        assert!(
            rules.max_split_hands >= 1,
            "max_split_hands must be at least 1"
        );
        let mut shoe = Shoe::new(rules.decks, rules.penetration, rng);
        shoe.burn();
        let seats = (0..rules.seats).map(|_| SeatState::new()).collect();
        Table {
            rules,
            shoe,
            seats,
            dealer: Vec::with_capacity(8),
            hole_revealed: false,
            phase: Phase::Betting,
            active: None,
        }
    }

    /// The frozen rules this table plays by.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// The current resting phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Perform `action` for `seat`.
    ///
    /// On success the table advances — running any automatic stages
    /// (dealing, peek, dealer draw, settlement) to the next resting point
    /// — and returns the new [`Snapshot`] plus ordered [`Event`]s. On
    /// failure a typed [`ActionError`] is returned and the table is
    /// untouched.
    pub fn apply(&mut self, seat: usize, action: Action) -> Result<Transition, ActionError> {
        if seat >= self.rules.seats {
            return Err(ActionError::SeatOutOfRange {
                seat,
                seats: self.rules.seats,
            });
        }
        let mut events = Vec::new();
        match action {
            Action::PlaceBet(amount) => self.place_bet(seat, amount, &mut events)?,
            Action::Deal => self.deal(&mut events)?,
            Action::TakeInsurance => self.decide_insurance(seat, true, &mut events)?,
            Action::DeclineInsurance => self.decide_insurance(seat, false, &mut events)?,
            Action::Hit => self.hit(seat, &mut events)?,
            Action::Stand => self.stand(seat, &mut events)?,
            Action::Double => self.double(seat, &mut events)?,
            Action::Split => self.split(seat, &mut events)?,
            Action::Surrender => self.surrender(seat, &mut events)?,
            Action::NextRound => self.next_round(&mut events)?,
        }
        Ok(Transition {
            snapshot: self.snapshot(),
            events,
        })
    }

    /// The complete visible state of the table right now.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            rules: self.rules.clone(),
            phase: self.phase,
            seats: self.seats.iter().map(seat_snapshot).collect(),
            dealer: DealerSnapshot {
                upcard: self.dealer.first().copied(),
                hole_card: if self.hole_revealed {
                    self.dealer.get(1).copied()
                } else {
                    None
                },
                hole_card_dealt: self.dealer.len() >= 2,
                draws: self.dealer.get(2..).unwrap_or_default().to_vec(),
                total: if self.hole_revealed {
                    Some(HandValue::of(&self.dealer).total())
                } else {
                    None
                },
            },
            active: self.active.map(|(seat, hand)| ActiveHand { seat, hand }),
            shoe: ShoeStatus {
                cards_dealt: self.shoe.cards_dealt(),
                cards_remaining: self.shoe.cards_remaining(),
                discard_pile_size: self.shoe.discard_pile_size(),
                cut_card_reached: self.shoe.cut_card_reached(),
            },
            legal_actions: self.legal_actions(),
        }
    }

    /// Exactly the action kinds [`Table::apply`] will accept next. Never
    /// empty: even a finished round accepts [`Action::NextRound`].
    pub fn legal_actions(&self) -> Vec<ActionKind> {
        match self.phase {
            Phase::Betting => {
                let mut legal = vec![ActionKind::PlaceBet];
                if self.seats.iter().any(|s| s.bet.is_some()) {
                    legal.push(ActionKind::Deal);
                }
                legal
            }
            Phase::InsuranceOffer => vec![ActionKind::TakeInsurance, ActionKind::DeclineInsurance],
            Phase::PlayerTurn => {
                let (seat, hand) = self.active.expect("player turn always has an active hand");
                let mut legal = vec![ActionKind::Hit, ActionKind::Stand];
                if self.can_double(seat, hand) {
                    legal.push(ActionKind::Double);
                }
                if self.can_split(seat, hand) {
                    legal.push(ActionKind::Split);
                }
                if self.can_surrender(seat, hand) {
                    legal.push(ActionKind::Surrender);
                }
                legal
            }
            Phase::RoundOver => vec![ActionKind::NextRound],
        }
    }

    // ------------------------------------------------------------------
    // Action handlers. Each validates completely before mutating anything
    // so a rejected action leaves the table untouched.
    // ------------------------------------------------------------------

    fn place_bet(
        &mut self,
        seat: usize,
        amount: u32,
        events: &mut Vec<Event>,
    ) -> Result<(), ActionError> {
        self.require_phase(Phase::Betting, ActionKind::PlaceBet)?;
        if amount < self.rules.min_bet || amount > self.rules.max_bet {
            return Err(ActionError::BetOutOfRange {
                bet: amount,
                min: self.rules.min_bet,
                max: self.rules.max_bet,
            });
        }
        self.seats[seat].bet = Some(amount);
        events.push(Event::BetPlaced { seat, amount });
        Ok(())
    }

    fn deal(&mut self, events: &mut Vec<Event>) -> Result<(), ActionError> {
        self.require_phase(Phase::Betting, ActionKind::Deal)?;
        if self.seats.iter().all(|s| s.bet.is_none()) {
            return Err(ActionError::NoBetsPlaced);
        }
        // Fresh hands for every betting seat.
        for seat in &mut self.seats {
            if let Some(bet) = seat.bet {
                seat.hands = vec![HandState::new(bet)];
            }
        }
        // Two passes around the table, dealer last, hole card face down.
        for pass in 0..2 {
            for seat in 0..self.rules.seats {
                if self.seats[seat].bet.is_some() {
                    self.deal_to_hand(seat, 0, events);
                }
            }
            let card = self.draw_card(events);
            self.dealer.push(card);
            if pass == 0 {
                events.push(Event::DealerCardDealt { card });
            } else {
                events.push(Event::HoleCardDealt);
            }
        }
        // Flag naturals before anyone acts.
        for seat in 0..self.rules.seats {
            let state = &mut self.seats[seat];
            if state.bet.is_some() && is_blackjack(&state.hands[0].cards) {
                state.hands[0].status = HandStatus::Blackjack;
                events.push(Event::PlayerBlackjack { seat });
            }
        }
        let upcard = self.dealer[0];
        if upcard.rank.is_ace() && self.rules.insurance_offered {
            // Insurance must resolve before the peek.
            for state in &mut self.seats {
                if state.bet.is_some() {
                    state.insurance = Insurance::Pending;
                }
            }
            events.push(Event::InsuranceOffered);
            self.phase = Phase::InsuranceOffer;
            let seat = self
                .pending_insurance_seat()
                .expect("a dealt round has at least one betting seat");
            self.active = Some((seat, 0));
        } else if upcard.rank.is_ace() || upcard.rank.is_ten_value() {
            self.peek(events);
        } else {
            self.begin_player_turns(events);
        }
        Ok(())
    }

    fn decide_insurance(
        &mut self,
        seat: usize,
        take: bool,
        events: &mut Vec<Event>,
    ) -> Result<(), ActionError> {
        let kind = if take {
            ActionKind::TakeInsurance
        } else {
            ActionKind::DeclineInsurance
        };
        self.require_phase(Phase::InsuranceOffer, kind)?;
        let (active_seat, _) = self.active.expect("insurance offer has an active seat");
        if seat != active_seat {
            return Err(ActionError::OutOfTurn { seat, active_seat });
        }
        if take {
            // Insurance is half the main bet, rounded down to a whole dollar.
            let amount = self.seats[seat].bet.expect("insured seat has a bet") / 2;
            self.seats[seat].insurance = Insurance::Taken { amount };
            events.push(Event::InsuranceTaken { seat, amount });
        } else {
            self.seats[seat].insurance = Insurance::Declined;
            events.push(Event::InsuranceDeclined { seat });
        }
        match self.pending_insurance_seat() {
            Some(next) => self.active = Some((next, 0)),
            None => {
                self.active = None;
                self.peek(events);
            }
        }
        Ok(())
    }

    fn hit(&mut self, seat: usize, events: &mut Vec<Event>) -> Result<(), ActionError> {
        let (seat, hand) = self.require_active(seat, ActionKind::Hit)?;
        self.deal_to_hand(seat, hand, events);
        self.auto_resolve(seat, hand, events);
        self.advance_from(seat, hand, events);
        Ok(())
    }

    fn stand(&mut self, seat: usize, events: &mut Vec<Event>) -> Result<(), ActionError> {
        let (seat, hand) = self.require_active(seat, ActionKind::Stand)?;
        let total = self.seats[seat].hands[hand].value().total();
        self.seats[seat].hands[hand].status = HandStatus::Stood;
        events.push(Event::PlayerStood { seat, hand, total });
        self.advance_from(seat, hand, events);
        Ok(())
    }

    fn double(&mut self, seat: usize, events: &mut Vec<Event>) -> Result<(), ActionError> {
        let (seat, hand) = self.require_active(seat, ActionKind::Double)?;
        if !self.can_double(seat, hand) {
            return Err(ActionError::DoubleNotAllowed);
        }
        {
            let state = &mut self.seats[seat].hands[hand];
            state.bet *= 2;
            state.doubled = true;
            events.push(Event::DoubledDown {
                seat,
                hand,
                bet: state.bet,
            });
        }
        self.deal_to_hand(seat, hand, events);
        // One card and done: stand unless it busted.
        let value = self.seats[seat].hands[hand].value();
        let state = &mut self.seats[seat].hands[hand];
        if value.is_bust() {
            state.status = HandStatus::Bust;
            events.push(Event::HandBusted {
                seat,
                hand,
                total: value.total(),
            });
        } else {
            state.status = HandStatus::Stood;
            events.push(Event::PlayerStood {
                seat,
                hand,
                total: value.total(),
            });
        }
        self.advance_from(seat, hand, events);
        Ok(())
    }

    fn split(&mut self, seat: usize, events: &mut Vec<Event>) -> Result<(), ActionError> {
        let (seat, hand) = self.require_active(seat, ActionKind::Split)?;
        if !self.can_split(seat, hand) {
            return Err(ActionError::SplitNotAllowed);
        }
        let bet = self.seats[seat].hands[hand].bet;
        let second = self.seats[seat].hands[hand]
            .cards
            .pop()
            .expect("a splittable hand has two cards");
        self.seats[seat].hands[hand].from_split = true;
        let mut new_hand = HandState::new(bet);
        new_hand.from_split = true;
        new_hand.cards.push(second);
        self.seats[seat].hands.insert(hand + 1, new_hand);
        events.push(Event::HandSplit { seat, hand });
        // The first split hand draws immediately; the second waits for the
        // cursor, as at a real table.
        self.deal_to_hand(seat, hand, events);
        self.auto_resolve(seat, hand, events);
        self.advance_from(seat, hand, events);
        Ok(())
    }

    fn surrender(&mut self, seat: usize, events: &mut Vec<Event>) -> Result<(), ActionError> {
        let (seat, hand) = self.require_active(seat, ActionKind::Surrender)?;
        if !self.can_surrender(seat, hand) {
            return Err(ActionError::SurrenderNotAllowed);
        }
        self.seats[seat].hands[hand].status = HandStatus::Surrendered;
        events.push(Event::PlayerSurrendered { seat });
        self.advance_from(seat, hand, events);
        Ok(())
    }

    fn next_round(&mut self, events: &mut Vec<Event>) -> Result<(), ActionError> {
        self.require_phase(Phase::RoundOver, ActionKind::NextRound)?;
        // Everything on the felt goes to the discard pile.
        let dealer_cards: Vec<Card> = self.dealer.drain(..).collect();
        self.shoe.discard_all(dealer_cards);
        for seat in &mut self.seats {
            for hand in seat.hands.drain(..) {
                self.shoe.discard_all(hand.cards);
            }
            seat.clear();
        }
        self.hole_revealed = false;
        if self.shoe.cut_card_reached() {
            self.shoe.reshuffle();
            events.push(Event::ShoeShuffled);
            self.shoe.burn();
            events.push(Event::CardBurned);
        }
        self.phase = Phase::Betting;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Validation helpers (no mutation).
    // ------------------------------------------------------------------

    fn require_phase(&self, phase: Phase, action: ActionKind) -> Result<(), ActionError> {
        if self.phase != phase {
            return Err(ActionError::WrongPhase {
                action,
                phase: self.phase,
            });
        }
        Ok(())
    }

    fn require_active(
        &self,
        seat: usize,
        action: ActionKind,
    ) -> Result<(usize, usize), ActionError> {
        self.require_phase(Phase::PlayerTurn, action)?;
        let (active_seat, hand) = self.active.expect("player turn always has an active hand");
        if seat != active_seat {
            return Err(ActionError::OutOfTurn { seat, active_seat });
        }
        Ok((active_seat, hand))
    }

    fn can_double(&self, seat: usize, hand: usize) -> bool {
        let state = &self.seats[seat].hands[hand];
        if state.cards.len() != 2 {
            return false;
        }
        if state.from_split && !self.rules.double_after_split {
            return false;
        }
        if self.rules.double_on_any_two {
            return true;
        }
        let value = state.value();
        !value.is_soft() && (9..=11).contains(&value.total())
    }

    fn can_split(&self, seat: usize, hand: usize) -> bool {
        // Splitting is by blackjack value: any two ten-value cards are a
        // splittable pair, as at most multi-deck shoes.
        self.seats[seat].hands.len() < self.rules.max_split_hands
            && is_value_pair(&self.seats[seat].hands[hand].cards)
    }

    fn can_surrender(&self, seat: usize, hand: usize) -> bool {
        self.rules.late_surrender
            && self.seats[seat].hands.len() == 1
            && self.seats[seat].hands[hand].cards.len() == 2
            && !self.seats[seat].hands[hand].from_split
    }

    fn pending_insurance_seat(&self) -> Option<usize> {
        self.seats
            .iter()
            .position(|s| s.insurance == Insurance::Pending)
    }

    // ------------------------------------------------------------------
    // Automatic stages.
    // ------------------------------------------------------------------

    /// Draw a card, shuffling the discard pile back in if the shoe runs
    /// dry mid-round (the cut card normally prevents this).
    ///
    /// # Panics
    ///
    /// Panics if both the shoe and discard pile are empty, which requires
    /// a pathological configuration (every card of the shoe on the felt).
    fn draw_card(&mut self, events: &mut Vec<Event>) -> Card {
        if let Some(card) = self.shoe.draw() {
            return card;
        }
        self.shoe.reshuffle();
        events.push(Event::ShoeShuffled);
        self.shoe
            .draw()
            .expect("shoe and discard pile cannot both be empty mid-round")
    }

    fn deal_to_hand(&mut self, seat: usize, hand: usize, events: &mut Vec<Event>) {
        let card = self.draw_card(events);
        self.seats[seat].hands[hand].cards.push(card);
        events.push(Event::CardDealt { seat, hand, card });
    }

    /// Apply automatic stands and busts after a hand receives a card:
    /// busts end the hand, a one-card split ace stands (when the rules say
    /// so), and any 21 stands — hitting 21 is never useful.
    fn auto_resolve(&mut self, seat: usize, hand: usize, events: &mut Vec<Event>) {
        let value = self.seats[seat].hands[hand].value();
        let state = &mut self.seats[seat].hands[hand];
        if value.is_bust() {
            state.status = HandStatus::Bust;
            events.push(Event::HandBusted {
                seat,
                hand,
                total: value.total(),
            });
        } else {
            let one_card_split_ace = state.from_split
                && self.rules.split_aces_one_card
                && state.cards[0].rank.is_ace()
                && state.cards.len() == 2;
            if one_card_split_ace || value.total() == 21 {
                state.status = HandStatus::Stood;
                events.push(Event::PlayerStood {
                    seat,
                    hand,
                    total: value.total(),
                });
            }
        }
    }

    fn begin_player_turns(&mut self, events: &mut Vec<Event>) {
        self.phase = Phase::PlayerTurn;
        self.advance_from(0, 0, events);
    }

    /// Move the active cursor forward from `(seat, hand)` in casino order,
    /// dealing second cards to fresh split hands, skipping resolved hands
    /// and empty seats, and running the dealer's turn when every player
    /// hand is done.
    fn advance_from(&mut self, mut seat: usize, mut hand: usize, events: &mut Vec<Event>) {
        loop {
            if seat >= self.rules.seats {
                self.active = None;
                self.dealer_turn(events);
                return;
            }
            if self.seats[seat].bet.is_none() || hand >= self.seats[seat].hands.len() {
                seat += 1;
                hand = 0;
                continue;
            }
            if self.seats[seat].hands[hand].status != HandStatus::Playing {
                hand += 1;
                continue;
            }
            if self.seats[seat].hands[hand].cards.len() < 2 {
                // A split hand waiting for its second card gets it now.
                self.deal_to_hand(seat, hand, events);
                self.auto_resolve(seat, hand, events);
                continue;
            }
            self.active = Some((seat, hand));
            return;
        }
    }

    /// Reveal the hole card, draw to 17 (S17/H17 per the rules) if any
    /// player hand is still standing, then settle the round.
    fn dealer_turn(&mut self, events: &mut Vec<Event>) {
        self.hole_revealed = true;
        events.push(Event::HoleCardRevealed {
            card: self.dealer[1],
        });
        let any_standing = self
            .seats
            .iter()
            .flat_map(|s| &s.hands)
            .any(|h| h.status == HandStatus::Stood);
        if any_standing {
            loop {
                let value = HandValue::of(&self.dealer);
                if value.is_bust() {
                    events.push(Event::DealerBust {
                        total: value.total(),
                    });
                    break;
                }
                let stands = value.total() > 17
                    || (value.total() == 17
                        && (!value.is_soft() || self.rules.soft_17 == Soft17::Stand));
                if stands {
                    events.push(Event::DealerStood {
                        total: value.total(),
                    });
                    break;
                }
                let card = self.draw_card(events);
                self.dealer.push(card);
                events.push(Event::DealerCardDealt { card });
            }
        }
        self.settle(events);
    }

    /// The dealer has blackjack (found on the peek): reveal, pay insurance
    /// at 2:1, and settle every hand — player blackjacks push, everything
    /// else loses.
    fn dealer_blackjack(&mut self, events: &mut Vec<Event>) {
        self.hole_revealed = true;
        events.push(Event::HoleCardRevealed {
            card: self.dealer[1],
        });
        self.resolve_insurance(true, events);
        self.settle(events);
    }

    /// Settle every insurance bet: 2:1 winnings on a dealer blackjack,
    /// stake lost otherwise. Nets are folded into each seat's round total.
    fn resolve_insurance(&mut self, dealer_blackjack: bool, events: &mut Vec<Event>) {
        for seat in 0..self.rules.seats {
            if let Insurance::Taken { amount } = self.seats[seat].insurance {
                let net = if dealer_blackjack {
                    2 * amount as i32
                } else {
                    -(amount as i32)
                };
                self.seats[seat].insurance_net = net;
                events.push(Event::InsuranceResolved { seat, amount: net });
            }
        }
    }

    /// The American hole-card peek, made whenever the upcard is an ace or
    /// a ten-value card (after insurance resolves, with an ace up).
    fn peek(&mut self, events: &mut Vec<Event>) {
        let blackjack = is_blackjack(&self.dealer);
        events.push(Event::DealerPeeked { blackjack });
        if blackjack {
            self.dealer_blackjack(events);
        } else {
            self.resolve_insurance(false, events);
            self.begin_player_turns(events);
        }
    }

    /// Settle every hand against the dealer, record per-seat nets, and end
    /// the round.
    ///
    /// Payout arithmetic, all rounded down to whole dollars where a rule
    /// creates fractions: naturals pay the table's blackjack payout; wins
    /// pay 1:1; pushes return the bet; busts and losses forfeit the bet
    /// (doubled hands forfeit the doubled bet); surrender recovers
    /// `bet / 2` rounded down, so the net loss is `bet - bet / 2`.
    fn settle(&mut self, events: &mut Vec<Event>) {
        let dealer_value = HandValue::of(&self.dealer);
        let dealer_bust = dealer_value.is_bust();
        let dealer_blackjack = is_blackjack(&self.dealer);
        for seat in 0..self.rules.seats {
            if self.seats[seat].bet.is_none() {
                continue;
            }
            let mut net = self.seats[seat].insurance_net;
            for hand in 0..self.seats[seat].hands.len() {
                let state = &self.seats[seat].hands[hand];
                let bet = state.bet;
                let (outcome, amount) = match state.status {
                    HandStatus::Surrendered => (HandOutcome::Surrender, -((bet - bet / 2) as i32)),
                    HandStatus::Bust => (HandOutcome::Bust, -(bet as i32)),
                    HandStatus::Blackjack => {
                        if dealer_blackjack {
                            (HandOutcome::Push, 0)
                        } else {
                            let winnings = self.rules.blackjack_payout.winnings(bet);
                            (HandOutcome::Blackjack, winnings as i32)
                        }
                    }
                    // A hand still `Playing` here means the dealer's
                    // blackjack ended the round before its turn.
                    HandStatus::Playing => (HandOutcome::Lose, -(bet as i32)),
                    HandStatus::Stood => {
                        let total = state.value().total();
                        if dealer_bust || total > dealer_value.total() {
                            (HandOutcome::Win, bet as i32)
                        } else if total == dealer_value.total() {
                            (HandOutcome::Push, 0)
                        } else {
                            (HandOutcome::Lose, -(bet as i32))
                        }
                    }
                };
                let state = &mut self.seats[seat].hands[hand];
                state.outcome = Some(outcome);
                state.payout = Some(amount);
                net += amount;
                events.push(Event::HandSettled {
                    seat,
                    hand,
                    outcome,
                    amount,
                });
            }
            self.seats[seat].round_net = Some(net);
        }
        self.phase = Phase::RoundOver;
        self.active = None;
        if self.shoe.cut_card_reached() {
            events.push(Event::CutCardReached);
        }
    }
}

impl Table<ChaCha8Rng> {
    /// Open a table whose shoe is seeded from `seed`, for deterministic
    /// tests and replays. The same rules and seed always produce the same
    /// cards.
    pub fn from_seed(rules: Rules, seed: u64) -> Table<ChaCha8Rng> {
        Table::new(rules, ChaCha8Rng::seed_from_u64(seed))
    }
}

fn seat_snapshot(seat: &SeatState) -> SeatSnapshot {
    SeatSnapshot {
        bet: seat.bet,
        insurance: seat.insurance,
        hands: seat
            .hands
            .iter()
            .map(|hand| {
                let value = hand.value();
                HandSnapshot {
                    cards: hand.cards.clone(),
                    bet: hand.bet,
                    doubled: hand.doubled,
                    from_split: hand.from_split,
                    status: hand.status,
                    total: value.total(),
                    soft: value.is_soft(),
                    outcome: hand.outcome,
                    payout: hand.payout,
                }
            })
            .collect(),
        round_net: seat.round_net,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(seed: u64) -> Table<ChaCha8Rng> {
        Table::from_seed(Rules::canonical(), seed)
    }

    #[test]
    fn a_new_table_opens_in_betting_with_one_burned_card() {
        let t = table(0);
        let snap = t.snapshot();
        assert_eq!(snap.phase, Phase::Betting);
        assert_eq!(snap.seats.len(), 7);
        assert_eq!(snap.shoe.cards_dealt, 1);
        assert_eq!(snap.shoe.discard_pile_size, 1);
        assert_eq!(snap.legal_actions, [ActionKind::PlaceBet]);
    }

    #[test]
    fn deal_becomes_legal_once_a_bet_is_down() {
        let mut t = table(0);
        let transition = t.apply(0, Action::PlaceBet(10)).unwrap();
        assert_eq!(
            transition.events,
            [Event::BetPlaced {
                seat: 0,
                amount: 10
            }]
        );
        assert_eq!(
            transition.snapshot.legal_actions,
            [ActionKind::PlaceBet, ActionKind::Deal]
        );
    }

    #[test]
    fn bets_outside_table_limits_are_rejected() {
        let mut t = table(0);
        assert_eq!(
            t.apply(0, Action::PlaceBet(5)),
            Err(ActionError::BetOutOfRange {
                bet: 5,
                min: 10,
                max: 500
            })
        );
        assert_eq!(
            t.apply(0, Action::PlaceBet(501)),
            Err(ActionError::BetOutOfRange {
                bet: 501,
                min: 10,
                max: 500
            })
        );
    }

    #[test]
    fn dealing_with_no_bets_is_rejected() {
        let mut t = table(0);
        assert_eq!(t.apply(0, Action::Deal), Err(ActionError::NoBetsPlaced));
    }

    #[test]
    fn seat_indexes_are_bounds_checked() {
        let mut t = table(0);
        assert_eq!(
            t.apply(7, Action::PlaceBet(10)),
            Err(ActionError::SeatOutOfRange { seat: 7, seats: 7 })
        );
    }

    #[test]
    fn dealing_gives_two_cards_per_bettor_and_the_dealer() {
        let mut t = table(0);
        t.apply(1, Action::PlaceBet(10)).unwrap();
        t.apply(4, Action::PlaceBet(25)).unwrap();
        let transition = t.apply(0, Action::Deal).unwrap();
        let snap = transition.snapshot;
        assert_eq!(snap.seats[1].hands[0].cards.len(), 2);
        assert_eq!(snap.seats[4].hands[0].cards.len(), 2);
        assert!(snap.seats[0].hands.is_empty());
        assert!(snap.dealer.upcard.is_some());
        assert!(snap.dealer.hole_card_dealt);
        assert_eq!(snap.dealer.hole_card, None, "hole card must stay hidden");
        assert_eq!(snap.dealer.total, None);
        // 1 burn + 4 player cards + 2 dealer cards.
        assert_eq!(snap.shoe.cards_dealt, 7);
    }

    #[test]
    fn out_of_turn_actions_are_rejected_without_mutation() {
        let mut t = table(0);
        t.apply(1, Action::PlaceBet(10)).unwrap();
        t.apply(4, Action::PlaceBet(10)).unwrap();
        let dealt = t.apply(0, Action::Deal).unwrap();
        if dealt.snapshot.phase != Phase::PlayerTurn {
            return; // Insurance or dealer blackjack round; other seeds cover this.
        }
        let active = dealt.snapshot.active.unwrap().seat;
        let other = if active == 1 { 4 } else { 1 };
        let before = t.snapshot();
        let err = t.apply(other, Action::Hit).unwrap_err();
        assert_eq!(
            err,
            ActionError::OutOfTurn {
                seat: other,
                active_seat: active
            }
        );
        assert_eq!(t.snapshot(), before);
    }

    #[test]
    fn wrong_phase_actions_are_rejected_without_mutation() {
        let mut t = table(0);
        let before = t.snapshot();
        for action in [
            Action::Hit,
            Action::Stand,
            Action::Double,
            Action::Split,
            Action::Surrender,
            Action::TakeInsurance,
            Action::DeclineInsurance,
            Action::NextRound,
        ] {
            let err = t.apply(0, action).unwrap_err();
            assert_eq!(
                err,
                ActionError::WrongPhase {
                    action: action.kind(),
                    phase: Phase::Betting
                }
            );
            assert_eq!(t.snapshot(), before);
        }
    }

    #[test]
    fn snapshot_serde_round_trips() {
        let mut t = table(3);
        t.apply(0, Action::PlaceBet(10)).unwrap();
        let transition = t.apply(0, Action::Deal).unwrap();
        let json = serde_json::to_string(&transition).unwrap();
        let back: Transition = serde_json::from_str(&json).unwrap();
        assert_eq!(transition, back);
    }

    #[test]
    #[should_panic(expected = "at least one seat")]
    fn zero_seat_rules_panic() {
        let mut rules = Rules::canonical();
        rules.seats = 0;
        let _ = Table::from_seed(rules, 0);
    }

    #[test]
    #[should_panic(expected = "min_bet")]
    fn inverted_table_limits_panic() {
        let mut rules = Rules::canonical();
        rules.min_bet = 600;
        let _ = Table::from_seed(rules, 0);
    }
}
