//! The display state: what the scene currently shows, kept separate
//! from the engine's authoritative snapshot so events can play out one
//! at a time.
//!
//! A [`DisplayState`] holds a synthetic [`Snapshot`] advanced
//! event-by-event: every [`Event`] has a known physical effect (a card
//! joins a hand, a bet appears, the shoe shortens), and applying it
//! patches the display exactly that far. Hand totals are computed with
//! the engine's own [`HandValue`] — the UI choreographs but never
//! invents game facts. At the end of every transition the queue adopts
//! the authoritative snapshot ([`DisplayState::adopt`]), so the display
//! always converges to exactly what the engine reported; the fields the
//! event stream cannot carry (phase, active hand, legal actions, the
//! cut card's mid-round position) are corrected there.

use blackjack_core::{
    DealerSnapshot, Event, HandSnapshot, HandStatus, HandValue, Insurance, Phase, Snapshot,
};

/// What the scene currently shows, advanced event-by-event between the
/// engine's resting snapshots.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DisplayState {
    /// The synthetic snapshot; `None` until the first transition lands.
    snapshot: Option<Snapshot>,
    /// Per-seat net insurance results for the round in progress. Side
    /// memory only: [`Event::InsuranceResolved`] can arrive transitions
    /// before the settlement that folds it into `round_net`, and the
    /// snapshot has no field for it in between.
    insurance_net: Vec<i32>,
}

impl DisplayState {
    /// An empty display: nothing on screen yet.
    pub fn new() -> DisplayState {
        DisplayState::default()
    }

    /// The snapshot the scene should render right now.
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// Adopt an authoritative engine snapshot wholesale. Called at the
    /// end of every transition's animation queue so the display always
    /// converges exactly.
    pub fn adopt(&mut self, snapshot: Snapshot) {
        self.insurance_net.resize(snapshot.seats.len(), 0);
        self.snapshot = Some(snapshot);
    }

    /// Whether the felt still shows a settled round that `target` (the
    /// next authoritative snapshot) has already cleared. The engine
    /// clears the felt inside `NextRound` without emitting an event —
    /// physically the dealer sweeps every card to the discard tray — so
    /// the choreographer synthesizes that sweep when this reports true.
    pub fn needs_clear(&self, target: &Snapshot) -> bool {
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        let showing = snapshot.seats.iter().any(|seat| !seat.hands.is_empty())
            || snapshot.dealer.upcard.is_some();
        let cleared =
            target.seats.iter().all(|seat| seat.hands.is_empty()) && target.dealer.upcard.is_none();
        showing && cleared
    }

    /// Sweep the felt: every card to the discard tray, every seat back
    /// to empty, exactly as the engine's `NextRound` does silently.
    pub fn clear_felt(&mut self) {
        let Some(snapshot) = &mut self.snapshot else {
            return;
        };
        let dealer = &snapshot.dealer;
        let mut felt_cards = usize::from(dealer.upcard.is_some())
            + usize::from(dealer.hole_card.is_some() || dealer.hole_card_dealt)
            + dealer.draws.len();
        for seat in &snapshot.seats {
            felt_cards += seat
                .hands
                .iter()
                .map(|hand| hand.cards.len())
                .sum::<usize>();
        }
        snapshot.shoe.discard_pile_size += felt_cards;
        for seat in &mut snapshot.seats {
            seat.bet = None;
            seat.insurance = Insurance::NotOffered;
            seat.hands.clear();
            seat.round_net = None;
        }
        snapshot.dealer = DealerSnapshot {
            upcard: None,
            hole_card: None,
            hole_card_dealt: false,
            draws: Vec::new(),
            total: None,
        };
        snapshot.phase = Phase::Betting;
        snapshot.active = None;
        self.insurance_net.fill(0);
    }

