//! The thin Leptos layer over the pure recognizers: one transparent SVG
//! stacked on the table scene that owns every pointer and keyboard
//! event, plus the input layer's own felt furniture — the human's chip
//! rack, the staged bet, drag ghosts, and drop-zone hints.
//!
//! All decisions are delegated to [`super::gesture`] and
//! [`super::keyboard`]; this module only converts DOM coordinates to
//! scene coordinates and re-renders signals. High-level [`Intent`]s go
//! out through the `on_intent` callback — the overlay never talks to a
//! backend and never constructs an engine [`Action`](blackjack_core::Action).
//!
//! The rack is the session's real bankroll (the `rack` signal, fed from
//! every [`SessionView`](blackjack_protocol::SessionView)): staging is
//! limited to chips actually in it, and the rendered piles deplete as
//! chips are staged ([`rack_after_staging`]).

use blackjack_core::{ActionKind, ChipStack, Phase, Snapshot};
use leptos::prelude::*;
use leptos::wasm_bindgen::JsCast;

use super::geometry::{
    BET_ZONE_R, DOUBLE_ZONE_X, Point, RACK_CAP, RACK_X, RACK_Y, SIDE_ZONE_R, SPLIT_ZONE_Y, Stroke,
    scene_point,
};
use super::gesture::{
    GestureCtx, Grab, Intent, StrokeOutcome, classify_stroke, grab_at, human_seat,
};
use super::keyboard::{KeyCommand, command_intent, key_command};
use super::staging::{rack_after_staging, stage_add, stage_nudge, stage_remove, stage_units};
use crate::scene::geometry::{
    INSURANCE_R_INNER, INSURANCE_R_OUTER, SeatPlace, VIEW_H, VIEW_W, arc_path,
};
use crate::scene::{ChipStackView, ChipView, RackView};

/// How long staged chips must rest in the circle before the bet posts
/// on its own — long enough to stack more chips unhurried, short
/// enough that the deal feels like the dealer's doing.
const SETTLE_POST_MS: u64 = 1500;

/// A drag in progress, in scene coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DragState {
    /// What the pointer picked up, if anything.
    grab: Option<Grab>,
    /// Where the pointer went down.
    start: Point,
    /// Where the pointer is now.
    pos: Point,
}

/// The scene-coordinate position of a pointer event over the overlay,
/// honoring the viewBox's `meet` scaling.
fn event_scene_point(
    ev: &leptos::web_sys::PointerEvent,
) -> Option<(leptos::web_sys::Element, Point)> {
    let element = ev
        .current_target()?
        .dyn_into::<leptos::web_sys::Element>()
        .ok()?;
    let rect = element.get_bounding_client_rect();
    let p = scene_point(
        rect.width(),
        rect.height(),
        f64::from(ev.client_x()) - rect.left(),
        f64::from(ev.client_y()) - rect.top(),
    );
    Some((element, p))
}

/// The gesture context for the current snapshot, if a session is open.
fn current_ctx(snapshot: RwSignal<Option<Snapshot>>) -> Option<GestureCtx> {
    let snap = snapshot.get_untracked()?;
    let seat = human_seat(&snap);
    Some(GestureCtx::from_snapshot(&snap, seat))
}

/// A faint gold outline for a drop zone in the human's seat frame.
#[component]
fn ZoneHint(place: SeatPlace, cx: f64, cy: f64) -> impl IntoView {
    view! {
        <g transform=format!(
            "translate({:.2} {:.2}) rotate({:.2})",
            place.x,
            place.y,
            place.tilt
        )>
            <circle
                cx=cx
                cy=cy
                r=SIDE_ZONE_R
                fill="rgba(255, 217, 122, 0.06)"
                stroke="#ffd97a"
                stroke-width="2"
                stroke-dasharray="7 9"
                opacity="0.5"
            ></circle>
        </g>
    }
}

