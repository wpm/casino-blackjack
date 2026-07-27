//! Player seats: betting circles, bet piles, and hands in progress.

use blackjack_core::{ChipStack, HandOutcome, HandSnapshot, HandStatus, Insurance, SeatSnapshot};
use leptos::prelude::*;

use super::card::CardFace;
use super::chips::ChipStackView;
use super::geometry::{BET_CIRCLE_R, CARD_H, CARD_W, FAN_DX, FAN_DY, SeatPlace, bust_jitter};

/// Vertical offset from the betting-circle center to the top of a
/// hand's first card. Public so motion (#10) can land cards exactly
/// where this view rests them.
pub const HAND_Y: f64 = -245.0;
/// Horizontal spacing between split hands.
pub const SPLIT_DX: f64 = 112.0;
/// Where a split hand's bet pile sits, between circle and cards.
pub const SPLIT_BET_Y: f64 = -95.0;
/// Where insurance chips sit: on the seat's radial line, inside the
/// insurance band.
pub const INSURANCE_Y: f64 = -282.0;

/// Whether a settled hand's bet has been swept by the dealer.
fn bet_swept(hand: &HandSnapshot) -> bool {
    matches!(
        hand.outcome,
        Some(HandOutcome::Lose | HandOutcome::Bust | HandOutcome::Surrender)
    )
}

/// One hand's fan of cards, drawn with its first card's top-left corner
/// at the local origin and later cards stepping up toward the dealer.
///
/// Status shows without a single text label: the active hand glows, a
/// blackjack gets a gold aura, a busted hand's cards scatter and drain
/// of color, a surrendered hand dims.
#[component]
pub fn HandView(hand: HandSnapshot, active: bool) -> impl IntoView {
    let count = hand.cards.len();
    let spread_x = CARD_W + (count.saturating_sub(1) as f64) * FAN_DX;
    let spread_y = (count.saturating_sub(1) as f64) * FAN_DY;
    let center_x = spread_x / 2.0;
    let center_y = (CARD_H - spread_y) / 2.0;

    let bust = hand.status == HandStatus::Bust;
    let group_opacity = match hand.status {
        HandStatus::Surrendered => Some(0.4),
        HandStatus::Bust => Some(0.9),
        _ => None,
    };
    let group_filter = bust.then_some("url(#desaturate)");

    let glow = (active || hand.status == HandStatus::Blackjack).then(|| {
        let rx = spread_x / 2.0 + 28.0;
        let ry = CARD_H / 2.0 + spread_y / 2.0 + 24.0;
        let (fill, fill_opacity, ring_opacity) = if active {
            ("#ffe9b0", 0.20, 0.85)
        } else {
            ("#f2c94c", 0.28, 0.0)
        };
        view! {
            <ellipse
                cx=center_x
                cy=center_y
                rx=rx
                ry=ry
                fill=fill
                opacity=fill_opacity
                filter="url(#soft-blur)"
            ></ellipse>
            <ellipse
                cx=center_x
                cy=center_y
                rx=rx
                ry=ry
                fill="none"
                stroke="#ffd97a"
                stroke-width="4.5"
                opacity=ring_opacity
                filter="url(#soft-blur)"
            ></ellipse>
        }
    });

    let cards = hand
        .cards
        .iter()
        .enumerate()
        .map(|(i, &card)| {
            let mut x = (i as f64) * FAN_DX;
            let mut y = -(i as f64) * FAN_DY;
            let mut rot = 0.0;
            if bust {
                let (dx, dy, jitter_rot) = bust_jitter(i);
                x += dx;
                y += dy;
                rot += jitter_rot;
            }
            // The double-down card lands sideways, casino-style.
            if hand.doubled && i + 1 == count {
                rot += 90.0;
            }
            let transform = format!(
                "translate({x:.2} {y:.2}) rotate({rot:.1} {:.1} {:.1})",
                CARD_W / 2.0,
                CARD_H / 2.0
            );
            view! {
                <g transform=transform>
                    <CardFace card=card />
                </g>
            }
        })
        .collect_view();

    view! {
        {glow}
        <g opacity=group_opacity filter=group_filter>
            {cards}
        </g>
    }
}

