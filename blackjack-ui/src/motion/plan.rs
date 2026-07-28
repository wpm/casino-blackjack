//! The choreographer: turn a [`Transition`]'s event list into a timed
//! animation queue.
//!
//! [`plan_transition`] walks the events in engine order and emits one
//! [`TimedStep`] per event — a card flying from the shoe, chips sliding
//! to the tray, a pause for a decision — with durations scaled by the
//! dealer's [`Pace`]. Every step carries the display patch that lands
//! when it completes, so the scene advances one event at a time and
//! never renders a transition as an instant batch. The final step of
//! every plan adopts the authoritative snapshot, guaranteeing the
//! display arrives exactly where the engine rests.

use blackjack_core::{Card, Event, HandOutcome, Pace, Snapshot, Transition};

use super::display::DisplayState;
use super::paths::{
    self, Anchor, DEALER_TRAY_POS, DISCARD_POS, SHOE_ANGLE, SHOE_POS, dealer_card_center,
};
use super::sound::SoundCue;

/// What the transient animation layer shows while a step plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visual {
    /// A face-up card.
    Card(Card),
    /// A face-down card.
    Back,
    /// A pile of chips worth this many dollars.
    Chips(u32),
    /// The shuffle's riffle shimmer at the shoe.
    Shimmer,
}

/// The motion a step performs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepKind {
    /// Move `visual` from `from` to `to`, easing out, rotating from the
    /// source's angle to the destination's resting angle.
    Fly {
        /// What flies.
        visual: Visual,
        /// Start anchor (center plus rotation).
        from: Anchor,
        /// End anchor.
        to: Anchor,
    },
    /// Flip the dealer's hole card in place: the back shrinks to an
    /// edge, then `card` grows from it.
    Flip {
        /// The revealed card.
        card: Card,
        /// Where the hole card sits.
        at: Anchor,
    },
    /// The shuffle shimmer at the shoe.
    Shimmer,
    /// Nothing moves; the table breathes (a decision, a peek).
    Pause,
}

/// The display change a step commits.
#[derive(Debug, Clone, PartialEq)]
pub enum Patch {
    /// Apply one engine event to the display state.
    Event(Event),
    /// Sweep the felt clean (the engine's silent `NextRound` clear).
    ClearFelt,
    /// Adopt the authoritative end-of-transition snapshot.
    Adopt(Box<Snapshot>),
    /// No display change.
    None,
}

/// One entry in the animation queue.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedStep {
    /// The motion to play.
    pub kind: StepKind,
    /// How long the step runs.
    pub duration_ms: u32,
    /// The fraction of the step at which `patch` lands. `1.0` for
    /// everything except the flip, whose reveal commits at the turn.
    pub patch_at: f64,
    /// The display change committed at `patch_at`.
    pub patch: Patch,
    /// The step's sound, if any (see [`SoundCue::at_step_start`]).
    pub sound: Option<SoundCue>,
}

impl TimedStep {
    fn pause(duration_ms: u32, patch: Patch) -> TimedStep {
        TimedStep {
            kind: StepKind::Pause,
            duration_ms,
            patch_at: 1.0,
            patch,
            sound: None,
        }
    }
}

/// Total running time of a queue in milliseconds.
pub fn queue_duration_ms(steps: &[TimedStep]) -> u64 {
    steps.iter().map(|step| u64::from(step.duration_ms)).sum()
}