    /// Advance the display by one event's physical effect.
    ///
    /// A no-op until a snapshot has been adopted. Events with no visible
    /// state change (peeks, arrivals, departures, dealer changes) leave
    /// the snapshot untouched — their effect is pacing, handled by the
    /// planner.
    pub fn apply(&mut self, event: &Event) {
        let Some(snapshot) = &mut self.snapshot else {
            return;
        };
        self.insurance_net.resize(snapshot.seats.len(), 0);
        match *event {
            Event::BetPlaced { seat, amount } => {
                snapshot.seats[seat].bet = Some(amount);
            }
            Event::ShoeShuffled => {
                let shoe = &mut snapshot.shoe;
                shoe.cards_remaining += shoe.discard_pile_size;
                shoe.cards_dealt = shoe.cards_dealt.saturating_sub(shoe.discard_pile_size);
                shoe.discard_pile_size = 0;
                shoe.cut_card_reached = false;
            }
            Event::CardBurned => {
                let shoe = &mut snapshot.shoe;
                shoe.cards_dealt += 1;
                shoe.cards_remaining = shoe.cards_remaining.saturating_sub(1);
                shoe.discard_pile_size += 1;
            }
            Event::CardDealt { seat, hand, card } => {
                draw_from_shoe(snapshot);
                let seat_state = &mut snapshot.seats[seat];
                let bet = seat_state.bet.unwrap_or(0);
                while seat_state.hands.len() <= hand {
                    seat_state.hands.push(empty_hand(bet));
                }
                let hand_state = &mut seat_state.hands[hand];
                hand_state.cards.push(card);
                refresh_value(hand_state);
            }
            Event::DealerCardDealt { card } => {
                draw_from_shoe(snapshot);
                let dealer = &mut snapshot.dealer;
                if dealer.upcard.is_none() {
                    dealer.upcard = Some(card);
                } else {
                    dealer.draws.push(card);
                }
                refresh_dealer_total(dealer);
            }
            Event::HoleCardDealt => {
                draw_from_shoe(snapshot);
                snapshot.dealer.hole_card_dealt = true;
            }
            Event::HoleCardRevealed { card } => {
                let dealer = &mut snapshot.dealer;
                dealer.hole_card = Some(card);
                dealer.hole_card_dealt = true;
                refresh_dealer_total(dealer);
            }
            Event::PlayerBlackjack { seat } => {
                if let Some(hand) = snapshot.seats[seat].hands.first_mut() {
                    hand.status = HandStatus::Blackjack;
                }
            }
            Event::InsuranceOffered => {
                for seat in &mut snapshot.seats {
                    if seat.bet.is_some() {
                        seat.insurance = Insurance::Pending;
                    }
                }
                snapshot.phase = Phase::InsuranceOffer;
            }
            Event::InsuranceTaken { seat, amount } => {
                snapshot.seats[seat].insurance = Insurance::Taken { amount };
            }
            Event::InsuranceDeclined { seat } => {
                snapshot.seats[seat].insurance = Insurance::Declined;
            }
            Event::InsuranceResolved { seat, amount } => {
                self.insurance_net[seat] = amount;
            }
            Event::DealerPeeked { .. } => {}
            Event::HandSplit { seat, hand } => {
                let seat_state = &mut snapshot.seats[seat];
                let original = &mut seat_state.hands[hand];
                let second = original.cards.pop();
                original.from_split = true;
                refresh_value(original);
                let bet = original.bet;
                let mut new_hand = empty_hand(bet);
                new_hand.from_split = true;
                new_hand.cards.extend(second);
                refresh_value(&mut new_hand);
                seat_state.hands.insert(hand + 1, new_hand);
            }
            Event::DoubledDown { seat, hand, bet } => {
                let hand_state = &mut snapshot.seats[seat].hands[hand];
                hand_state.bet = bet;
                hand_state.doubled = true;
            }
            Event::PlayerStood { seat, hand, .. } => {
                snapshot.seats[seat].hands[hand].status = HandStatus::Stood;
            }
            Event::HandBusted { seat, hand, .. } => {
                snapshot.seats[seat].hands[hand].status = HandStatus::Bust;
            }
            Event::PlayerSurrendered { seat } => {
                // Surrender is only legal on an original two-card hand,
                // so the playing hand is unambiguous.
                if let Some(hand) = snapshot.seats[seat]
                    .hands
                    .iter_mut()
                    .find(|hand| hand.status == HandStatus::Playing)
                {
                    hand.status = HandStatus::Surrendered;
                }
            }
            Event::DealerStood { total } | Event::DealerBust { total } => {
                snapshot.dealer.total = Some(total);
            }
            Event::HandSettled {
                seat,
                hand,
                outcome,
                amount,
            } => {
                let seat_state = &mut snapshot.seats[seat];
                let hand_state = &mut seat_state.hands[hand];
                hand_state.outcome = Some(outcome);
                hand_state.payout = Some(amount);
                // The engine folds the insurance net in once per seat.
                let net = seat_state.round_net.get_or_insert(self.insurance_net[seat]);
                *net += amount;
                snapshot.phase = Phase::RoundOver;
                snapshot.active = None;
            }
            Event::CutCardReached => {
                snapshot.shoe.cut_card_reached = true;
            }
            Event::PlayerArrived { .. } | Event::PlayerDeparted { .. } | Event::DealerChanged => {}
        }
    }
}

