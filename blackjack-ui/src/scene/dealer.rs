//! The dealer's side of the felt: chip tray, dealing shoe, discard
//! tray, and the dealer's hand.

use blackjack_core::{ChipStack, DealerSnapshot, Denomination, ShoeStatus};
use leptos::prelude::*;

use super::card::{CardBack, CardFace};
use super::chips::RackView;
use super::geometry::{CARD_H, CARD_W, discard_fill, shoe_fill};

/// The house float shown in the dealer's tray.
///
/// The engine snapshot does not carry the tray's contents, so the scene
/// shows a representative, well-stocked float. Purely visual; #12's
/// session arc can feed [`DealerTray`] a real tray stack instead.
fn house_float() -> ChipStack {
    let mut float = ChipStack::new();
    for denomination in Denomination::ALL {
        float.add_chips(denomination, 9);
    }
    float
}

/// The dealer's metal chip tray, centered on the local origin.
#[component]
pub fn DealerTray() -> impl IntoView {
    view! {
        <rect
            x="-212"
            y="-56"
            width="424"
            height="112"
            rx="12"
            fill="url(#tray-metal)"
            stroke="#3f424a"
            stroke-width="2.5"
        ></rect>
        <rect
            x="-202"
            y="-47"
            width="404"
            height="94"
            rx="8"
            fill="rgba(12, 14, 18, 0.55)"
        ></rect>
        <g transform="translate(0 34)">
            <RackView stack=house_float() />
        </g>
    }
}

/// The dealing shoe, centered on the local origin: a wedge of undealt
/// cards that visibly shortens as the shoe burns down — a counter's
/// penetration cue, with no numbers.
#[component]
pub fn ShoeView(shoe: ShoeStatus) -> impl IntoView {
    let fill = shoe_fill(&shoe);
    let wedge_w = 148.0 * fill;
    // Card edges inside the wedge, one line per few cards.
    let mut edges = String::new();
    let mut x = 74.0 - wedge_w + 3.0;
    while x < 71.0 {
        edges.push_str(&format!("M {x:.1} -32 L {x:.1} 20 "));
        x += 4.0;
    }
    view! {
        // Shoe body.
        <rect
            x="-96"
            y="-54"
            width="192"
            height="108"
            rx="10"
            fill="#4c1a22"
            stroke="#26090e"
            stroke-width="3"
        ></rect>
        <rect
            x="-88"
            y="-46"
            width="176"
            height="92"
            rx="6"
            fill="none"
            stroke="rgba(216, 195, 122, 0.35)"
            stroke-width="1.5"
        ></rect>
        // Card well.
        <rect x="-78" y="-38" width="152" height="64" rx="4" fill="#1a0b0e"></rect>
        // The wedge of cards still to be dealt, anchored at the mouth.
        <rect
            x=74.0 - wedge_w
            y="-34"
            width=wedge_w
            height="56"
            fill="#efe9da"
        ></rect>
        <path d=edges stroke="#c9c2af" stroke-width="1" fill="none"></path>
        // The shoe's mouth roller.
        <rect x="66" y="-38" width="10" height="64" rx="3" fill="#2e1216"></rect>
        // Felt-side lip the next card slides onto.
        <rect x="-96" y="38" width="192" height="12" rx="4" fill="#33121a"></rect>
    }
}

/// The discard tray, centered on the local origin: a stack of face-down
/// cards that grows as the shoe burns down. When the cut card has been
/// reached it rides on top of the pile.
#[component]
pub fn DiscardTrayView(shoe: ShoeStatus) -> impl IntoView {
    let layers = (discard_fill(&shoe) * 15.0).ceil() as usize;
    let pile = (0..layers)
        .map(|i| {
            let y = 34.0 - (i as f64) * 4.6;
            let top = i + 1 == layers;
            let fill = if top { "#8d2836" } else { "#71202c" };
            view! {
                <rect
                    x=-62.0 + (i as f64) * 0.5
                    y=y
                    width="124"
                    height="9"
                    rx="3"
                    fill=fill
                    stroke="rgba(20, 6, 9, 0.5)"
                    stroke-width="0.7"
                ></rect>
            }
        })
        .collect_view();
    let cut_card = shoe.cut_card_reached.then(|| {
        let y = 30.0 - (layers as f64) * 4.6;
        view! {
            <rect
                x=-60.0
                y=y
                width="124"
                height="6"
                rx="2"
                fill="#d8a12f"
                stroke="rgba(90, 62, 8, 0.6)"
                stroke-width="0.7"
            ></rect>
        }
    });
    view! {
        // Tray base and back wall.
        <rect
            x="-74"
            y="-58"
            width="148"
            height="116"
            rx="8"
            fill="#241318"
            stroke="#0f0608"
            stroke-width="2.5"
        ></rect>
        <rect
            x="-68"
            y="-52"
            width="136"
            height="104"
            rx="5"
            fill="none"
            stroke="rgba(216, 195, 122, 0.22)"
            stroke-width="1.2"
        ></rect>
        {pile}
        {cut_card}
    }
}

/// A card position in the dealer's row.
#[derive(Clone, Copy)]
enum DealerCard {
    Up(blackjack_core::Card),
    Hole,
}

/// The dealer's hand, centered on the local origin: upcard, hole card
/// (a patterned back until revealed), then draws, dealt left to right.
#[component]
pub fn DealerView(dealer: DealerSnapshot) -> impl IntoView {
    let mut row: Vec<DealerCard> = Vec::new();
    if let Some(upcard) = dealer.upcard {
        row.push(DealerCard::Up(upcard));
    }
    if let Some(hole) = dealer.hole_card {
        row.push(DealerCard::Up(hole));
    } else if dealer.hole_card_dealt {
        row.push(DealerCard::Hole);
    }
    for draw in &dealer.draws {
        row.push(DealerCard::Up(*draw));
    }
    let spacing = CARD_W + 10.0;
    let count = row.len();
    let total_w = if count == 0 {
        0.0
    } else {
        CARD_W + (count as f64 - 1.0) * spacing
    };
    row.into_iter()
        .enumerate()
        .map(|(i, card)| {
            let x = -total_w / 2.0 + (i as f64) * spacing;
            let transform = format!("translate({x:.2} {:.2})", -CARD_H / 2.0);
            match card {
                DealerCard::Up(card) => view! {
                    <g transform=transform>
                        <CardFace card=card />
                    </g>
                }
                .into_any(),
                DealerCard::Hole => view! {
                    <g transform=transform>
                        <CardBack />
                    </g>
                }
                .into_any(),
            }
        })
        .collect_view()
}
