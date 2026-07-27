//! The table scene root: felt, rail, markings, and every object on the
//! table, rendered purely from a [`Snapshot`].

use blackjack_core::{Phase, Snapshot};
use leptos::prelude::*;

use super::dealer::{DealerTray, DealerView, DiscardTrayView, ShoeView};
use super::geometry::{
    INSURANCE_R_INNER, INSURANCE_R_OUTER, INSURANCE_R_TEXT, VIEW_H, VIEW_W, arc_path, seat_places,
    table_outline,
};
use super::placard::Placard;
use super::seat::SeatView;

/// Angular half-span of the insurance band, in arc degrees.
const INSURANCE_SPAN: f64 = 52.0;

/// Every gradient, filter, and clip the scene's components reference.
///
/// Shared ids (all defined here, used across the scene modules):
/// `room`, `felt`, `felt-vignette`, `wood-rail`, `card-face`,
/// `court-plate`, `plaque-face`, `tray-metal`, `soft-blur`,
/// `desaturate`, `card-back-clip`, and `insurance-arc-path`.
#[component]
fn SceneDefs() -> impl IntoView {
    view! {
        <defs>
            <radialGradient id="room" cx="50%" cy="40%" r="75%">
                <stop offset="0%" stop-color="#1b1420"></stop>
                <stop offset="100%" stop-color="#0b0910"></stop>
            </radialGradient>
            <radialGradient id="felt" cx="50%" cy="42%" r="68%">
                <stop offset="0%" stop-color="#2f8153"></stop>
                <stop offset="45%" stop-color="#276f46"></stop>
                <stop offset="80%" stop-color="#1a5433"></stop>
                <stop offset="100%" stop-color="#124026"></stop>
            </radialGradient>
            <radialGradient id="felt-vignette" cx="50%" cy="45%" r="70%">
                <stop offset="0%" stop-color="rgba(0, 0, 0, 0)"></stop>
                <stop offset="62%" stop-color="rgba(0, 0, 0, 0)"></stop>
                <stop offset="100%" stop-color="rgba(0, 0, 0, 0.38)"></stop>
            </radialGradient>
            <linearGradient id="wood-rail" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="#8a6134"></stop>
                <stop offset="35%" stop-color="#5d3d1e"></stop>
                <stop offset="70%" stop-color="#6f4a25"></stop>
                <stop offset="100%" stop-color="#472e15"></stop>
            </linearGradient>
            <linearGradient id="card-face" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="#ffffff"></stop>
                <stop offset="100%" stop-color="#f1ede1"></stop>
            </linearGradient>
            <linearGradient id="court-plate" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="#f8f2dd"></stop>
                <stop offset="100%" stop-color="#eadfba"></stop>
            </linearGradient>
            <linearGradient id="plaque-face" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="#f6efd9"></stop>
                <stop offset="100%" stop-color="#e2d5ae"></stop>
            </linearGradient>
            <linearGradient id="tray-metal" x1="0%" y1="0%" x2="0%" y2="100%">
                <stop offset="0%" stop-color="#b9bdc6"></stop>
                <stop offset="50%" stop-color="#83878f"></stop>
                <stop offset="100%" stop-color="#5f636b"></stop>
            </linearGradient>
            <filter id="soft-blur" x="-40%" y="-40%" width="180%" height="180%">
                <feGaussianBlur stdDeviation="6"></feGaussianBlur>
            </filter>
            <filter id="desaturate">
                <feColorMatrix type="saturate" values="0.35"></feColorMatrix>
            </filter>
            <clipPath id="card-back-clip">
                <rect x="6" y="6" width="80" height="118" rx="5"></rect>
            </clipPath>
            <path
                id="insurance-arc-path"
                d=arc_path(INSURANCE_R_TEXT, -INSURANCE_SPAN, INSURANCE_SPAN)
                fill="none"
            ></path>
        </defs>
    }
}

