//! Scene-space endpoints for animated objects: where cards leave the
//! shoe, where they land in a hand, where chips slide from and to.
//!
//! Pure math over the scene's published geometry
//! ([`scene::geometry`](crate::scene::geometry)), so flight paths test
//! on the host and can never disagree with the rendered table. Every
//! function returns *scene coordinates* (the `0 0 1600 1000` viewBox);
//! rotations are SVG degrees (positive clockwise).

use crate::scene::geometry::{BET_CIRCLE_R, CARD_H, CARD_W, FAN_DX, FAN_DY, seat_places};
use crate::scene::{HAND_Y, INSURANCE_Y, SPLIT_BET_Y, SPLIT_DX};

/// Center of the dealing shoe. Mirrors the `<g>` transform in
/// `scene/table.rs` (`translate(1368 145) rotate(-8)`).
pub const SHOE_POS: (f64, f64) = (1368.0, 145.0);
/// The shoe's resting rotation, mirrored from `scene/table.rs`.
pub const SHOE_ANGLE: f64 = -8.0;
/// Center of the discard tray (`translate(272 122) rotate(3)`).
pub const DISCARD_POS: (f64, f64) = (272.0, 122.0);
/// Center of the dealer's chip tray (`translate(800 118)`).
pub const DEALER_TRAY_POS: (f64, f64) = (800.0, 118.0);
/// Origin of the dealer's card row (`translate(800 268)`).
pub const DEALER_HAND_POS: (f64, f64) = (800.0, 268.0);

/// How far beyond the betting circle a player's chips enter from (the
/// player's rail-side stack, just off the felt edge of the circle).
const ENTRY_REACH: f64 = 120.0;

/// A point in scene coordinates plus the rotation an object resting
/// there carries (a card in a tilted seat frame lands tilted).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Scene x of the object's center.
    pub x: f64,
    /// Scene y of the object's center.
    pub y: f64,
    /// Resting rotation in SVG degrees.
    pub rot: f64,
}

impl Anchor {
    /// An anchor with no rotation.
    pub fn flat(point: (f64, f64)) -> Anchor {
        Anchor {
            x: point.0,
            y: point.1,
            rot: 0.0,
        }
    }
}

/// Rotate `(x, y)` by `deg` about the origin, in SVG's clockwise sense.
fn rotate(x: f64, y: f64, deg: f64) -> (f64, f64) {
    let rad = deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    (x * cos - y * sin, x * sin + y * cos)
}

/// Map a point in seat `seat`'s tilted card frame (origin at the betting
/// circle center, rotated by the seat's tilt) to scene coordinates.
fn seat_frame_point(seats: usize, seat: usize, lx: f64, ly: f64) -> (f64, f64) {
    let place = seat_places(seats)[seat];
    let (dx, dy) = rotate(lx, ly, place.tilt);
    (place.x + dx, place.y + dy)
}

/// The horizontal offset of hand `hand` of `hand_count` within a seat's
/// tilted frame — split hands sit side by side.
fn hand_offset(hand_count: usize, hand: usize) -> f64 {
    (hand as f64 - (hand_count as f64 - 1.0) / 2.0) * SPLIT_DX
}

/// Where card `index` of a hand of `cards` cards comes to rest, for hand
/// `hand` of `hand_count` at seat `seat` on a `seats`-seat table.
///
/// Mirrors `SeatView`/`HandView` exactly: the hand group is centered on
/// the hand's fan width, cards step by [`FAN_DX`]/[`FAN_DY`], and the
/// whole frame tilts to face the dealer.
pub fn seat_card_center(
    seats: usize,
    seat: usize,
    hand_count: usize,
    hand: usize,
    index: usize,
    cards: usize,
) -> Anchor {
    let width = CARD_W + (cards.saturating_sub(1) as f64) * FAN_DX;
    let lx = hand_offset(hand_count, hand) - width / 2.0 + (index as f64) * FAN_DX + CARD_W / 2.0;
    let ly = HAND_Y - (index as f64) * FAN_DY + CARD_H / 2.0;
    let (x, y) = seat_frame_point(seats, seat, lx, ly);
    Anchor {
        x,
        y,
        rot: seat_places(seats)[seat].tilt,
    }
}

