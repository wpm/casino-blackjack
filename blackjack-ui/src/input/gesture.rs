//! Pure gesture recognition: classify pointer strokes into intents,
//! gated on the snapshot's legal actions.
//!
//! The safety invariant of the whole input layer lives here (and in
//! [`resolve`]): a stroke may resolve to an intent only when the
//! corresponding [`ActionKind`] is in the current snapshot's
//! `legal_actions` **and** the gesture belongs to the human seat's turn.
//! A misread is a no-op, never a wrong action; anything ambiguous
//! resolves to [`StrokeOutcome::Nothing`].
//!
//! # Gesture vocabulary
//!
//! | Gesture | Real-table signal | Recognition |
//! |---|---|---|
//! | Drag rack chips into the circle | placing a bet | rack grab dropped in [`DropZone::BetCircle`] (stages, no engine call) |
//! | Drag staged chips out of the circle | pulling a bet back | circle grab dropped outside [`BET_ZONE_R`](super::geometry::BET_ZONE_R) |
//! | Tap the circle (or press Enter) | "money plays" | posts the staged bet ([`Intent::ConfirmBet`]) |
//! | Tap the felt behind the cards | tapping for a card | [`Intent::Hit`] |
//! | Horizontal wave across the cards | waving off | [`Intent::Stand`] |
//! | Drag chips beside the circle | matching stack beside the bet | [`Intent::Double`] |
//! | Drag chips directly behind the circle | matching stack behind the pair | [`Intent::Split`] |
//! | Horizontal line behind the circle, no chips | drawing the surrender line | [`Intent::Surrender`] |
//! | Drag chips onto the insurance band | chips on the line | [`Intent::TakeInsurance`] |
//! | Tap the felt during the offer (or N) | waving it off | [`Intent::DeclineInsurance`] |
//! | Tap the felt when the round is over (or Enter) | nodding for the next hand | [`Intent::NextRound`] |
//! | Drag the rack off the felt's bottom edge | racking up and leaving | [`Intent::WalkAway`] |

use blackjack_core::{Action, ActionKind, ChipStack, Denomination, Phase, Rules, Snapshot};

use super::geometry::{
    BET_ZONE_R, DropZone, FELT_EXIT_Y, Point, Stroke, TAP_MAX, WALK_MIN_DY, WAVE_MAX_DY,
    WAVE_MIN_DX, drop_zone, in_behind_band, in_hand_zone, in_insurance_band, in_rack_region,
    on_felt, rack_pile_at, seat_local,
};
use super::staging::stage_ready;
use crate::scene::geometry::{SeatPlace, seat_places};

/// The seat the UI treats as the human's: the center seat. Issue #12
/// replaces this with the session's configured seat.
pub fn human_seat(snapshot: &Snapshot) -> usize {
    snapshot.seats.len() / 2
}

/// What the pointer picked up at the start of a drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    /// A chip of this denomination from the human's rack.
    Rack(Denomination),
    /// The top chip of the staged bet, lifted out of the circle.
    Bet(Denomination),
}

/// A high-level interaction the input layer asks the app to perform.
///
/// Intents are already legality-gated when emitted, but carry no engine
/// coupling: the app maps them to [`Action`]s through [`resolve`] (a
/// second, independent gate) or handles them itself (`WalkAway`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Post the staged bet of this many dollars (and start the round).
    ConfirmBet(u32),
    /// Take one more card.
    Hit,
    /// Stand.
    Stand,
    /// Double down.
    Double,
    /// Split the pair.
    Split,
    /// Surrender the hand.
    Surrender,
    /// Buy insurance.
    TakeInsurance,
    /// Decline insurance.
    DeclineInsurance,
    /// Clear the finished round.
    NextRound,
    /// Leave the table (cash-out arrives with #12).
    WalkAway,
}

/// What a completed stroke amounts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeOutcome {
    /// An intent to hand to the app.
    Intent(Intent),
    /// Add one chip of this denomination to the staged bet (UI-local).
    StageAdd(Denomination),
    /// Remove one chip of this denomination from the staged bet
    /// (UI-local).
    StageRemove(Denomination),
    /// Ambiguous or dead input: do nothing.
    Nothing,
}

/// Everything gesture recognition needs to know about the current
/// snapshot, extracted once per event.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureCtx {
    /// The current phase.
    pub phase: Phase,
    /// The snapshot's authoritative legal-action set.
    pub legal: Vec<ActionKind>,
    /// The human's seat index.
    pub human_seat: usize,
    /// Whether the engine is waiting on the human seat specifically.
    pub human_active: bool,
    /// Table minimum bet.
    pub min_bet: u32,
    /// Table maximum bet.
    pub max_bet: u32,
    /// The human seat's placement on the felt.
    pub place: SeatPlace,
}