/// The insurance band: twin gold arcs with the traditional embroidered
/// lettering. Rendered only when the posted rules offer insurance.
#[component]
fn InsuranceArc() -> impl IntoView {
    view! {
        <path
            d=arc_path(INSURANCE_R_OUTER, -INSURANCE_SPAN, INSURANCE_SPAN)
            fill="none"
            stroke="#d8c37a"
            stroke-width="3"
            opacity="0.9"
        ></path>
        <path
            d=arc_path(INSURANCE_R_INNER, -INSURANCE_SPAN, INSURANCE_SPAN)
            fill="none"
            stroke="#d8c37a"
            stroke-width="2.2"
            opacity="0.8"
        ></path>
        <text
            font-family="Georgia, 'Palatino Linotype', 'Times New Roman', serif"
            font-size="30"
            font-weight="bold"
            letter-spacing="6"
            fill="#d8c37a"
            opacity="0.92"
        >
            <textPath href="#insurance-arc-path" startOffset="50%" text-anchor="middle">
                {"\u{2666} INSURANCE PAYS 2 TO 1 \u{2666}"}
            </textPath>
        </text>
    }
}

/// The complete table, top down, as one responsive SVG — a pure
/// function of the [`Snapshot`] (and the [`Rules`](blackjack_core::Rules)
/// frozen inside it). No backend calls, no interaction: #9 layers
/// gestures on top and #10 the motion.
#[component]
pub fn TableScene(snapshot: Snapshot) -> impl IntoView {
    let places = seat_places(snapshot.seats.len());
    let phase = snapshot.phase;
    let active = snapshot.active;
    let insurance_visible = matches!(phase, Phase::InsuranceOffer | Phase::PlayerTurn);

    let seats = snapshot
        .seats
        .iter()
        .zip(places)
        .enumerate()
        .map(|(i, (seat, place))| {
            let is_active_seat = active.is_some_and(|a| a.seat == i);
            let active_hand = (phase == Phase::PlayerTurn && is_active_seat)
                .then(|| active.map(|a| a.hand))
                .flatten();
            let circle_glow = phase == Phase::InsuranceOffer && is_active_seat;
            view! {
                <SeatView
                    seat=seat.clone()
                    place=place
                    active_hand=active_hand
                    circle_glow=circle_glow
                    insurance_visible=insurance_visible
                />
            }
        })
        .collect_view();

    let insurance_line = snapshot
        .rules
        .insurance_offered
        .then(|| view! { <InsuranceArc /> });

    view! {
        <svg
            viewBox=format!("0 0 {VIEW_W} {VIEW_H}")
            preserveAspectRatio="xMidYMid meet"
            style="display:block;width:100vw;height:100vh;background:#0b0910;"
        >
            <SceneDefs />
            // The room around the table.
            <rect x="0" y="0" width=VIEW_W height=VIEW_H fill="url(#room)"></rect>
            // The felt and its printed markings.
            <path d=table_outline(0.0) fill="url(#felt)"></path>
            <path
                d=arc_path(320.0, -66.0, 66.0)
                fill="none"
                stroke="rgba(216, 195, 122, 0.25)"
                stroke-width="2"
            ></path>
            {insurance_line}
            <path d=table_outline(0.0) fill="url(#felt-vignette)"></path>
            // The padded wood rail and its brass piping.
            <path
                d=table_outline(0.0)
                fill="none"
                stroke="url(#wood-rail)"
                stroke-width="52"
                stroke-linejoin="round"
            ></path>
            <path
                d=table_outline(27.0)
                fill="none"
                stroke="rgba(202, 166, 79, 0.8)"
                stroke-width="2"
            ></path>
            // The dealer's side, left to right: discard tray, placard,
            // chip tray, cards, shoe.
            <g transform="translate(272 122) rotate(3)">
                <DiscardTrayView shoe=snapshot.shoe />
            </g>
            <g transform="translate(230 265) rotate(-8) scale(0.85)">
                <Placard rules=snapshot.rules.clone() />
            </g>
            <g transform="translate(800 118)">
                <DealerTray />
            </g>
            <g transform="translate(800 268)">
                <DealerView dealer=snapshot.dealer.clone() />
            </g>
            <g transform="translate(1368 145) rotate(-8)">
                <ShoeView shoe=snapshot.shoe />
            </g>
            // The seats, first base (seat 0) on the right.
            {seats}
        </svg>
    }
}