/// The gesture surface: a transparent SVG over the whole scene.
///
/// `snapshot` is read (never written); every recognized gesture or key
/// press leaves through `on_intent` as a legality-gated [`Intent`].
#[component]
pub fn InputLayer(
    /// The app's snapshot signal (read-only here).
    snapshot: RwSignal<Option<Snapshot>>,
    /// The human's real rack from the session view (read-only here).
    rack: RwSignal<ChipStack>,
    /// Where recognized intents go.
    on_intent: UnsyncCallback<Intent>,
) -> impl IntoView {
    let staged = RwSignal::new(ChipStack::new());
    let drag = RwSignal::new(Option::<DragState>::None);
    // What the rail shows and offers for grabbing: the rack minus the
    // chips already staged in the circle.
    let visible_rack = Memo::new(move |_| rack_after_staging(&rack.get(), &staged.get()));

    // The stage lives only while betting is open and unposted: once the
    // human's bet is on the felt (or the phase moves on), clear it.
    Effect::new(move |_| {
        if let Some(snap) = snapshot.get() {
            let posted = snap.seats[human_seat(&snap)].bet.is_some();
            if snap.phase != Phase::Betting || posted {
                staged.update(|stack| {
                    if !stack.is_empty() {
                        *stack = ChipStack::new();
                    }
                });
            }
        }
    });

    // The dealer takes a settled bet: once staged chips have rested in
    // the circle for a beat — no drag in flight, nothing added or
    // removed since — the bet posts on its own, exactly as the deal
    // begins when chips stop moving at a real table. Tapping the
    // circle or pressing Enter still hurries it. Each staging change
    // arms a fresh timer; the generation counter retires stale ones.
    let settle_generation = StoredValue::new(0u64);
    let settle_post = move || {
        let generation = settle_generation.with_value(|g| g + 1);
        settle_generation.set_value(generation);
        set_timeout(
            move || {
                if settle_generation.get_value() != generation || drag.get_untracked().is_some() {
                    return;
                }
                let staged_total = staged.get_untracked().total();
                let Some(ctx) = current_ctx(snapshot) else {
                    return;
                };
                if ctx.settle_ready(staged_total) {
                    on_intent.run(Intent::ConfirmBet(staged_total));
                }
            },
            std::time::Duration::from_millis(SETTLE_POST_MS),
        );
    };
    // Picking anything up retires any pending settle without arming a
    // new one — chips in hand are chips still moving.
    let settle_cancel = move || {
        settle_generation.update_value(|g| *g += 1);
    };

    let on_pointer_down = move |ev: leptos::web_sys::PointerEvent| {
        let Some((element, p)) = event_scene_point(&ev) else {
            return;
        };
        let Some(ctx) = current_ctx(snapshot) else {
            return;
        };
        settle_cancel();
        let grab = grab_at(
            &ctx,
            &visible_rack.get_untracked(),
            &staged.get_untracked(),
            p,
        );
        let _ = element.set_pointer_capture(ev.pointer_id());
        drag.set(Some(DragState {
            grab,
            start: p,
            pos: p,
        }));
    };

    let on_pointer_move = move |ev: leptos::web_sys::PointerEvent| {
        if drag.get_untracked().is_none() {
            return;
        }
        if let Some((_, p)) = event_scene_point(&ev) {
            drag.update(|state| {
                if let Some(state) = state {
                    state.pos = p;
                }
            });
        }
    };

    let on_pointer_up = move |ev: leptos::web_sys::PointerEvent| {
        let Some(state) = drag.get_untracked() else {
            return;
        };
        drag.set(None);
        let Some((_, end)) = event_scene_point(&ev) else {
            return;
        };
        let Some(ctx) = current_ctx(snapshot) else {
            return;
        };
        let stroke = Stroke {
            start: state.start,
            end,
        };
        match classify_stroke(&ctx, state.grab, stroke, staged.get_untracked().total()) {
            StrokeOutcome::StageAdd(denomination) => {
                let rack = rack.get_untracked();
                staged.update(|stack| *stack = stage_add(stack, denomination, ctx.max_bet, &rack));
                settle_post();
            }
            StrokeOutcome::StageRemove(denomination) => {
                staged.update(|stack| *stack = stage_remove(stack, denomination));
                settle_post();
            }
            StrokeOutcome::Intent(intent) => on_intent.run(intent),
            StrokeOutcome::Nothing => {}
        }
    };

    let on_pointer_cancel = move |_ev: leptos::web_sys::PointerEvent| {
        drag.set(None);
    };

    let key_handle = window_event_listener(leptos::ev::keydown, move |ev| {
        let Some(command) = key_command(&ev.key()) else {
            return;
        };
        let Some(ctx) = current_ctx(snapshot) else {
            return;
        };
        ev.prevent_default();
        let betting_open = ctx.phase == Phase::Betting && ctx.allows(ActionKind::PlaceBet);
        match command {
            KeyCommand::BetUp | KeyCommand::BetDown if betting_open => {
                let up = command == KeyCommand::BetUp;
                let rack = rack.get_untracked();
                staged.update(|stack| {
                    *stack = stage_nudge(stack, up, ctx.min_bet, ctx.max_bet, &rack)
                });
                settle_post();
            }
            KeyCommand::BetUnits(units) if betting_open => {
                staged.set(stage_units(
                    units,
                    ctx.min_bet,
                    ctx.max_bet,
                    &rack.get_untracked(),
                ));
                settle_post();
            }
            KeyCommand::BetUp | KeyCommand::BetDown | KeyCommand::BetUnits(_) => {}
            other => {
                if let Some(intent) = command_intent(&ctx, other, staged.get_untracked().total()) {
                    on_intent.run(intent);
                }
            }
        }
    });
    on_cleanup(move || key_handle.remove());

    // The human's rack on the bottom rail: real chips, minus the stage.
    let rack_rail = move || {
        view! {
            <g transform=format!("translate({RACK_X} {RACK_Y})")>
                <RackView stack=visible_rack.get() cap=RACK_CAP />
            </g>
        }
    };

    // The staged (not yet posted) bet building up in the circle.
    let staged_pile = move || {
        let snap = snapshot.get()?;
        let seat = human_seat(&snap);
        if snap.phase != Phase::Betting || snap.seats[seat].bet.is_some() {
            return None;
        }
        let ctx = GestureCtx::from_snapshot(&snap, seat);
        let stack = staged.get();
        Some(view! {
            <g transform=format!("translate({:.2} {:.2})", ctx.place.x, ctx.place.y)>
                <ChipStackView stack=stack />
            </g>
        })
    };

    // Faint felt-glow hints while chips are in hand: the circle while
    // betting, both side zones over a splittable pair (double-only
    // otherwise), the insurance band during the offer.
    let hints = move || {
        let state = drag.get()?;
        matches!(state.grab, Some(Grab::Rack(_))).then_some(())?;
        let snap = snapshot.get()?;
        let seat = human_seat(&snap);
        let ctx = GestureCtx::from_snapshot(&snap, seat);
        let place = ctx.place;
        let human_turn = ctx.phase == Phase::PlayerTurn && ctx.human_active;
        let bet_hint =
            (ctx.phase == Phase::Betting && ctx.allows(ActionKind::PlaceBet)).then(|| {
                view! {
                    <circle
                        cx=place.x
                        cy=place.y
                        r=BET_ZONE_R
                        fill="rgba(255, 217, 122, 0.05)"
                        stroke="#ffd97a"
                        stroke-width="2.5"
                        opacity="0.45"
                    ></circle>
                }
            });
        let double_hint = (human_turn && ctx.allows(ActionKind::Double))
            .then(|| view! { <ZoneHint place=place cx=DOUBLE_ZONE_X cy=0.0 /> });
        let split_hint = (human_turn && ctx.allows(ActionKind::Split))
            .then(|| view! { <ZoneHint place=place cx=0.0 cy=SPLIT_ZONE_Y /> });
        let insurance_hint = (ctx.phase == Phase::InsuranceOffer
            && ctx.human_active
            && ctx.allows(ActionKind::TakeInsurance))
        .then(|| {
            let mid = (INSURANCE_R_INNER + INSURANCE_R_OUTER) / 2.0;
            view! {
                <path
                    d=arc_path(mid, -52.0, 52.0)
                    fill="none"
                    stroke="#ffd97a"
                    stroke-width=INSURANCE_R_OUTER - INSURANCE_R_INNER
                    opacity="0.12"
                ></path>
            }
        });
        Some(view! {
            {bet_hint}
            {double_hint}
            {split_hint}
            {insurance_hint}
        })
    };

    // The chip riding the cursor mid-drag.
    let ghost = move || {
        let state = drag.get()?;
        let denomination = match state.grab? {
            Grab::Rack(d) | Grab::Bet(d) => d,
        };
        Some(view! {
            <g transform=format!("translate({:.2} {:.2})", state.pos.x, state.pos.y) opacity="0.9">
                <ChipView denomination=denomination top=true />
            </g>
        })
    };

    view! {
        <svg
            viewBox=format!("0 0 {VIEW_W} {VIEW_H}")
            preserveAspectRatio="xMidYMid meet"
            style="display:block;position:absolute;top:0;left:0;width:100vw;height:100vh;\
                   touch-action:none;cursor:default;"
            on:pointerdown=on_pointer_down
            on:pointermove=on_pointer_move
            on:pointerup=on_pointer_up
            on:pointercancel=on_pointer_cancel
        >
            {rack_rail}
            {staged_pile}
            {hints}
            {ghost}
        </svg>
    }
}