/// Everything one seat shows, drawn around its betting circle at the
/// local origin: the printed circle, the bet (and any payout being
/// pushed), insurance chips on the line, and the seat's hands — split
/// hands side by side.
#[component]
pub fn SeatView(
    seat: SeatSnapshot,
    place: SeatPlace,
    /// Index of this seat's active hand, when the engine waits on it.
    active_hand: Option<usize>,
    /// Glow the betting circle itself (the seat is deciding insurance).
    circle_glow: bool,
    /// Whether insurance chips ride the line right now.
    insurance_visible: bool,
) -> impl IntoView {
    let hand_count = seat.hands.len();

    let circle_ring = circle_glow.then(|| {
        view! {
            <circle
                cx="0"
                cy="0"
                r=BET_CIRCLE_R + 9.0
                fill="none"
                stroke="#ffd97a"
                stroke-width="5"
                opacity="0.85"
                filter="url(#soft-blur)"
            ></circle>
        }
    });

    // Bets: in the circle before the deal and for an unsplit hand;
    // beside each hand once split. Swept bets vanish; winning bets get
    // the payout pushed alongside.
    let chips = if hand_count == 0 {
        seat.bet
            .map(|bet| {
                view! { <ChipStackView stack=ChipStack::change(bet) /> }
            })
            .into_any()
    } else {
        seat.hands
            .iter()
            .enumerate()
            .map(|(j, hand)| {
                let offset = (j as f64 - (hand_count as f64 - 1.0) / 2.0) * SPLIT_DX;
                let (bet_pos, payout_pos) = if hand_count == 1 {
                    ((0.0, 0.0), (BET_CIRCLE_R + 22.0, 0.0))
                } else {
                    ((offset - 28.0, SPLIT_BET_Y), (offset + 30.0, SPLIT_BET_Y))
                };
                let bet_pile = (!bet_swept(hand)).then(|| {
                    view! {
                        <g transform=format!("translate({:.2} {:.2})", bet_pos.0, bet_pos.1)>
                            <ChipStackView stack=ChipStack::change(hand.bet) />
                        </g>
                    }
                });
                let payout_pile = hand.payout.filter(|&net| net > 0).map(|net| {
                    view! {
                        <g transform=format!(
                            "translate({:.2} {:.2})",
                            payout_pos.0,
                            payout_pos.1
                        )>
                            <ChipStackView stack=ChipStack::change(net as u32) />
                        </g>
                    }
                });
                view! {
                    {bet_pile}
                    {payout_pile}
                }
            })
            .collect_view()
            .into_any()
    };

    let insurance = insurance_visible
        .then(|| match seat.insurance {
            Insurance::Taken { amount } => Some(view! {
                <g transform=format!("translate(0 {INSURANCE_Y})")>
                    <ChipStackView stack=ChipStack::change(amount) />
                </g>
            }),
            _ => None,
        })
        .flatten();

    let hands = seat
        .hands
        .iter()
        .enumerate()
        .map(|(j, hand)| {
            let offset = (j as f64 - (hand_count as f64 - 1.0) / 2.0) * SPLIT_DX;
            let width = CARD_W + (hand.cards.len().saturating_sub(1) as f64) * FAN_DX;
            let x = offset - width / 2.0;
            view! {
                <g transform=format!("translate({x:.2} {HAND_Y})")>
                    <HandView hand=hand.clone() active=active_hand == Some(j) />
                </g>
            }
        })
        .collect_view();

    view! {
        <g transform=format!("translate({:.2} {:.2})", place.x, place.y)>
            // The printed betting circle.
            <circle
                cx="0"
                cy="0"
                r=BET_CIRCLE_R
                fill="rgba(255, 255, 255, 0.03)"
                stroke="#d8c37a"
                stroke-width="3"
            ></circle>
            <circle
                cx="0"
                cy="0"
                r=BET_CIRCLE_R - 6.0
                fill="none"
                stroke="rgba(216, 195, 122, 0.35)"
                stroke-width="1.2"
            ></circle>
            {circle_ring}
            // Cards and chips face the dealer.
            <g transform=format!("rotate({:.2})", place.tilt)>
                {hands}
                {chips}
                {insurance}
            </g>
        </g>
    }
}