/// One drawn card's shoe bookkeeping.
fn draw_from_shoe(snapshot: &mut Snapshot) {
    snapshot.shoe.cards_dealt += 1;
    snapshot.shoe.cards_remaining = snapshot.shoe.cards_remaining.saturating_sub(1);
}

/// A fresh, empty hand awaiting its first card.
fn empty_hand(bet: u32) -> HandSnapshot {
    HandSnapshot {
        cards: Vec::new(),
        bet,
        doubled: false,
        from_split: false,
        status: HandStatus::Playing,
        total: 0,
        soft: false,
        outcome: None,
        payout: None,
    }
}

/// Recompute a hand's total with the engine's own evaluator.
fn refresh_value(hand: &mut HandSnapshot) {
    let value = HandValue::of(&hand.cards);
    hand.total = value.total();
    hand.soft = value.is_soft();
}

/// Recompute the dealer's visible total: known only once the hole card
/// is face up, exactly as the engine reports it.
fn refresh_dealer_total(dealer: &mut DealerSnapshot) {
    dealer.total = dealer.hole_card.map(|hole| {
        let mut cards = Vec::with_capacity(2 + dealer.draws.len());
        cards.extend(dealer.upcard);
        cards.push(hole);
        cards.extend(dealer.draws.iter().copied());
        HandValue::of(&cards).total()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackjack_core::{
        Action, ActionKind, Awaiting, Card, Rank, Rules, Suit, TableLife, basic_strategy,
    };
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::SeedableRng;

    /// Strip the fields the event stream cannot carry: phase, active
    /// hand, and legal actions rest between transitions; the cut card's
    /// mid-round position is engine-internal until [`Event::CutCardReached`].
    /// All are corrected by [`DisplayState::adopt`] at queue end.
    fn animatable(mut snapshot: Snapshot) -> Snapshot {
        snapshot.phase = Phase::Betting;
        snapshot.active = None;
        snapshot.legal_actions.clear();
        snapshot.shoe.cut_card_reached = false;
        snapshot
    }

    #[test]
    fn events_patch_cards_bets_and_the_shoe() {
        let table = blackjack_core::Table::from_seed(Rules::canonical(), 3);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        let before = display.snapshot().unwrap().shoe;

        display.apply(&Event::BetPlaced {
            seat: 2,
            amount: 25,
        });
        assert_eq!(display.snapshot().unwrap().seats[2].bet, Some(25));

        let card = Card::new(Rank::Ace, Suit::Spades);
        display.apply(&Event::CardDealt {
            seat: 2,
            hand: 0,
            card,
        });
        let snapshot = display.snapshot().unwrap();
        let hand = &snapshot.seats[2].hands[0];
        assert_eq!(hand.cards, vec![card]);
        assert_eq!(hand.bet, 25);
        assert_eq!(hand.total, 11);
        assert!(hand.soft);
        assert_eq!(snapshot.shoe.cards_remaining, before.cards_remaining - 1);
        assert_eq!(snapshot.shoe.cards_dealt, before.cards_dealt + 1);
    }

    #[test]
    fn the_hole_card_reveal_completes_the_dealer_total() {
        let table = blackjack_core::Table::from_seed(Rules::canonical(), 3);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        display.apply(&Event::DealerCardDealt {
            card: Card::new(Rank::King, Suit::Hearts),
        });
        display.apply(&Event::HoleCardDealt);
        let dealer = &display.snapshot().unwrap().dealer;
        assert!(dealer.hole_card_dealt && dealer.total.is_none());
        display.apply(&Event::HoleCardRevealed {
            card: Card::new(Rank::Seven, Suit::Clubs),
        });
        let dealer = &display.snapshot().unwrap().dealer;
        assert_eq!(dealer.hole_card, Some(Card::new(Rank::Seven, Suit::Clubs)));
        assert_eq!(dealer.total, Some(17));
    }

    #[test]
    fn clearing_the_felt_sends_every_card_to_the_discard_tray() {
        let table = blackjack_core::Table::from_seed(Rules::canonical(), 3);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        let discard_before = display.snapshot().unwrap().shoe.discard_pile_size;
        display.apply(&Event::BetPlaced {
            seat: 0,
            amount: 10,
        });
        for card in [
            Card::new(Rank::Five, Suit::Clubs),
            Card::new(Rank::Nine, Suit::Hearts),
        ] {
            display.apply(&Event::CardDealt {
                seat: 0,
                hand: 0,
                card,
            });
        }
        display.apply(&Event::DealerCardDealt {
            card: Card::new(Rank::Two, Suit::Spades),
        });
        display.apply(&Event::HoleCardDealt);
        display.clear_felt();
        let snapshot = display.snapshot().unwrap();
        assert!(snapshot.seats.iter().all(|seat| seat.hands.is_empty()));
        assert!(snapshot.seats.iter().all(|seat| seat.bet.is_none()));
        assert!(snapshot.dealer.upcard.is_none());
        assert_eq!(snapshot.shoe.discard_pile_size, discard_before + 4);
    }

    /// The strong property: drive whole seeded rounds through
    /// [`TableLife`] and check that replaying each step's events over
    /// the display reproduces the step's final snapshot, transition by
    /// transition, for every animatable field.
    #[test]
    fn replaying_events_reproduces_engine_snapshots_across_seeded_rounds() {
        for seed in 0..4u64 {
            let mut life = TableLife::new(
                Rules::canonical(),
                3,
                ChaCha8Rng::seed_from_u64(seed),
                ChaCha8Rng::seed_from_u64(seed.wrapping_add(1000)),
            );
            let mut display = DisplayState::new();
            display.adopt(life.snapshot());
            let mut settled_hands = 0u32;
            let mut splits = 0u32;
            for _ in 0..600 {
                let step = match life.awaiting() {
                    Awaiting::Engine => life.advance(),
                    Awaiting::HumanBet => life
                        .human_apply(Action::PlaceBet(10))
                        .expect("the minimum bet is legal"),
                    Awaiting::HumanInsurance => life
                        .human_apply(Action::DeclineInsurance)
                        .expect("declining insurance is legal"),
                    Awaiting::HumanTurn => {
                        let snapshot = life.snapshot();
                        let active = snapshot.active.expect("a turn has an active hand");
                        let hand = &snapshot.seats[active.seat].hands[active.hand];
                        let legal = &snapshot.legal_actions;
                        let choice = basic_strategy(
                            &hand.cards,
                            snapshot.dealer.upcard.expect("dealt rounds have an upcard"),
                            legal.contains(&ActionKind::Double),
                            legal.contains(&ActionKind::Split),
                            legal.contains(&ActionKind::Surrender),
                        );
                        let action = match choice {
                            ActionKind::Hit => Action::Hit,
                            ActionKind::Stand => Action::Stand,
                            ActionKind::Double => Action::Double,
                            ActionKind::Split => Action::Split,
                            ActionKind::Surrender => Action::Surrender,
                            _ => Action::Stand,
                        };
                        life.human_apply(action).expect("basic strategy is legal")
                    }
                };
                for event in &step.events {
                    match event {
                        Event::HandSettled { .. } => settled_hands += 1,
                        Event::HandSplit { .. } => splits += 1,
                        _ => {}
                    }
                }
                if display.needs_clear(&step.snapshot) {
                    display.clear_felt();
                }
                for event in &step.events {
                    display.apply(event);
                }
                assert_eq!(
                    animatable(display.snapshot().unwrap().clone()),
                    animatable(step.snapshot.clone()),
                    "seed {seed}: display diverged after events {:?}",
                    step.events
                );
                display.adopt(step.snapshot);
            }
            assert!(settled_hands > 20, "seed {seed} settled too few hands");
            let _ = splits; // Splits occur on some seeds; coverage, not a guarantee.
        }
    }
}