impl GestureCtx {
    /// Extract the gesture context for `human_seat` from a snapshot.
    pub fn from_snapshot(snapshot: &Snapshot, human_seat: usize) -> GestureCtx {
        GestureCtx {
            phase: snapshot.phase,
            legal: snapshot.legal_actions.clone(),
            human_seat,
            human_active: snapshot.active.is_some_and(|a| a.seat == human_seat),
            min_bet: snapshot.rules.min_bet,
            max_bet: snapshot.rules.max_bet,
            place: seat_places(snapshot.seats.len())
                .get(human_seat)
                .copied()
                .unwrap_or(SeatPlace {
                    x: 800.0,
                    y: 790.0,
                    angle: 0.0,
                    tilt: 0.0,
                }),
        }
    }

    /// Whether the engine will accept this action kind right now.
    pub fn allows(&self, kind: ActionKind) -> bool {
        self.legal.contains(&kind)
    }

    /// Whether a hand-signal action (`kind`) is available to the human:
    /// their turn, and the action legal.
    fn hand_signal(&self, kind: ActionKind) -> bool {
        self.phase == Phase::PlayerTurn && self.human_active && self.allows(kind)
    }

    /// Whether the insurance decision (`kind`) rests with the human.
    fn insurance_signal(&self, kind: ActionKind) -> bool {
        self.phase == Phase::InsuranceOffer && self.human_active && self.allows(kind)
    }

    /// Whether the human may stage and post bets right now.
    fn betting_open(&self) -> bool {
        self.phase == Phase::Betting && self.allows(ActionKind::PlaceBet)
    }

    /// Whether the table is between rounds (walk-away is meaningful).
    fn between_rounds(&self) -> bool {
        matches!(self.phase, Phase::Betting | Phase::RoundOver)
    }
}

/// What (if anything) the pointer picks up when it goes down at `p`.
///
/// Rack piles are grabbable whenever chips could mean something: a
/// staged bet, a double/split, insurance, or a walk-away. The staged
/// bet's top chip is grabbable out of the circle while betting is open.
pub fn grab_at(ctx: &GestureCtx, rack: &ChipStack, staged: &ChipStack, p: Point) -> Option<Grab> {
    let chips_mean_something = ctx.betting_open()
        || ctx.hand_signal(ActionKind::Double)
        || ctx.hand_signal(ActionKind::Split)
        || ctx.insurance_signal(ActionKind::TakeInsurance)
        || ctx.between_rounds();
    if chips_mean_something && let Some(denomination) = rack_pile_at(rack, p) {
        return Some(Grab::Rack(denomination));
    }
    if ctx.betting_open()
        && seat_local(ctx.place, p).distance(Point::new(0.0, 0.0)) <= BET_ZONE_R
        && let Some(top) = super::staging::stage_top(staged)
    {
        return Some(Grab::Bet(top));
    }
    None
}

/// Classify a completed stroke.
///
/// `grab` is what the pointer picked up at `stroke.start` (from
/// [`grab_at`]); `staged_total` is the staged bet's value in dollars.
/// Every arm is gated on phase, turn, and the legal-action set; the
/// fall-through is always [`StrokeOutcome::Nothing`].
pub fn classify_stroke(
    ctx: &GestureCtx,
    grab: Option<Grab>,
    stroke: Stroke,
    staged_total: u32,
) -> StrokeOutcome {
    let Stroke { start, end } = stroke;

    // Walk away: a drag from the rack off the felt's bottom edge,
    // between rounds. Checked first — racking up trumps everything.
    if ctx.between_rounds()
        && in_rack_region(start)
        && end.y >= FELT_EXIT_Y
        && end.y - start.y >= WALK_MIN_DY
    {
        return StrokeOutcome::Intent(Intent::WalkAway);
    }

    match grab {
        Some(Grab::Rack(denomination)) => match drop_zone(ctx.place, end) {
            DropZone::BetCircle if ctx.betting_open() => StrokeOutcome::StageAdd(denomination),
            DropZone::DoubleSide if ctx.hand_signal(ActionKind::Double) => {
                StrokeOutcome::Intent(Intent::Double)
            }
            DropZone::SplitBehind if ctx.hand_signal(ActionKind::Split) => {
                StrokeOutcome::Intent(Intent::Split)
            }
            DropZone::InsuranceBand if ctx.insurance_signal(ActionKind::TakeInsurance) => {
                StrokeOutcome::Intent(Intent::TakeInsurance)
            }
            _ => StrokeOutcome::Nothing,
        },
        Some(Grab::Bet(denomination)) => {
            // Dragging staged chips out of the circle reduces the bet;
            // dropping them back inside returns them.
            if ctx.betting_open()
                && seat_local(ctx.place, end).distance(Point::new(0.0, 0.0)) > BET_ZONE_R
            {
                StrokeOutcome::StageRemove(denomination)
            } else {
                StrokeOutcome::Nothing
            }
        }
        None => classify_bare_stroke(ctx, start, end, staged_total),
    }
}

