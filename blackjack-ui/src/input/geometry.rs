//! Pure spatial math for gesture recognition: viewport-to-scene mapping,
//! seat-frame conversion, and the felt's gesture zones.
//!
//! Everything here is plain math over scene coordinates (the SVG
//! viewBox, `0 0 1600 1000` — see [`crate::scene::geometry`]), with no
//! DOM anywhere, so it all tests on the host.
//!
//! # Zone map (seat frame)
//!
//! Zones for the human seat live in its *seat frame*: origin at the
//! betting-circle center, rotated by the seat's tilt so `-y` points up
//! the radial line toward the dealer and `+y` toward the player.
//!
//! ```text
//!            -y (toward dealer)
//!      ┌───────────────────────┐  y = HAND_ZONE_Y_FAR (-420)
//!      │   hit / stand zone    │  (covers the cards and the felt
//!      └───────────────────────┘  behind them)  y = HAND_ZONE_Y_NEAR (-100)
//!               (bet circle)      r = BET_ZONE_R (56)      (double zone
//!      ┌───────────────────────┐  beside: center (112, 0), r 52)
//!      │ surrender / split band│  y = BEHIND_ZONE_Y_MIN (58)
//!      └───────────────────────┘  y = BEHIND_ZONE_Y_MAX (178)
//!            +y (toward player)   (split zone behind: center (0, 112), r 52)
//! ```
//!
//! The insurance band is tested in global coordinates: the annulus
//! between [`INSURANCE_R_INNER`] and [`INSURANCE_R_OUTER`] around the
//! shared arc focus, within the seats' angular spread.

use blackjack_core::{ChipStack, Denomination};

use crate::scene::geometry::{
    ARC_CX, ARC_CY, BET_CIRCLE_R, CHIP_LIFT, INSURANCE_R_INNER, INSURANCE_R_OUTER, SeatPlace,
    VIEW_H, VIEW_W, rack_piles,
};

/// A point in scene (viewBox) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Scene x, `0..1600`.
    pub x: f64,
    /// Scene y, `0..1000`.
    pub y: f64,
}

impl Point {
    /// A point from its coordinates.
    pub fn new(x: f64, y: f64) -> Point {
        Point { x, y }
    }

    /// Euclidean distance to `other`.
    pub fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// A pointer press-move-release in scene coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// Where the pointer went down.
    pub start: Point,
    /// Where it came up.
    pub end: Point,
}

/// Maximum start-to-end travel for a stroke to read as a tap.
pub const TAP_MAX: f64 = 12.0;
/// Minimum horizontal travel for a stroke to read as a wave.
pub const WAVE_MIN_DX: f64 = 90.0;
/// Maximum vertical travel allowed in a wave.
pub const WAVE_MAX_DY: f64 = 45.0;

/// Radius of the bet-circle drop/tap zone (the printed circle plus a
/// tolerance ring).
pub const BET_ZONE_R: f64 = BET_CIRCLE_R + 10.0;
/// Seat-frame x of the double drop zone's center, beside the circle.
pub const DOUBLE_ZONE_X: f64 = 112.0;
/// Seat-frame y of the split drop zone's center, behind the circle
/// (toward the player).
pub const SPLIT_ZONE_Y: f64 = 112.0;
/// Radius of the double and split drop zones.
pub const SIDE_ZONE_R: f64 = 52.0;
/// Seat-frame half-width of the hand and behind-the-bet bands.
pub const BAND_HALF_W: f64 = 170.0;
/// Far (dealer-side) edge of the hit/stand zone.
pub const HAND_ZONE_Y_FAR: f64 = -420.0;
/// Near (circle-side) edge of the hit/stand zone.
pub const HAND_ZONE_Y_NEAR: f64 = -100.0;
/// Near (circle-side) edge of the surrender/split band behind the bet.
pub const BEHIND_ZONE_Y_MIN: f64 = 58.0;
/// Far (player-side) edge of the surrender/split band.
pub const BEHIND_ZONE_Y_MAX: f64 = 178.0;
/// Radial tolerance around the printed insurance band.
pub const INSURANCE_TOL: f64 = 8.0;
/// Angular half-spread (degrees off straight-down from the arc focus)
/// accepted for insurance drops.
pub const INSURANCE_SPREAD: f64 = 60.0;