/// Plan the animation queue for one transition.
///
/// `state` is the *planning* display state — the state the scene will
/// show once everything already queued has played. It is advanced to
/// the transition's final snapshot as a side effect, so consecutive
/// transitions queue seamlessly (never dropping events, never batching
/// them into an instant jump).
pub fn plan_transition(
    state: &mut DisplayState,
    transition: &Transition,
    pace: Pace,
) -> Vec<TimedStep> {
    let mut steps = Vec::with_capacity(transition.events.len() + 2);
    if state.needs_clear(&transition.snapshot) {
        // The dealer sweeps the settled round to the discard tray, then
        // the table breathes before the next one.
        steps.push(TimedStep {
            kind: StepKind::Fly {
                visual: Visual::Back,
                from: Anchor::flat(paths::DEALER_HAND_POS),
                to: Anchor::flat(DISCARD_POS),
            },
            duration_ms: pace.settlement_ms,
            patch_at: 1.0,
            patch: Patch::ClearFelt,
            sound: Some(SoundCue::CardSlide),
        });
        steps.push(TimedStep::pause(pace.between_rounds_ms / 2, Patch::None));
        state.clear_felt();
    }
    for event in &transition.events {
        steps.push(plan_event(state, event, pace));
        state.apply(event);
    }
    steps.push(TimedStep::pause(
        0,
        Patch::Adopt(Box::new(transition.snapshot.clone())),
    ));
    state.adopt(transition.snapshot.clone());
    steps
}