/// Classify a stroke made with nothing in hand: taps and waves.
fn classify_bare_stroke(
    ctx: &GestureCtx,
    start: Point,
    end: Point,
    staged_total: u32,
) -> StrokeOutcome {
    let local_start = seat_local(ctx.place, start);
    let local_end = seat_local(ctx.place, end);

    if start.distance(end) <= TAP_MAX {
        // A tap. Meaning depends on where and when.
        let tap = end;
        let local = local_end;
        return match ctx.phase {
            // Decline insurance: tap the open felt (documented mapping;
            // N on the keyboard). Excludes the insurance band itself so
            // a slip while aiming chips at the line can never decline.
            Phase::InsuranceOffer
                if ctx.insurance_signal(ActionKind::DeclineInsurance)
                    && on_felt(tap)
                    && !in_insurance_band(tap) =>
            {
                StrokeOutcome::Intent(Intent::DeclineInsurance)
            }
            // Post the staged bet: tap the betting circle.
            Phase::Betting
                if ctx.betting_open()
                    && local.distance(Point::new(0.0, 0.0)) <= BET_ZONE_R
                    && stage_ready(staged_total, ctx.min_bet, ctx.max_bet) =>
            {
                StrokeOutcome::Intent(Intent::ConfirmBet(staged_total))
            }
            // Hit: tap the felt behind the cards.
            Phase::PlayerTurn if ctx.hand_signal(ActionKind::Hit) && in_hand_zone(local) => {
                StrokeOutcome::Intent(Intent::Hit)
            }
            // Next round: tap the felt once everything is settled.
            Phase::RoundOver if ctx.allows(ActionKind::NextRound) && on_felt(tap) => {
                StrokeOutcome::Intent(Intent::NextRound)
            }
            _ => StrokeOutcome::Nothing,
        };
    }

    let dx = (end.x - start.x).abs();
    let dy = (end.y - start.y).abs();
    if dx >= WAVE_MIN_DX && dy <= WAVE_MAX_DY {
        // A horizontal wave. Across the cards: stand. Behind the bet:
        // the surrender line. Both endpoints must sit in the zone, so a
        // stroke that wanders between zones stays ambiguous.
        if ctx.hand_signal(ActionKind::Stand)
            && in_hand_zone(local_start)
            && in_hand_zone(local_end)
        {
            return StrokeOutcome::Intent(Intent::Stand);
        }
        if ctx.hand_signal(ActionKind::Surrender)
            && in_behind_band(local_start)
            && in_behind_band(local_end)
        {
            return StrokeOutcome::Intent(Intent::Surrender);
        }
    }

    StrokeOutcome::Nothing
}

/// Map an intent to the engine action it stands for, re-checking
/// legality against the same context — the second, independent gate
/// behind the recognizers. `None` means "do not submit anything"
/// (including [`Intent::WalkAway`], which is not an engine action).
///
/// If the engine ever rejects an action produced here, that is a bug in
/// this module: log it loudly.
pub fn resolve(ctx: &GestureCtx, intent: Intent) -> Option<Action> {
    match intent {
        Intent::ConfirmBet(amount) => (ctx.betting_open()
            && stage_ready(amount, ctx.min_bet, ctx.max_bet))
        .then_some(Action::PlaceBet(amount)),
        Intent::Hit => ctx.hand_signal(ActionKind::Hit).then_some(Action::Hit),
        Intent::Stand => ctx.hand_signal(ActionKind::Stand).then_some(Action::Stand),
        Intent::Double => ctx
            .hand_signal(ActionKind::Double)
            .then_some(Action::Double),
        Intent::Split => ctx.hand_signal(ActionKind::Split).then_some(Action::Split),
        Intent::Surrender => ctx
            .hand_signal(ActionKind::Surrender)
            .then_some(Action::Surrender),
        Intent::TakeInsurance => ctx
            .insurance_signal(ActionKind::TakeInsurance)
            .then_some(Action::TakeInsurance),
        Intent::DeclineInsurance => ctx
            .insurance_signal(ActionKind::DeclineInsurance)
            .then_some(Action::DeclineInsurance),
        Intent::NextRound => (ctx.phase == Phase::RoundOver && ctx.allows(ActionKind::NextRound))
            .then_some(Action::NextRound),
        Intent::WalkAway => None,
    }
}