/// Center of the whole hand's fan (used when sweeping a busted hand).
pub fn seat_hand_center(seats: usize, seat: usize, hand_count: usize, hand: usize) -> Anchor {
    let lx = hand_offset(hand_count, hand);
    let ly = HAND_Y + CARD_H / 2.0;
    let (x, y) = seat_frame_point(seats, seat, lx, ly);
    Anchor {
        x,
        y,
        rot: seat_places(seats)[seat].tilt,
    }
}

/// Where a card in the dealer's row comes to rest: card `index` of a row
/// of `cards`. Mirrors `DealerView` (a centered row spaced
/// `CARD_W + 10`).
pub fn dealer_card_center(index: usize, cards: usize) -> Anchor {
    let spacing = CARD_W + 10.0;
    let total_w = if cards == 0 {
        0.0
    } else {
        CARD_W + (cards as f64 - 1.0) * spacing
    };
    Anchor {
        x: DEALER_HAND_POS.0 - total_w / 2.0 + (index as f64) * spacing + CARD_W / 2.0,
        y: DEALER_HAND_POS.1,
        rot: 0.0,
    }
}

/// Where the bet pile of hand `hand` of `hand_count` sits: in the
/// betting circle for an unsplit hand, beside each hand once split.
/// Mirrors `SeatView`.
pub fn bet_center(seats: usize, seat: usize, hand_count: usize, hand: usize) -> Anchor {
    let (lx, ly) = if hand_count <= 1 {
        (0.0, 0.0)
    } else {
        (hand_offset(hand_count, hand) - 28.0, SPLIT_BET_Y)
    };
    let (x, y) = seat_frame_point(seats, seat, lx, ly);
    Anchor { x, y, rot: 0.0 }
}

/// Where a payout pile is pushed to, beside the bet. Mirrors `SeatView`.
pub fn payout_center(seats: usize, seat: usize, hand_count: usize, hand: usize) -> Anchor {
    let (lx, ly) = if hand_count <= 1 {
        (BET_CIRCLE_R + 22.0, 0.0)
    } else {
        (hand_offset(hand_count, hand) + 30.0, SPLIT_BET_Y)
    };
    let (x, y) = seat_frame_point(seats, seat, lx, ly);
    Anchor { x, y, rot: 0.0 }
}

/// Where a seat's insurance chips sit on the insurance line.
pub fn insurance_center(seats: usize, seat: usize) -> Anchor {
    let (x, y) = seat_frame_point(seats, seat, 0.0, INSURANCE_Y);
    Anchor { x, y, rot: 0.0 }
}