/// Plan one event against the display state *before* that event.
fn plan_event(state: &DisplayState, event: &Event, pace: Pace) -> TimedStep {
    let snapshot = state.snapshot();
    let patch = Patch::Event(*event);
    // Positions need a snapshot; without one (never, in practice, since
    // sessions open with an event-free transition) everything is a pause.
    let Some(snapshot) = snapshot else {
        return TimedStep::pause(0, patch);
    };
    let seats = snapshot.seats.len();
    match *event {
        Event::BetPlaced { seat, amount } => chip_fly(
            paths::seat_entry(seats, seat),
            paths::bet_center(seats, seat, 1, 0),
            amount,
            pace.settlement_ms,
            patch,
        ),
        Event::ShoeShuffled => TimedStep {
            kind: StepKind::Shimmer,
            duration_ms: 2 * pace.reveal_ms,
            patch_at: 1.0,
            patch,
            sound: Some(SoundCue::Riffle),
        },
        Event::CardBurned => card_fly(Visual::Back, Anchor::flat(DISCARD_POS), pace.card_ms, patch),
        Event::CardDealt { seat, hand, card } => {
            let seat_state = &snapshot.seats[seat];
            let hand_count = seat_state.hands.len().max(hand + 1);
            let cards_after = seat_state
                .hands
                .get(hand)
                .map_or(1, |hand| hand.cards.len() + 1);
            let mut to = paths::seat_card_center(
                seats,
                seat,
                hand_count,
                hand,
                cards_after - 1,
                cards_after,
            );
            // The double-down card lands sideways, casino-style.
            if seat_state.hands.get(hand).is_some_and(|hand| hand.doubled) {
                to.rot += 90.0;
            }
            card_fly(Visual::Card(card), to, pace.card_ms, patch)
        }
        Event::DealerCardDealt { card } => {
            let dealer = &snapshot.dealer;
            let shown = usize::from(dealer.upcard.is_some())
                + usize::from(dealer.hole_card.is_some() || dealer.hole_card_dealt)
                + dealer.draws.len();
            card_fly(
                Visual::Card(card),
                dealer_card_center(shown, shown + 1),
                pace.card_ms,
                patch,
            )
        }
        Event::HoleCardDealt => {
            card_fly(Visual::Back, dealer_card_center(1, 2), pace.card_ms, patch)
        }
        Event::HoleCardRevealed { card } => {
            let dealer = &snapshot.dealer;
            let shown = usize::from(dealer.upcard.is_some())
                + usize::from(dealer.hole_card.is_some() || dealer.hole_card_dealt)
                + dealer.draws.len();
            TimedStep {
                kind: StepKind::Flip {
                    card,
                    at: dealer_card_center(1, shown.max(2)),
                },
                duration_ms: pace.reveal_ms,
                patch_at: 0.5,
                patch,
                sound: Some(SoundCue::CardSlide),
            }
        }
        Event::PlayerBlackjack { .. } => TimedStep::pause(pace.reveal_ms, patch),
        Event::InsuranceOffered => TimedStep::pause(pace.decision_ms, patch),
        Event::InsuranceTaken { seat, amount } => chip_fly(
            paths::bet_center(seats, seat, 1, 0),
            paths::insurance_center(seats, seat),
            amount,
            pace.settlement_ms,
            patch,
        ),
        Event::InsuranceDeclined { .. } => TimedStep::pause(pace.decision_ms, patch),
        Event::InsuranceResolved { seat, amount } => {
            let line = paths::insurance_center(seats, seat);
            let tray = Anchor::flat(DEALER_TRAY_POS);
            let (from, to) = if amount < 0 {
                (line, tray)
            } else {
                (tray, line)
            };
            chip_fly(from, to, amount.unsigned_abs(), pace.settlement_ms, patch)
        }
        Event::DealerPeeked { .. } => TimedStep::pause(pace.reveal_ms, patch),
        Event::HandSplit { seat, hand } => {
            let hand_count = snapshot.seats[seat].hands.len();
            let moved = snapshot.seats[seat]
                .hands
                .get(hand)
                .and_then(|hand| hand.cards.get(1))
                .copied();
            let from = paths::seat_card_center(seats, seat, hand_count, hand, 1, 2);
            let to = paths::seat_card_center(seats, seat, hand_count + 1, hand + 1, 0, 1);
            TimedStep {
                kind: StepKind::Fly {
                    visual: moved.map_or(Visual::Back, Visual::Card),
                    from,
                    to,
                },
                duration_ms: pace.card_ms,
                patch_at: 1.0,
                patch,
                sound: Some(SoundCue::CardSlide),
            }
        }
        Event::DoubledDown { seat, hand, bet } => {
            let hand_count = snapshot.seats[seat].hands.len().max(1);
            let already = snapshot.seats[seat]
                .hands
                .get(hand)
                .map_or(0, |hand| hand.bet);
            chip_fly(
                paths::seat_entry(seats, seat),
                paths::bet_center(seats, seat, hand_count, hand),
                bet.saturating_sub(already),
                pace.settlement_ms,
                patch,
            )
        }
        Event::PlayerStood { .. } => TimedStep::pause(pace.decision_ms, patch),
        Event::HandBusted { seat, hand, .. } => {
            // The sweep gesture: a card slides to the discard tray. The
            // scene keeps the busted cards scattered and drained of
            // color until the round clears, as #8 drew them.
            let hand_count = snapshot.seats[seat].hands.len().max(1);
            TimedStep {
                kind: StepKind::Fly {
                    visual: Visual::Back,
                    from: paths::seat_hand_center(seats, seat, hand_count, hand),
                    to: Anchor::flat(DISCARD_POS),
                },
                duration_ms: pace.settlement_ms,
                patch_at: 1.0,
                patch,
                sound: Some(SoundCue::CardSlide),
            }
        }
        Event::PlayerSurrendered { .. } => TimedStep::pause(pace.decision_ms, patch),
        Event::DealerStood { .. } | Event::DealerBust { .. } => {
            TimedStep::pause(pace.decision_ms, patch)
        }
        Event::HandSettled {
            seat,
            hand,
            outcome,
            amount,
        } => {
            let hand_count = snapshot.seats[seat].hands.len().max(1);
            match outcome {
                // Losing bets are swept to the dealer's tray.
                HandOutcome::Lose | HandOutcome::Bust | HandOutcome::Surrender => chip_fly(
                    paths::bet_center(seats, seat, hand_count, hand),
                    Anchor::flat(DEALER_TRAY_POS),
                    amount.unsigned_abs(),
                    pace.settlement_ms,
                    patch,
                ),
                // Winnings are pushed from the tray to the seat.
                HandOutcome::Win | HandOutcome::Blackjack => chip_fly(
                    Anchor::flat(DEALER_TRAY_POS),
                    paths::payout_center(seats, seat, hand_count, hand),
                    amount.unsigned_abs(),
                    pace.settlement_ms,
                    patch,
                ),
                // A push: the bet stays; the dealer taps the felt.
                HandOutcome::Push => TimedStep::pause(pace.settlement_ms, patch),
            }
        }
        Event::CutCardReached => TimedStep::pause(0, patch),
        Event::PlayerArrived { .. } | Event::PlayerDeparted { .. } => {
            TimedStep::pause(pace.settlement_ms, patch)
        }
        Event::DealerChanged => TimedStep::pause(pace.reveal_ms, patch),
    }
}