/// Scene x of the human rack's center, on the rail at bottom center.
pub const RACK_X: f64 = 800.0;
/// Scene y of the human rack's base line.
pub const RACK_Y: f64 = 944.0;
/// Tallest pile the human rack shows (chips per pile).
pub const RACK_CAP: u32 = 8;
/// Horizontal spacing between rack piles (matches `RackView`).
pub const RACK_SPACING: f64 = 64.0;
/// Half-width of the pointer region owned by the rack.
pub const RACK_HALF_W: f64 = 200.0;
/// Top edge of the rack's pointer region.
pub const RACK_REGION_TOP: f64 = 890.0;
/// A drag ending below this scene y has left the felt off the bottom
/// edge (the walk-away release line).
pub const FELT_EXIT_Y: f64 = 985.0;
/// Minimum downward travel for a walk-away drag.
pub const WALK_MIN_DY: f64 = 40.0;

/// Map a pointer position inside the scene's bounding box to scene
/// coordinates, honoring `preserveAspectRatio="xMidYMid meet"`: the
/// viewBox scales uniformly to fit and centers on both axes.
///
/// `x`/`y` are pixels relative to the box's top-left corner (client
/// coordinates minus the bounding rect's origin).
pub fn scene_point(box_w: f64, box_h: f64, x: f64, y: f64) -> Point {
    let scale = (box_w / VIEW_W).min(box_h / VIEW_H);
    if scale <= 0.0 {
        return Point::new(0.0, 0.0);
    }
    let off_x = (box_w - VIEW_W * scale) / 2.0;
    let off_y = (box_h - VIEW_H * scale) / 2.0;
    Point::new((x - off_x) / scale, (y - off_y) / scale)
}

/// A scene point expressed in `place`'s seat frame: translated to the
/// betting-circle center and rotated back through the seat's tilt, so
/// `-y` runs up the seat's radial line toward the dealer.
pub fn seat_local(place: SeatPlace, p: Point) -> Point {
    let dx = p.x - place.x;
    let dy = p.y - place.y;
    // Inverse of the SVG `rotate(tilt)` applied to the seat's group.
    let rad = place.tilt.to_radians();
    let (sin, cos) = rad.sin_cos();
    Point::new(dx * cos + dy * sin, -dx * sin + dy * cos)
}

/// Where a chip (or tap) landed relative to a seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    /// Inside the betting circle.
    BetCircle,
    /// The double zone beside the circle.
    DoubleSide,
    /// The split zone directly behind the circle, toward the player.
    SplitBehind,
    /// On the insurance band.
    InsuranceBand,
    /// Nowhere meaningful.
    Off,
}

/// Classify where a scene point lands for `place`'s seat.
///
/// The circle wins over the side zones, which cannot overlap it; the
/// insurance band is checked last (it is far from the others).
pub fn drop_zone(place: SeatPlace, p: Point) -> DropZone {
    let local = seat_local(place, p);
    let origin = Point::new(0.0, 0.0);
    if local.distance(origin) <= BET_ZONE_R {
        DropZone::BetCircle
    } else if local.distance(Point::new(DOUBLE_ZONE_X, 0.0)) <= SIDE_ZONE_R {
        DropZone::DoubleSide
    } else if local.distance(Point::new(0.0, SPLIT_ZONE_Y)) <= SIDE_ZONE_R {
        DropZone::SplitBehind
    } else if in_insurance_band(p) {
        DropZone::InsuranceBand
    } else {
        DropZone::Off
    }
}

/// Whether a scene point sits on the printed insurance band (with
/// [`INSURANCE_TOL`] slack), inside the seats' angular spread.
pub fn in_insurance_band(p: Point) -> bool {
    let dx = p.x - ARC_CX;
    let dy = p.y - ARC_CY;
    let r = dx.hypot(dy);
    if !(INSURANCE_R_INNER - INSURANCE_TOL..=INSURANCE_R_OUTER + INSURANCE_TOL).contains(&r) {
        return false;
    }
    // Arc angle off straight-down from the focus, as in `seat_places`.
    let angle = dx.atan2(dy).to_degrees();
    angle.abs() <= INSURANCE_SPREAD
}

/// Whether a seat-frame point is in the hit/stand zone: the felt band
/// covering the seat's cards and the felt behind them.
pub fn in_hand_zone(local: Point) -> bool {
    local.x.abs() <= BAND_HALF_W && (HAND_ZONE_Y_FAR..=HAND_ZONE_Y_NEAR).contains(&local.y)
}

/// Whether a seat-frame point is in the band behind the bet (between
/// circle and player) where surrender lines are drawn and split chips
/// dropped.
pub fn in_behind_band(local: Point) -> bool {
    local.x.abs() <= BAND_HALF_W && (BEHIND_ZONE_Y_MIN..=BEHIND_ZONE_Y_MAX).contains(&local.y)
}