/// Where a seat's chips enter the felt from: radially outward from the
/// betting circle, toward that player's spot at the rail.
pub fn seat_entry(seats: usize, seat: usize) -> Anchor {
    let place = seat_places(seats)[seat];
    let rad = place.angle.to_radians();
    // The radial unit vector from the arc focus through the seat.
    let (ux, uy) = (rad.sin(), rad.cos());
    Anchor {
        x: place.x + ux * (BET_CIRCLE_R + ENTRY_REACH),
        y: place.y + uy * (BET_CIRCLE_R + ENTRY_REACH),
        rot: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::geometry::{ARC_CX, ARC_CY, VIEW_H};

    const SEATS: usize = 7;

    #[test]
    fn center_seat_first_card_sits_on_the_center_line() {
        // Seat 3 of 7 is dead center with no tilt: local math is exact.
        let anchor = seat_card_center(SEATS, 3, 1, 0, 0, 1);
        assert!((anchor.x - ARC_CX).abs() < 1e-9);
        let place_y = ARC_CY + 690.0;
        assert!((anchor.y - (place_y + HAND_Y + CARD_H / 2.0)).abs() < 1e-9);
        assert!(anchor.rot.abs() < 1e-9);
    }

    #[test]
    fn cards_land_tilted_toward_the_dealer() {
        let first_base = seat_card_center(SEATS, 0, 1, 0, 0, 1);
        let third_base = seat_card_center(SEATS, 6, 1, 0, 0, 1);
        // Tilt is against the seat angle: negative on the right side.
        assert!(first_base.rot < 0.0);
        assert!(third_base.rot > 0.0);
        assert!((first_base.rot + third_base.rot).abs() < 1e-9);
        // Hands sit between their circle and the dealer (smaller y).
        let place = seat_places(SEATS)[0];
        assert!(first_base.y < place.y);
    }

    #[test]
    fn later_cards_fan_toward_first_base() {
        let a = seat_card_center(SEATS, 3, 1, 0, 0, 3);
        let b = seat_card_center(SEATS, 3, 1, 0, 2, 3);
        assert!((b.x - a.x - 2.0 * FAN_DX).abs() < 1e-9);
        assert!((a.y - b.y - 2.0 * FAN_DY).abs() < 1e-9);
    }

    #[test]
    fn split_hands_sit_apart() {
        let left = seat_hand_center(SEATS, 3, 2, 0);
        let right = seat_hand_center(SEATS, 3, 2, 1);
        assert!((right.x - left.x - SPLIT_DX).abs() < 1e-9);
    }

    #[test]
    fn dealer_row_recenters_as_it_grows() {
        let two = dealer_card_center(1, 2);
        let three = dealer_card_center(1, 3);
        // With three cards the middle card is dead center.
        assert!((three.x - DEALER_HAND_POS.0).abs() < 1e-9);
        assert!(two.x > three.x);
        assert_eq!(two.y, DEALER_HAND_POS.1);
    }

    #[test]
    fn bet_and_payout_positions_mirror_the_scene() {
        // Unsplit: bet in the circle, payout pushed beside it.
        let bet = bet_center(SEATS, 3, 1, 0);
        let place = seat_places(SEATS)[3];
        assert!((bet.x - place.x).abs() < 1e-9 && (bet.y - place.y).abs() < 1e-9);
        let payout = payout_center(SEATS, 3, 1, 0);
        assert!((payout.x - (place.x + BET_CIRCLE_R + 22.0)).abs() < 1e-9);
        // Split: bets ride beside their hands.
        let bet0 = bet_center(SEATS, 3, 2, 0);
        let bet1 = bet_center(SEATS, 3, 2, 1);
        assert!(bet0.x < bet1.x);
        assert!(bet0.y < place.y);
    }

    #[test]
    fn insurance_chips_sit_dealer_side_of_the_circle() {
        for seat in 0..SEATS {
            let anchor = insurance_center(SEATS, seat);
            let place = seat_places(SEATS)[seat];
            assert!(anchor.y < place.y, "seat {seat}");
        }
    }

    #[test]
    fn chips_enter_from_the_rail_side() {
        for seat in 0..SEATS {
            let entry = seat_entry(SEATS, seat);
            let place = seat_places(SEATS)[seat];
            assert!(entry.y > place.y, "seat {seat}");
            assert!(entry.y < VIEW_H + 60.0, "seat {seat} stays near the felt");
        }
    }

    #[test]
    fn fixed_anchors_match_the_scene_transforms() {
        // These mirror hardcoded transforms in scene/table.rs.
        assert_eq!(SHOE_POS, (1368.0, 145.0));
        assert_eq!(DISCARD_POS, (272.0, 122.0));
        assert_eq!(DEALER_TRAY_POS, (800.0, 118.0));
        assert_eq!(DEALER_HAND_POS, (800.0, 268.0));
    }
}