/// A card flying out of the shoe.
fn card_fly(visual: Visual, to: Anchor, duration_ms: u32, patch: Patch) -> TimedStep {
    TimedStep {
        kind: StepKind::Fly {
            visual,
            from: Anchor {
                x: SHOE_POS.0,
                y: SHOE_POS.1,
                rot: SHOE_ANGLE,
            },
            to,
        },
        duration_ms,
        patch_at: 1.0,
        patch,
        sound: Some(SoundCue::CardSlide),
    }
}

/// Chips sliding between two spots. A zero-dollar slide (nothing to
/// show) degrades to a silent pause of the same length.
fn chip_fly(from: Anchor, to: Anchor, amount: u32, duration_ms: u32, patch: Patch) -> TimedStep {
    if amount == 0 {
        return TimedStep::pause(duration_ms, patch);
    }
    TimedStep {
        kind: StepKind::Fly {
            visual: Visual::Chips(amount),
            from,
            to,
        },
        duration_ms,
        patch_at: 1.0,
        patch,
        sound: Some(SoundCue::ChipClink),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackjack_core::{Action, Rules, Table};

    /// The Measured dealer's numbers, the default until #12 wires real
    /// pace through.
    fn measured() -> Pace {
        Pace {
            card_ms: 400,
            decision_ms: 700,
            reveal_ms: 700,
            settlement_ms: 500,
            between_rounds_ms: 2200,
        }
    }

    fn brisk() -> Pace {
        Pace {
            card_ms: 220,
            decision_ms: 350,
            reveal_ms: 450,
            settlement_ms: 300,
            between_rounds_ms: 1200,
        }
    }

    fn leisurely() -> Pace {
        Pace {
            card_ms: 650,
            decision_ms: 1200,
            reveal_ms: 1000,
            settlement_ms: 800,
            between_rounds_ms: 3500,
        }
    }

    /// A dealt round with bets on seats 1 and 4, from a fixed seed.
    fn dealt_round(seed: u64) -> (DisplayState, Transition) {
        let mut table = Table::from_seed(Rules::canonical(), seed);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        for seat in [1usize, 4] {
            let transition = table.apply(seat, Action::PlaceBet(25)).unwrap();
            let mut planning = display.clone();
            plan_transition(&mut planning, &transition, measured());
            display = planning;
        }
        let deal = table.apply(0, Action::Deal).unwrap();
        (display, deal)
    }

    #[test]
    fn a_card_dealt_flies_from_the_shoe_to_its_seat_frame() {
        let (mut display, deal) = dealt_round(7);
        let steps = plan_transition(&mut display, &deal, measured());
        let first_card = steps
            .iter()
            .find_map(|step| match (&step.kind, &step.patch) {
                (
                    StepKind::Fly { visual, from, to },
                    Patch::Event(Event::CardDealt { seat, card, .. }),
                ) => Some((*visual, *from, *to, *seat, *card)),
                _ => None,
            })
            .expect("a deal deals cards");
        let (visual, from, to, seat, card) = first_card;
        assert_eq!(visual, Visual::Card(card));
        assert_eq!((from.x, from.y), SHOE_POS);
        // Casino order: the first card goes to the lowest betting seat.
        assert_eq!(seat, 1);
        let expected = paths::seat_card_center(7, 1, 1, 0, 0, 1);
        assert_eq!(to, expected);
    }

    #[test]
    fn every_event_gets_its_own_nonzero_beat() {
        let (mut display, deal) = dealt_round(7);
        let steps = plan_transition(&mut display, &deal, measured());
        // One step per event plus the adopt step, no batching.
        assert_eq!(steps.len(), deal.events.len() + 1);
        for step in &steps[..steps.len() - 1] {
            assert!(
                step.duration_ms > 0 || matches!(step.patch, Patch::Event(Event::CutCardReached)),
                "instant step for {step:?}"
            );
        }
        assert!(matches!(steps.last().unwrap().patch, Patch::Adopt(_)));
        assert_eq!(steps.last().unwrap().duration_ms, 0);
    }

    #[test]
    fn card_beats_run_at_the_dealers_card_pace() {
        let (mut display, deal) = dealt_round(7);
        let steps = plan_transition(&mut display, &deal, measured());
        for step in &steps {
            if let Patch::Event(Event::CardDealt { .. } | Event::HoleCardDealt) = step.patch {
                assert_eq!(step.duration_ms, measured().card_ms);
            }
        }
    }

    #[test]
    fn a_brisk_dealer_is_felt_next_to_a_leisurely_one() {
        let (display, deal) = dealt_round(7);
        let quick = plan_transition(&mut display.clone(), &deal, brisk());
        let slow = plan_transition(&mut display.clone(), &deal, leisurely());
        let quick_ms = queue_duration_ms(&quick);
        let slow_ms = queue_duration_ms(&slow);
        assert!(
            slow_ms >= 2 * quick_ms,
            "leisurely ({slow_ms}ms) must dawdle over brisk ({quick_ms}ms)"
        );
        // And the arithmetic is a plain sum of the step durations.
        assert_eq!(
            quick_ms,
            quick.iter().map(|s| u64::from(s.duration_ms)).sum::<u64>()
        );
    }

    #[test]
    fn settlement_sweeps_losers_to_the_tray_and_pays_winners_from_it() {
        // Play a full seeded round to settlement with standing players.
        let mut table = Table::from_seed(Rules::canonical(), 11);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        let apply = |display: &mut DisplayState, transition: &Transition| {
            plan_transition(display, transition, measured())
        };
        for seat in [1usize, 4] {
            let t = table.apply(seat, Action::PlaceBet(25)).unwrap();
            apply(&mut display, &t);
        }
        let t = table.apply(0, Action::Deal).unwrap();
        apply(&mut display, &t);
        let mut settle_steps = Vec::new();
        for _ in 0..32 {
            let snapshot = table.snapshot();
            if snapshot.phase == blackjack_core::Phase::RoundOver {
                break;
            }
            let seat = snapshot.active.expect("mid-round has an active seat").seat;
            let t = table.apply(seat, Action::Stand).unwrap();
            settle_steps.extend(apply(&mut display, &t));
        }
        let mut saw_settlement = false;
        for step in &settle_steps {
            let Patch::Event(Event::HandSettled {
                outcome, amount, ..
            }) = step.patch
            else {
                continue;
            };
            saw_settlement = true;
            match outcome {
                HandOutcome::Lose | HandOutcome::Bust | HandOutcome::Surrender => {
                    let StepKind::Fly { visual, to, .. } = step.kind else {
                        panic!("a lost bet must fly");
                    };
                    assert_eq!(visual, Visual::Chips(amount.unsigned_abs()));
                    assert_eq!((to.x, to.y), DEALER_TRAY_POS);
                }
                HandOutcome::Win | HandOutcome::Blackjack => {
                    let StepKind::Fly { visual, from, .. } = step.kind else {
                        panic!("a payout must fly");
                    };
                    assert_eq!(visual, Visual::Chips(amount.unsigned_abs()));
                    assert_eq!((from.x, from.y), DEALER_TRAY_POS);
                }
                HandOutcome::Push => {
                    assert!(matches!(step.kind, StepKind::Pause));
                }
            }
            assert_eq!(step.sound.is_some(), outcome != HandOutcome::Push);
        }
        assert!(saw_settlement, "the round must settle");
    }

    #[test]
    fn the_next_round_sweep_and_shuffle_ritual() {
        // Force the cut card by exhausting a single-deck shoe.
        let rules = Rules {
            decks: 1,
            penetration: 0.05,
            ..Rules::canonical()
        };
        let mut table = Table::from_seed(rules, 3);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        let run =
            |display: &mut DisplayState, t: &Transition| plan_transition(display, t, measured());
        let t = table.apply(2, Action::PlaceBet(25)).unwrap();
        run(&mut display, &t);
        let t = table.apply(0, Action::Deal).unwrap();
        run(&mut display, &t);
        for _ in 0..16 {
            let snapshot = table.snapshot();
            if snapshot.phase == blackjack_core::Phase::RoundOver {
                break;
            }
            let seat = snapshot.active.unwrap().seat;
            let t = table.apply(seat, Action::Stand).unwrap();
            run(&mut display, &t);
        }
        let next = table.apply(0, Action::NextRound).unwrap();
        assert!(
            next.events.contains(&Event::ShoeShuffled),
            "the tiny shoe forces a reshuffle"
        );
        let steps = run(&mut display, &next);
        // The silent felt clear becomes a sweep to the discard tray...
        assert!(matches!(
            (&steps[0].kind, &steps[0].patch),
            (StepKind::Fly { to, .. }, Patch::ClearFelt) if (to.x, to.y) == DISCARD_POS
        ));
        // ...followed by a breather, the riffle at the shoe, and the
        // burn card sliding to the discard tray.
        let shuffle = steps
            .iter()
            .find(|s| matches!(s.patch, Patch::Event(Event::ShoeShuffled)))
            .unwrap();
        assert!(matches!(shuffle.kind, StepKind::Shimmer));
        assert_eq!(shuffle.sound, Some(SoundCue::Riffle));
        assert!(shuffle.sound.unwrap().at_step_start());
        let burn = steps
            .iter()
            .find(|s| matches!(s.patch, Patch::Event(Event::CardBurned)))
            .unwrap();
        assert!(matches!(
            burn.kind,
            StepKind::Fly { visual: Visual::Back, to, .. } if (to.x, to.y) == DISCARD_POS
        ));
        // The display ends exactly at the authoritative snapshot.
        assert_eq!(display.snapshot().unwrap(), &next.snapshot);
    }

    #[test]
    fn the_hole_card_reveal_flips_at_the_turn() {
        let (mut display, deal) = dealt_round(7);
        plan_transition(&mut display, &deal, measured());
        // Synthesize a reveal against the dealt display state.
        let hole = Card::new(blackjack_core::Rank::Nine, blackjack_core::Suit::Clubs);
        let reveal = Transition {
            snapshot: {
                let mut s = deal.snapshot.clone();
                s.dealer.hole_card = Some(hole);
                s
            },
            events: vec![Event::HoleCardRevealed { card: hole }],
        };
        let steps = plan_transition(&mut display.clone(), &reveal, measured());
        let flip = &steps[0];
        assert!(matches!(flip.kind, StepKind::Flip { card, .. } if card == hole));
        assert_eq!(flip.duration_ms, measured().reveal_ms);
        assert_eq!(flip.patch_at, 0.5);
    }

    #[test]
    fn bets_slide_in_from_the_rail_with_a_clink() {
        let mut table = Table::from_seed(Rules::canonical(), 5);
        let mut display = DisplayState::new();
        display.adopt(table.snapshot());
        let t = table.apply(4, Action::PlaceBet(60)).unwrap();
        let steps = plan_transition(&mut display, &t, measured());
        let StepKind::Fly { visual, from, to } = steps[0].kind else {
            panic!("a bet slides");
        };
        assert_eq!(visual, Visual::Chips(60));
        assert_eq!(steps[0].sound, Some(SoundCue::ChipClink));
        let place = crate::scene::geometry::seat_places(7)[4];
        assert!((to.x - place.x).abs() < 1e-9 && (to.y - place.y).abs() < 1e-9);
        // Chips come from the player's side of the circle.
        assert!(from.y > to.y);
    }
}