/// A context for tests and tools: a synthetic snapshot-free builder.
///
/// Production code always goes through [`GestureCtx::from_snapshot`].
pub fn test_ctx(phase: Phase, legal: &[ActionKind], human_active: bool) -> GestureCtx {
    let rules = Rules::canonical();
    GestureCtx {
        phase,
        legal: legal.to_vec(),
        human_seat: 3,
        human_active,
        min_bet: rules.min_bet,
        max_bet: rules.max_bet,
        place: seat_places(7)[3],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::geometry::{DOUBLE_ZONE_X, RACK_X, RACK_Y, SPLIT_ZONE_Y};

    /// A stroke between two seat-frame offsets of the center seat
    /// (tilt 0, so local offsets are global offsets).
    fn stroke(ctx: &GestureCtx, from: (f64, f64), to: (f64, f64)) -> Stroke {
        Stroke {
            start: Point::new(ctx.place.x + from.0, ctx.place.y + from.1),
            end: Point::new(ctx.place.x + to.0, ctx.place.y + to.1),
        }
    }

    fn tap(ctx: &GestureCtx, at: (f64, f64)) -> Stroke {
        stroke(ctx, at, at)
    }

    #[test]
    fn tap_behind_the_cards_hits_only_when_legal() {
        let legal = [ActionKind::Hit, ActionKind::Stand];
        let ctx = test_ctx(Phase::PlayerTurn, &legal, true);
        let hit_tap = tap(&ctx, (10.0, -220.0));
        assert_eq!(
            classify_stroke(&ctx, None, hit_tap, 0),
            StrokeOutcome::Intent(Intent::Hit)
        );
        // Hit not in the legal set: nothing.
        let ctx = test_ctx(Phase::PlayerTurn, &[ActionKind::Stand], true);
        assert_eq!(
            classify_stroke(&ctx, None, hit_tap, 0),
            StrokeOutcome::Nothing
        );
        // Not the human's turn: nothing.
        let ctx = test_ctx(Phase::PlayerTurn, &legal, false);
        assert_eq!(
            classify_stroke(&ctx, None, hit_tap, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn wave_across_the_cards_stands_and_never_hits() {
        let legal = [ActionKind::Hit, ActionKind::Stand];
        let ctx = test_ctx(Phase::PlayerTurn, &legal, true);
        let wave = stroke(&ctx, (-70.0, -230.0), (70.0, -210.0));
        assert_eq!(
            classify_stroke(&ctx, None, wave, 0),
            StrokeOutcome::Intent(Intent::Stand)
        );
        // Too short to be a wave, too long to be a tap: ambiguous.
        let wiggle = stroke(&ctx, (-20.0, -230.0), (30.0, -225.0));
        assert_eq!(
            classify_stroke(&ctx, None, wiggle, 0),
            StrokeOutcome::Nothing
        );
        // Too diagonal: ambiguous.
        let diagonal = stroke(&ctx, (-70.0, -300.0), (70.0, -180.0));
        assert_eq!(
            classify_stroke(&ctx, None, diagonal, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn surrender_line_is_a_wave_behind_the_bet_without_chips() {
        let legal = [ActionKind::Hit, ActionKind::Stand, ActionKind::Surrender];
        let ctx = test_ctx(Phase::PlayerTurn, &legal, true);
        let line = stroke(&ctx, (-60.0, 120.0), (60.0, 115.0));
        assert_eq!(
            classify_stroke(&ctx, None, line, 0),
            StrokeOutcome::Intent(Intent::Surrender)
        );
        // The same line with surrender not legal: nothing.
        let ctx_no = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand],
            true,
        );
        assert_eq!(
            classify_stroke(&ctx_no, None, line, 0),
            StrokeOutcome::Nothing
        );
        // A wave that starts behind the bet and ends across the cards
        // is ambiguous: nothing.
        let wander = stroke(&ctx, (-60.0, 120.0), (60.0, -220.0));
        assert_eq!(
            classify_stroke(&ctx, None, wander, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn chip_drops_classify_by_zone_and_legality() {
        let legal = [
            ActionKind::Hit,
            ActionKind::Stand,
            ActionKind::Double,
            ActionKind::Split,
        ];
        let ctx = test_ctx(Phase::PlayerTurn, &legal, true);
        let grab = Some(Grab::Rack(Denomination::TwentyFive));
        let beside = stroke(&ctx, (0.0, 300.0), (DOUBLE_ZONE_X, 0.0));
        assert_eq!(
            classify_stroke(&ctx, grab, beside, 0),
            StrokeOutcome::Intent(Intent::Double)
        );
        let behind = stroke(&ctx, (0.0, 300.0), (0.0, SPLIT_ZONE_Y));
        assert_eq!(
            classify_stroke(&ctx, grab, behind, 0),
            StrokeOutcome::Intent(Intent::Split)
        );
        // No split in the legal set (not a pair): the behind drop dies.
        let ctx_no_split = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand, ActionKind::Double],
            true,
        );
        assert_eq!(
            classify_stroke(&ctx_no_split, grab, behind, 0),
            StrokeOutcome::Nothing
        );
        // A drop on open felt returns the chips.
        let nowhere = stroke(&ctx, (0.0, 300.0), (-350.0, -50.0));
        assert_eq!(
            classify_stroke(&ctx, grab, nowhere, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn betting_drags_stage_and_unstage() {
        let ctx = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        let grab = Some(Grab::Rack(Denomination::Five));
        let into_circle = stroke(&ctx, (0.0, 300.0), (12.0, -8.0));
        assert_eq!(
            classify_stroke(&ctx, grab, into_circle, 0),
            StrokeOutcome::StageAdd(Denomination::Five)
        );
        // Chips into the circle outside the betting phase: nothing.
        let ctx_play = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand],
            true,
        );
        assert_eq!(
            classify_stroke(&ctx_play, grab, into_circle, 0),
            StrokeOutcome::Nothing
        );
        // Dragging the staged top chip out reduces.
        let out = stroke(&ctx, (0.0, 0.0), (150.0, 60.0));
        assert_eq!(
            classify_stroke(&ctx, Some(Grab::Bet(Denomination::Five)), out, 15),
            StrokeOutcome::StageRemove(Denomination::Five)
        );
        // Dropping it back inside the circle changes nothing.
        let back = stroke(&ctx, (0.0, 0.0), (20.0, 10.0));
        assert_eq!(
            classify_stroke(&ctx, Some(Grab::Bet(Denomination::Five)), back, 15),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn circle_tap_posts_only_a_ready_bet() {
        let ctx = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        let circle_tap = tap(&ctx, (5.0, 5.0));
        assert_eq!(
            classify_stroke(&ctx, None, circle_tap, 25),
            StrokeOutcome::Intent(Intent::ConfirmBet(25))
        );
        // Below the table minimum: not postable.
        assert_eq!(
            classify_stroke(&ctx, None, circle_tap, 5),
            StrokeOutcome::Nothing
        );
        // Nothing staged: nothing to post.
        assert_eq!(
            classify_stroke(&ctx, None, circle_tap, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn insurance_drop_and_decline_tap() {
        let legal = [ActionKind::TakeInsurance, ActionKind::DeclineInsurance];
        let ctx = test_ctx(Phase::InsuranceOffer, &legal, true);
        let grab = Some(Grab::Rack(Denomination::Five));
        // Drop on the band along the seat's radial line: take.
        let band_y = -(690.0 - 407.0); // seat ring 690, band middle ~407
        let onto_band = stroke(&ctx, (0.0, 300.0), (0.0, band_y));
        assert_eq!(
            classify_stroke(&ctx, grab, onto_band, 0),
            StrokeOutcome::Intent(Intent::TakeInsurance)
        );
        // Tap the open felt: decline.
        let felt_tap = tap(&ctx, (-250.0, -60.0));
        assert_eq!(
            classify_stroke(&ctx, None, felt_tap, 0),
            StrokeOutcome::Intent(Intent::DeclineInsurance)
        );
        // Tap ON the band is ambiguous (aiming chips), never a decline.
        let band_tap = tap(&ctx, (0.0, band_y));
        assert_eq!(
            classify_stroke(&ctx, None, band_tap, 0),
            StrokeOutcome::Nothing
        );
        // Someone else's insurance decision: the human's taps do nothing.
        let ctx_not_mine = test_ctx(Phase::InsuranceOffer, &legal, false);
        assert_eq!(
            classify_stroke(&ctx_not_mine, None, felt_tap, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn round_over_tap_advances_and_walk_away_needs_the_edge() {
        let ctx = test_ctx(Phase::RoundOver, &[ActionKind::NextRound], false);
        let felt_tap = tap(&ctx, (0.0, -300.0));
        assert_eq!(
            classify_stroke(&ctx, None, felt_tap, 0),
            StrokeOutcome::Intent(Intent::NextRound)
        );
        // Rack dragged off the bottom edge: walk away.
        let leave = Stroke {
            start: Point::new(RACK_X, RACK_Y),
            end: Point::new(RACK_X + 10.0, 992.0),
        };
        assert_eq!(
            classify_stroke(&ctx, Some(Grab::Rack(Denomination::Five)), leave, 0),
            StrokeOutcome::Intent(Intent::WalkAway)
        );
        // A rack drag that stops short of the edge: nothing.
        let short = Stroke {
            start: Point::new(RACK_X, RACK_Y),
            end: Point::new(RACK_X, 970.0),
        };
        assert_eq!(
            classify_stroke(&ctx, Some(Grab::Rack(Denomination::Five)), short, 0),
            StrokeOutcome::Nothing
        );
        // Mid-hand there is no walking away.
        let ctx_play = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand],
            true,
        );
        assert_eq!(
            classify_stroke(&ctx_play, Some(Grab::Rack(Denomination::Five)), leave, 0),
            StrokeOutcome::Nothing
        );
    }

    #[test]
    fn grabbing_finds_rack_piles_and_the_staged_top_chip() {
        let ctx = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::Five, 8);
        let mut staged = ChipStack::new();
        staged.add_chips(Denomination::Five, 2);
        staged.add_chips(Denomination::TwentyFive, 1);
        // The rack pile.
        assert_eq!(
            grab_at(&ctx, &rack, &staged, Point::new(RACK_X, RACK_Y)),
            Some(Grab::Rack(Denomination::Five))
        );
        // The staged bet's top chip is the smallest denomination.
        let circle = Point::new(ctx.place.x, ctx.place.y);
        assert_eq!(
            grab_at(&ctx, &rack, &staged, circle),
            Some(Grab::Bet(Denomination::Five))
        );
        // An empty circle grabs nothing.
        assert_eq!(grab_at(&ctx, &rack, &ChipStack::new(), circle), None);
        // Open felt grabs nothing.
        assert_eq!(
            grab_at(&ctx, &rack, &staged, Point::new(400.0, 500.0)),
            None
        );
    }

    #[test]
    fn resolve_regates_every_intent() {
        let ctx = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand],
            true,
        );
        assert_eq!(resolve(&ctx, Intent::Hit), Some(Action::Hit));
        assert_eq!(resolve(&ctx, Intent::Stand), Some(Action::Stand));
        // Intents whose actions are not legal resolve to nothing, even
        // if a recognizer bug were to emit them.
        assert_eq!(resolve(&ctx, Intent::Double), None);
        assert_eq!(resolve(&ctx, Intent::Split), None);
        assert_eq!(resolve(&ctx, Intent::Surrender), None);
        assert_eq!(resolve(&ctx, Intent::ConfirmBet(50)), None);
        assert_eq!(resolve(&ctx, Intent::NextRound), None);
        // WalkAway is never an engine action.
        let ctx_bet = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        assert_eq!(resolve(&ctx_bet, Intent::WalkAway), None);
        // Bets resolve only in range.
        assert_eq!(
            resolve(&ctx_bet, Intent::ConfirmBet(50)),
            Some(Action::PlaceBet(50))
        );
        assert_eq!(resolve(&ctx_bet, Intent::ConfirmBet(5)), None);
        assert_eq!(resolve(&ctx_bet, Intent::ConfirmBet(10_000)), None);
    }

    #[test]
    fn human_seat_is_the_center_seat() {
        let snapshot = blackjack_core::Table::from_seed(Rules::canonical(), 0).snapshot();
        assert_eq!(human_seat(&snapshot), 3);
        let ctx = GestureCtx::from_snapshot(&snapshot, 3);
        assert_eq!(ctx.phase, Phase::Betting);
        assert_eq!(ctx.min_bet, 10);
        assert_eq!(ctx.max_bet, 500);
        assert!(ctx.allows(ActionKind::PlaceBet));
        assert!(!ctx.human_active);
        let places = seat_places(7);
        assert_eq!(ctx.place, places[3]);
    }
}
