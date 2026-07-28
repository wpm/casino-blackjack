//! Chip rendering: piles cut from [`ChipStack`]s in the standard casino
//! colors, with edge stripes and no numerals anywhere.
//!
//! A pile draws at the local origin with the bottom chip resting on
//! `y = 0`; callers position piles with a `transform`. Stack heights are
//! the only value cue — exactly as at a real table.

use blackjack_core::{ChipStack, Denomination};
use leptos::prelude::*;

use super::geometry::{CHIP_LIFT, CHIP_RX, CHIP_RY, chip_style, pile_chips, rack_piles};

/// One chip lying flat, its top face centered on the local origin.
///
/// `top` adds the inner ring and inlay only the exposed top chip of a
/// pile shows.
#[component]
pub fn ChipView(denomination: Denomination, #[prop(optional)] top: bool) -> impl IntoView {
    let style = chip_style(denomination);
    let detail = top.then(|| {
        view! {
            <ellipse
                cx="0"
                cy="0"
                rx=CHIP_RX - 9.0
                ry=CHIP_RY - 3.4
                fill="none"
                stroke=style.stripe
                stroke-width="1.8"
                stroke-dasharray="5 6.5"
            ></ellipse>
            <ellipse
                cx="0"
                cy="0"
                rx=CHIP_RX - 15.0
                ry=CHIP_RY - 5.6
                fill=style.inlay
                stroke="rgba(0, 0, 0, 0.18)"
                stroke-width="0.6"
            ></ellipse>
        }
    });
    view! {
        // Shaded underside of the cylinder, peeking out below the face.
        <ellipse cx="0" cy=CHIP_LIFT - 1.0 rx=CHIP_RX ry=CHIP_RY fill=style.rim></ellipse>
        // Top face with dashed edge stripes.
        <ellipse
            cx="0"
            cy="0"
            rx=CHIP_RX
            ry=CHIP_RY
            fill=style.body
            stroke="rgba(0, 0, 0, 0.28)"
            stroke-width="0.6"
        ></ellipse>
        <ellipse
            cx="0"
            cy="0"
            rx=CHIP_RX - 2.2
            ry=CHIP_RY - 1.0
            fill="none"
            stroke=style.stripe
            stroke-width="3"
            stroke-dasharray="8.5 11"
        ></ellipse>
        {detail}
    }
}

/// A single pile of chips, bottom chip first, rising off `y = 0`.
#[component]
pub fn ChipPile(chips: Vec<Denomination>) -> impl IntoView {
    let count = chips.len();
    chips
        .into_iter()
        .enumerate()
        .map(|(i, denomination)| {
            let y = -(i as f64) * CHIP_LIFT;
            let top = i + 1 == count;
            view! {
                <g transform=format!("translate(0 {y:.2})")>
                    <ChipView denomination=denomination top=top />
                </g>
            }
        })
        .collect_view()
}

/// A bet on the felt: the stack's chips as one pile, largest
/// denominations on the bottom, with a soft shadow. Renders nothing for
/// an empty stack.
#[component]
pub fn ChipStackView(stack: ChipStack) -> impl IntoView {
    (!stack.is_empty()).then(|| {
        view! {
            <ellipse
                cx="1.5"
                cy=CHIP_LIFT
                rx=CHIP_RX + 4.0
                ry=CHIP_RY + 2.0
                fill="rgba(8, 18, 12, 0.30)"
            ></ellipse>
            <ChipPile chips=pile_chips(&stack) />
        }
    })
}

/// A rack of chips laid out as tidy per-denomination piles (largest
/// first, split when taller than `cap`), centered on the local origin.
///
/// Used for the dealer's tray today; ready for #12 to feed players'
/// bankroll racks.
#[component]
pub fn RackView(stack: ChipStack, #[prop(default = 10)] cap: u32) -> impl IntoView {
    let piles = rack_piles(&stack, cap);
    let count = piles.len();
    let spacing = 64.0;
    piles
        .into_iter()
        .enumerate()
        .map(|(i, (denomination, n))| {
            let x = (i as f64 - (count as f64 - 1.0) / 2.0) * spacing;
            let chips = vec![denomination; n as usize];
            view! {
                <g transform=format!("translate({x:.2} 0)")>
                    <ChipPile chips=chips />
                </g>
            }
        })
        .collect_view()
}
