//! SVG playing cards: legible faces with true pip layouts, ornamental
//! courts, and a patterned back for the dealer's hole card.
//!
//! Cards draw at the local origin, `0,0` to
//! [`CARD_W`]`x`[`CARD_H`]; callers position them with a `transform` on
//! an enclosing `<g>`. Gradient, filter, and clip ids referenced here
//! (`card-face`, `court-plate`, `card-back-clip`) are provided once by
//! [`SceneDefs`](super::SceneDefs).

use blackjack_core::{Card, Rank};
use leptos::prelude::*;

use super::geometry::{CARD_H, CARD_W, is_court, pip_layout, rank_label, suit_color, suit_glyph};

/// Serif stack used for all card ink.
const CARD_FONT: &str = "Georgia, 'Palatino Linotype', 'Times New Roman', serif";

/// Pip area of the card face, in card-local coordinates.
const PIP_X0: f64 = 26.0;
const PIP_X1: f64 = 66.0;
const PIP_Y0: f64 = 28.0;
const PIP_Y1: f64 = 106.0;

/// The white body, drop shadow, and border shared by faces and backs.
#[component]
fn CardBlank() -> impl IntoView {
    view! {
        <rect
            x="3.5"
            y="4.5"
            width=CARD_W
            height=CARD_H
            rx="9"
            fill="rgba(10, 22, 14, 0.35)"
        ></rect>
        <rect
            x="0"
            y="0"
            width=CARD_W
            height=CARD_H
            rx="9"
            fill="url(#card-face)"
            stroke="#b9b2a4"
            stroke-width="1"
        ></rect>
    }
}

/// One corner index: the rank over a small suit pip.
#[component]
fn CornerIndex(card: Card) -> impl IntoView {
    let color = suit_color(card.suit);
    let rank_size = if card.rank == Rank::Ten { 20.0 } else { 24.0 };
    view! {
        <text
            x="15"
            y="24"
            text-anchor="middle"
            font-family=CARD_FONT
            font-weight="bold"
            font-size=rank_size
            fill=color
        >
            {rank_label(card.rank)}
        </text>
        <text
            x="15"
            y="42"
            text-anchor="middle"
            font-family=CARD_FONT
            font-size="16"
            fill=color
        >
            {suit_glyph(card.suit)}
        </text>
    }
}

/// The ornamental plate of a court card: a bordered panel with a large
/// rank letter — stylized, but instantly readable.
#[component]
fn CourtPlate(card: Card) -> impl IntoView {
    let color = suit_color(card.suit);
    view! {
        <rect
            x="20"
            y="26"
            width="52"
            height="78"
            rx="4"
            fill="url(#court-plate)"
            stroke=color
            stroke-width="2"
        ></rect>
        <rect
            x="24.5"
            y="30.5"
            width="43"
            height="69"
            rx="2"
            fill="none"
            stroke=color
            stroke-width="0.8"
            opacity="0.55"
        ></rect>
        <text
            x="46"
            y="65"
            text-anchor="middle"
            dominant-baseline="central"
            font-family=CARD_FONT
            font-weight="bold"
            font-size="42"
            fill=color
        >
            {rank_label(card.rank)}
        </text>
        <text
            x="46"
            y="40"
            text-anchor="middle"
            dominant-baseline="central"
            font-family=CARD_FONT
            font-size="13"
            fill=color
        >
            {suit_glyph(card.suit)}
        </text>
        <g transform="rotate(180 46 90)">
            <text
                x="46"
                y="90"
                text-anchor="middle"
                dominant-baseline="central"
                font-family=CARD_FONT
                font-size="13"
                fill=color
            >
                {suit_glyph(card.suit)}
            </text>
        </g>
    }
}

/// A face-up playing card at the local origin.
///
/// Rank indices sit in opposite corners; number cards carry the
/// traditional pip layout, the ace one oversized pip, and courts an
/// ornamental plate.
#[component]
pub fn CardFace(card: Card) -> impl IntoView {
    let color = suit_color(card.suit);
    let pips = (!is_court(card.rank)).then(|| {
        let size = if card.rank == Rank::Ace { 54.0 } else { 20.0 };
        pip_layout(card.rank)
            .iter()
            .map(|&(nx, ny)| {
                let px = PIP_X0 + nx * (PIP_X1 - PIP_X0);
                let py = PIP_Y0 + ny * (PIP_Y1 - PIP_Y0);
                view! {
                    <text
                        x=px
                        y=py
                        text-anchor="middle"
                        dominant-baseline="central"
                        font-family=CARD_FONT
                        font-size=size
                        fill=color
                    >
                        {suit_glyph(card.suit)}
                    </text>
                }
            })
            .collect_view()
    });
    let court = is_court(card.rank).then(|| view! { <CourtPlate card=card /> });
    view! {
        <CardBlank />
        <CornerIndex card=card />
        <g transform=format!("rotate(180 {} {})", CARD_W / 2.0, CARD_H / 2.0)>
            <CornerIndex card=card />
        </g>
        {pips}
        {court}
    }
}

/// The diagonal lattice pattern of the card back, as one path.
fn back_lattice() -> String {
    let mut d = String::new();
    let mut x = -120.0;
    while x < 90.0 {
        // Down-right diagonals.
        d.push_str(&format!("M {:.1} 6 L {:.1} 124 ", 6.0 + x, 124.0 + x));
        // Down-left diagonals.
        d.push_str(&format!("M {:.1} 6 L {:.1} 124 ", 86.0 - x, -32.0 - x));
        x += 12.0;
    }
    d
}

/// A face-down card at the local origin: the dealer's hole card before
/// the reveal.
#[component]
pub fn CardBack() -> impl IntoView {
    view! {
        <CardBlank />
        <rect x="6" y="6" width="80" height="118" rx="5" fill="#71202c"></rect>
        <g clip-path="url(#card-back-clip)">
            <path
                d=back_lattice()
                stroke="rgba(240, 226, 195, 0.30)"
                stroke-width="1.4"
                fill="none"
            ></path>
        </g>
        <rect
            x="11"
            y="11"
            width="70"
            height="108"
            rx="3"
            fill="none"
            stroke="rgba(240, 226, 195, 0.55)"
            stroke-width="1.4"
        ></rect>
        <ellipse
            cx=CARD_W / 2.0
            cy=CARD_H / 2.0
            rx="19"
            ry="29"
            fill="rgba(240, 226, 195, 0.10)"
            stroke="rgba(240, 226, 195, 0.5)"
            stroke-width="1.2"
        ></ellipse>
        <ellipse
            cx=CARD_W / 2.0
            cy=CARD_H / 2.0
            rx="12"
            ry="20"
            fill="none"
            stroke="rgba(240, 226, 195, 0.35)"
            stroke-width="1"
        ></ellipse>
    }
}