/// Whether a scene point is on the felt proper: inside the table
/// outline (flat top edge, half-ellipse toward the players) and not in
/// the rack's pointer region.
pub fn on_felt(p: Point) -> bool {
    if in_rack_region(p) {
        return false;
    }
    // The outline of `table_outline(0.0)`: top edge y = 40 from x 60 to
    // 1540, closed by a half-ellipse centered (800, 40), rx 740, ry 920.
    if p.y < 40.0 {
        return false;
    }
    let nx = (p.x - 800.0) / 740.0;
    let ny = (p.y - 40.0) / 920.0;
    nx * nx + ny * ny <= 1.0
}

/// Whether a scene point is inside the human rack's pointer region on
/// the bottom rail.
pub fn in_rack_region(p: Point) -> bool {
    (p.x - RACK_X).abs() <= RACK_HALF_W && p.y >= RACK_REGION_TOP
}

/// The denomination pile under a scene point in the human rack, laid
/// out exactly as `RackView` draws it (piles from [`rack_piles`] with
/// cap [`RACK_CAP`], spaced [`RACK_SPACING`], centered on
/// [`RACK_X`]/[`RACK_Y`]).
pub fn rack_pile_at(rack: &ChipStack, p: Point) -> Option<Denomination> {
    let piles = rack_piles(rack, RACK_CAP);
    let count = piles.len();
    for (i, (denomination, n)) in piles.into_iter().enumerate() {
        let cx = RACK_X + (i as f64 - (count as f64 - 1.0) / 2.0) * RACK_SPACING;
        let top = RACK_Y - (n as f64) * CHIP_LIFT - 12.0;
        if (p.x - cx).abs() <= RACK_SPACING / 2.0 && (top..=RACK_Y + 14.0).contains(&p.y) {
            return Some(denomination);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::geometry::seat_places;

    fn center_place() -> SeatPlace {
        seat_places(7)[3]
    }

    fn side_place() -> SeatPlace {
        seat_places(7)[0]
    }

    #[test]
    fn scene_point_maps_meet_letterboxing() {
        // A 3200x2000 box scales exactly 2x with no letterbox.
        let p = scene_point(3200.0, 2000.0, 1600.0, 1000.0);
        assert!((p.x - 800.0).abs() < 1e-9 && (p.y - 500.0).abs() < 1e-9);
        // A wide box letterboxes horizontally: 2000x1000 -> scale 1,
        // 200px bars left and right.
        let p = scene_point(2000.0, 1000.0, 200.0, 0.0);
        assert!((p.x - 0.0).abs() < 1e-9 && (p.y - 0.0).abs() < 1e-9);
        // A tall box letterboxes vertically: 1600x1200 -> scale 1,
        // 100px bars top and bottom.
        let p = scene_point(1600.0, 1200.0, 800.0, 100.0);
        assert!((p.x - 800.0).abs() < 1e-9 && (p.y - 0.0).abs() < 1e-9);
    }

    #[test]
    fn seat_local_undoes_the_seat_transform() {
        for place in seat_places(7) {
            // The circle center maps to the local origin.
            let origin = seat_local(place, Point::new(place.x, place.y));
            assert!(origin.x.abs() < 1e-9 && origin.y.abs() < 1e-9);
            // A point up the radial line toward the arc focus is -y.
            let toward_focus = Point::new(
                place.x + (ARC_CX - place.x) * 0.1,
                place.y + (ARC_CY - place.y) * 0.1,
            );
            let local = seat_local(place, toward_focus);
            assert!(local.x.abs() < 1e-6, "{place:?} -> {local:?}");
            assert!(local.y < 0.0);
        }
    }

    #[test]
    fn drop_zones_partition_the_seat_frame() {
        let place = center_place();
        let global = |lx: f64, ly: f64| {
            // Center seat has tilt 0: local == global offset.
            Point::new(place.x + lx, place.y + ly)
        };
        assert_eq!(drop_zone(place, global(0.0, 0.0)), DropZone::BetCircle);
        assert_eq!(drop_zone(place, global(40.0, 0.0)), DropZone::BetCircle);
        assert_eq!(
            drop_zone(place, global(DOUBLE_ZONE_X, 8.0)),
            DropZone::DoubleSide
        );
        assert_eq!(
            drop_zone(place, global(6.0, SPLIT_ZONE_Y)),
            DropZone::SplitBehind
        );
        // Far off to the left: nothing.
        assert_eq!(drop_zone(place, global(-400.0, 0.0)), DropZone::Off);
        // The insurance band along the seat's radial line.
        let band = global(
            0.0,
            -(690.0 - (INSURANCE_R_INNER + INSURANCE_R_OUTER) / 2.0),
        );
        assert_eq!(drop_zone(place, band), DropZone::InsuranceBand);
    }

    #[test]
    fn drop_zones_rotate_with_the_seat_frame() {
        // First base is tilted; "beside" and "behind" follow the tilt.
        let place = side_place();
        let rad = place.tilt.to_radians();
        let (sin, cos) = rad.sin_cos();
        let global = |lx: f64, ly: f64| {
            Point::new(place.x + lx * cos - ly * sin, place.y + lx * sin + ly * cos)
        };
        assert_eq!(
            drop_zone(place, global(DOUBLE_ZONE_X, 0.0)),
            DropZone::DoubleSide
        );
        assert_eq!(
            drop_zone(place, global(0.0, SPLIT_ZONE_Y)),
            DropZone::SplitBehind
        );
        // The unrotated global offset beside the circle is NOT the
        // double zone for a tilted seat.
        let unrotated = Point::new(place.x + DOUBLE_ZONE_X, place.y);
        let local = seat_local(place, unrotated);
        assert!(local.distance(Point::new(DOUBLE_ZONE_X, 0.0)) > 1.0);
    }

    #[test]
    fn insurance_band_hugs_the_printed_arc() {
        let mid = (INSURANCE_R_INNER + INSURANCE_R_OUTER) / 2.0;
        assert!(in_insurance_band(Point::new(ARC_CX, ARC_CY + mid)));
        // Inside the inner radius: no.
        assert!(!in_insurance_band(Point::new(
            ARC_CX,
            ARC_CY + INSURANCE_R_INNER - 20.0
        )));
        // Outside the outer radius: no.
        assert!(!in_insurance_band(Point::new(
            ARC_CX,
            ARC_CY + INSURANCE_R_OUTER + 20.0
        )));
        // Same radius but far outside the angular spread: no.
        assert!(!in_insurance_band(Point::new(ARC_CX + mid, ARC_CY)));
    }

    #[test]
    fn hand_and_behind_bands_do_not_touch_the_circle() {
        assert!(in_hand_zone(Point::new(0.0, -245.0)));
        assert!(in_hand_zone(Point::new(0.0, -110.0)));
        assert!(!in_hand_zone(Point::new(0.0, -BET_ZONE_R)));
        assert!(in_behind_band(Point::new(0.0, 112.0)));
        assert!(!in_behind_band(Point::new(0.0, BET_ZONE_R - 5.0)));
        assert!(!in_behind_band(Point::new(300.0, 112.0)));
    }

    #[test]
    fn felt_test_excludes_room_rail_bottom_and_rack() {
        assert!(on_felt(Point::new(800.0, 500.0)));
        assert!(on_felt(Point::new(300.0, 300.0)));
        // Above the dealer edge: the room.
        assert!(!on_felt(Point::new(800.0, 20.0)));
        // Below the bottom lip of the half-ellipse.
        assert!(!on_felt(Point::new(800.0, 995.0)));
        // The rack region is not felt.
        assert!(!on_felt(Point::new(RACK_X, 940.0)));
        // Outside the ellipse near a bottom corner.
        assert!(!on_felt(Point::new(80.0, 900.0)));
    }

    #[test]
    fn rack_piles_are_grabbable_by_denomination() {
        let mut rack = ChipStack::new();
        rack.add_chips(Denomination::Five, 8);
        rack.add_chips(Denomination::TwentyFive, 8);
        rack.add_chips(Denomination::Hundred, 6);
        // Three piles, largest first, centered: $100 at x-64, $25 at
        // x 800, $5 at x+64.
        assert_eq!(
            rack_pile_at(&rack, Point::new(RACK_X - RACK_SPACING, RACK_Y)),
            Some(Denomination::Hundred)
        );
        assert_eq!(
            rack_pile_at(&rack, Point::new(RACK_X, RACK_Y - 20.0)),
            Some(Denomination::TwentyFive)
        );
        assert_eq!(
            rack_pile_at(&rack, Point::new(RACK_X + RACK_SPACING, RACK_Y)),
            Some(Denomination::Five)
        );
        // Between piles but past the half-spacing: nothing.
        assert_eq!(
            rack_pile_at(&rack, Point::new(RACK_X + 2.0 * RACK_SPACING, RACK_Y)),
            None
        );
        // Above the pile tops: nothing.
        assert_eq!(rack_pile_at(&rack, Point::new(RACK_X, RACK_Y - 80.0)), None);
        assert_eq!(
            rack_pile_at(&ChipStack::new(), Point::new(RACK_X, RACK_Y)),
            None
        );
    }
}
