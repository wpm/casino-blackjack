//! Pure layout and styling data for the table scene.
//!
//! Everything here is a plain function of engine types — no DOM, no
//! signals — so it compiles and tests on the host under `cargo test`.
//! The rendering components in the sibling modules are thin views over
//! these tables.
//!
//! # Coordinate system
//!
//! The scene lives in one SVG viewBox, `0 0 1600 1000`, scaling
//! responsively to the window. The dealer side is the top edge; the
//! players' arc bows toward the bottom. All the arcs (betting circles,
//! insurance band, seat card rows) share one focus, [`ARC_CX`]/[`ARC_CY`],
//! a point above the visible felt: a seat at *arc angle* `a` (degrees,
//! `0` pointing straight down toward the viewer, positive to the right)
//! sits at `(ARC_CX + r·sin a, ARC_CY + r·cos a)`.

use blackjack_core::{
    BlackjackPayout, ChipColor, ChipStack, Denomination, Rank, Rules, ShoeStatus, Soft17, Suit,
};

/// Width of the scene viewBox in scene units.
pub const VIEW_W: f64 = 1600.0;
/// Height of the scene viewBox in scene units.
pub const VIEW_H: f64 = 1000.0;

/// X of the shared arc focus all table arcs are drawn around.
pub const ARC_CX: f64 = 800.0;
/// Y of the shared arc focus (above the felt's visible top edge).
pub const ARC_CY: f64 = 100.0;

/// Radius of the betting-circle arc.
pub const SEAT_RING_R: f64 = 690.0;
/// Radius of a single betting circle.
pub const BET_CIRCLE_R: f64 = 46.0;
/// Half-spread of the seat arc in degrees (seat 0 at `+SEAT_SPREAD`).
pub const SEAT_SPREAD: f64 = 52.0;

/// Inner radius of the insurance band.
pub const INSURANCE_R_INNER: f64 = 380.0;
/// Outer radius of the insurance band.
pub const INSURANCE_R_OUTER: f64 = 435.0;
/// Radius of the insurance lettering baseline.
pub const INSURANCE_R_TEXT: f64 = 424.0;

/// Card width in scene units. Sized so every index at the table stays
/// legible — card counting depends on it.
pub const CARD_W: f64 = 92.0;
/// Card height in scene units.
pub const CARD_H: f64 = 130.0;
/// Fan offset between successive cards in a hand, x then y.
pub const FAN_DX: f64 = 26.0;
/// Upward step between successive cards in a hand.
pub const FAN_DY: f64 = 20.0;

/// Vertical rise per chip in a rendered pile.
pub const CHIP_LIFT: f64 = 5.5;
/// Horizontal radius of a chip ellipse.
pub const CHIP_RX: f64 = 27.0;
/// Vertical radius of a chip ellipse (the top-down side-view illusion).
pub const CHIP_RY: f64 = 10.0;

/// Where a seat sits on the felt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeatPlace {
    /// Center of the betting circle.
    pub x: f64,
    /// Center of the betting circle.
    pub y: f64,
    /// Arc angle in degrees (0 = straight down from the arc focus,
    /// positive toward the right edge).
    pub angle: f64,
    /// Rotation applied to the seat's card group so hands face the
    /// dealer, in degrees. Exactly `-angle`, so everything in the
    /// rotated seat frame lies along the seat's radial line.
    pub tilt: f64,
}

/// Betting-circle placements for `seats` seats, seat 0 first.
///
/// Seat 0 is first base — the dealer's left, the viewer's right — so
/// x decreases with seat index.
pub fn seat_places(seats: usize) -> Vec<SeatPlace> {
    (0..seats)
        .map(|i| {
            let angle = if seats <= 1 {
                0.0
            } else {
                SEAT_SPREAD - (2.0 * SEAT_SPREAD) * (i as f64) / ((seats - 1) as f64)
            };
            let rad = angle.to_radians();
            SeatPlace {
                x: ARC_CX + SEAT_RING_R * rad.sin(),
                y: ARC_CY + SEAT_RING_R * rad.cos(),
                angle,
                tilt: -angle,
            }
        })
        .collect()
}

/// An SVG arc path around the shared focus from `from` to `to` degrees
/// (arc angles, `from < to`), bulging toward the players.
pub fn arc_path(r: f64, from_deg: f64, to_deg: f64) -> String {
    let point = |deg: f64| {
        let rad = deg.to_radians();
        (ARC_CX + r * rad.sin(), ARC_CY + r * rad.cos())
    };
    let (x1, y1) = point(from_deg);
    let (x2, y2) = point(to_deg);
    format!("M {x1:.2} {y1:.2} A {r:.2} {r:.2} 0 0 0 {x2:.2} {y2:.2}")
}

/// The felt (or rail) outline: a flat dealer edge on top closed by a
/// half-ellipse bowing toward the players. `inset` shrinks the shape
/// uniformly for nested outlines (rail piping, aprons).
pub fn table_outline(inset: f64) -> String {
    let top = 40.0 + inset;
    let left = 60.0 + inset;
    let right = 1540.0 - inset;
    let rx = (right - left) / 2.0;
    let ry = (960.0 - inset) - top;
    format!(
        "M {left:.2} {top:.2} L {right:.2} {top:.2} A {rx:.2} {ry:.2} 0 0 1 {left:.2} {top:.2} Z"
    )
}

/// The corner-index label for a rank, as printed on a real card.
pub fn rank_label(rank: Rank) -> &'static str {
    match rank {
        Rank::Two => "2",
        Rank::Three => "3",
        Rank::Four => "4",
        Rank::Five => "5",
        Rank::Six => "6",
        Rank::Seven => "7",
        Rank::Eight => "8",
        Rank::Nine => "9",
        Rank::Ten => "10",
        Rank::Jack => "J",
        Rank::Queen => "Q",
        Rank::King => "K",
        Rank::Ace => "A",
    }
}

/// The suit's glyph.
pub fn suit_glyph(suit: Suit) -> &'static str {
    match suit {
        Suit::Clubs => "\u{2663}",
        Suit::Diamonds => "\u{2666}",
        Suit::Hearts => "\u{2665}",
        Suit::Spades => "\u{2660}",
    }
}

/// The ink color for a suit: hearts and diamonds red, clubs and spades
/// black.
pub fn suit_color(suit: Suit) -> &'static str {
    match suit {
        Suit::Hearts | Suit::Diamonds => "#b3232f",
        Suit::Clubs | Suit::Spades => "#1d1f27",
    }
}

/// Pip centers for a rank, in coordinates normalized to the card's pip
/// area (`0..1` across, `0..1` down). Courts return an empty slice —
/// they render as ornamental plates, not pips. The ace's single pip is
/// oversized by [`CardFace`](super::CardFace), not here.
pub fn pip_layout(rank: Rank) -> &'static [(f64, f64)] {
    const L: f64 = 0.18;
    const C: f64 = 0.5;
    const R: f64 = 0.82;
    match rank {
        Rank::Ace => &[(C, 0.5)],
        Rank::Two => &[(C, 0.08), (C, 0.92)],
        Rank::Three => &[(C, 0.08), (C, 0.5), (C, 0.92)],
        Rank::Four => &[(L, 0.08), (R, 0.08), (L, 0.92), (R, 0.92)],
        Rank::Five => &[(L, 0.08), (R, 0.08), (C, 0.5), (L, 0.92), (R, 0.92)],
        Rank::Six => &[
            (L, 0.08),
            (R, 0.08),
            (L, 0.5),
            (R, 0.5),
            (L, 0.92),
            (R, 0.92),
        ],
        Rank::Seven => &[
            (L, 0.08),
            (R, 0.08),
            (C, 0.29),
            (L, 0.5),
            (R, 0.5),
            (L, 0.92),
            (R, 0.92),
        ],
        Rank::Eight => &[
            (L, 0.08),
            (R, 0.08),
            (C, 0.29),
            (L, 0.5),
            (R, 0.5),
            (C, 0.71),
            (L, 0.92),
            (R, 0.92),
        ],
        Rank::Nine => &[
            (L, 0.08),
            (R, 0.08),
            (L, 0.36),
            (R, 0.36),
            (C, 0.5),
            (L, 0.64),
            (R, 0.64),
            (L, 0.92),
            (R, 0.92),
        ],
        Rank::Ten => &[
            (L, 0.08),
            (R, 0.08),
            (C, 0.22),
            (L, 0.36),
            (R, 0.36),
            (L, 0.64),
            (R, 0.64),
            (C, 0.78),
            (L, 0.92),
            (R, 0.92),
        ],
        Rank::Jack | Rank::Queen | Rank::King => &[],
    }
}

/// True for the three court ranks, which render as ornamental plates.
pub fn is_court(rank: Rank) -> bool {
    matches!(rank, Rank::Jack | Rank::Queen | Rank::King)
}

/// The paint scheme for one chip denomination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipStyle {
    /// Top-face and visible-rim body color.
    pub body: &'static str,
    /// Shaded underside of the chip cylinder.
    pub rim: &'static str,
    /// Edge-stripe and inner-ring color.
    pub stripe: &'static str,
    /// Center inlay color.
    pub inlay: &'static str,
}

/// The standard casino paint for a denomination, keyed off the engine's
/// [`ChipColor`] so the mapping can never drift from the core crate.
pub fn chip_style(denomination: Denomination) -> ChipStyle {
    match denomination.color() {
        ChipColor::White => ChipStyle {
            body: "#e9e5da",
            rim: "#b5b0a1",
            stripe: "#3f6fae",
            inlay: "#f5f2ea",
        },
        ChipColor::Red => ChipStyle {
            body: "#b32b2b",
            rim: "#7c1b1b",
            stripe: "#f0e8d8",
            inlay: "#c2403a",
        },
        ChipColor::Green => ChipStyle {
            body: "#1f7a48",
            rim: "#124e2d",
            stripe: "#f0e8d8",
            inlay: "#2b8f59",
        },
        ChipColor::Black => ChipStyle {
            body: "#2b2d33",
            rim: "#141519",
            stripe: "#d8b95c",
            inlay: "#3a3d45",
        },
        ChipColor::Purple => ChipStyle {
            body: "#5f3d8f",
            rim: "#3c265c",
            stripe: "#f0e8d8",
            inlay: "#7350a5",
        },
    }
}

/// The chips of a stack in pile order: bottom of the pile first, largest
/// denominations on the bottom, exactly as a dealer cuts a bet.
pub fn pile_chips(stack: &ChipStack) -> Vec<Denomination> {
    let mut chips: Vec<Denomination> = stack.chips().collect();
    chips.reverse();
    chips
}

/// Split a stack into rack piles: per denomination, largest first, no
/// pile taller than `cap` chips. Ready for #12 to feed real racks.
pub fn rack_piles(stack: &ChipStack, cap: u32) -> Vec<(Denomination, u32)> {
    assert!(cap > 0, "rack piles need a positive height cap");
    let mut piles = Vec::new();
    for denomination in Denomination::ALL.into_iter().rev() {
        let mut remaining = stack.count(denomination);
        while remaining > 0 {
            let take = remaining.min(cap);
            piles.push((denomination, take));
            remaining -= take;
        }
    }
    piles
}

/// How full the shoe is, `0.0..=1.0` — the wedge of undealt cards a
/// counter reads penetration from.
pub fn shoe_fill(shoe: &ShoeStatus) -> f64 {
    let total = shoe.cards_remaining + shoe.cards_dealt;
    if total == 0 {
        0.0
    } else {
        shoe.cards_remaining as f64 / total as f64
    }
}

/// How full the discard tray is, `0.0..=1.0`, relative to the whole
/// shoe. Mid-round this plus [`shoe_fill`] is under `1.0` — the
/// difference is on the felt.
pub fn discard_fill(shoe: &ShoeStatus) -> f64 {
    let total = shoe.cards_remaining + shoe.cards_dealt;
    if total == 0 {
        0.0
    } else {
        shoe.discard_pile_size as f64 / total as f64
    }
}

/// The placard's lines, top to bottom, every one derived from the posted
/// [`Rules`] — nothing here is ever hardcoded in the view.
pub fn placard_lines(rules: &Rules) -> [String; 6] {
    [
        match rules.blackjack_payout {
            BlackjackPayout::ThreeToTwo => "BLACKJACK PAYS 3 TO 2",
            BlackjackPayout::SixToFive => "BLACKJACK PAYS 6 TO 5",
        }
        .to_string(),
        match rules.soft_17 {
            Soft17::Stand => "DEALER STANDS ON SOFT 17",
            Soft17::Hit => "DEALER HITS SOFT 17",
        }
        .to_string(),
        format!("${} MIN \u{2014} ${} MAX", rules.min_bet, rules.max_bet),
        if rules.double_on_any_two {
            "DOUBLE ON ANY TWO CARDS"
        } else {
            "DOUBLE ON 9 \u{00b7} 10 \u{00b7} 11 ONLY"
        }
        .to_string(),
        if rules.double_after_split {
            "DOUBLE AFTER SPLIT"
        } else {
            "NO DOUBLE AFTER SPLIT"
        }
        .to_string(),
        if rules.late_surrender {
            "LATE SURRENDER"
        } else {
            "NO SURRENDER"
        }
        .to_string(),
    ]
}

/// Deterministic scatter for the cards of a busted hand: `(dx, dy,
/// rotation degrees)` for the card at `index`. Pure data so motion (#10)
/// and tests agree on it.
pub fn bust_jitter(index: usize) -> (f64, f64, f64) {
    const JITTER: [(f64, f64, f64); 6] = [
        (-4.0, 3.0, -9.0),
        (5.0, -2.0, 7.0),
        (-3.0, -4.0, -5.0),
        (6.0, 2.0, 11.0),
        (-5.0, 1.0, -7.0),
        (3.0, 4.0, 6.0),
    ];
    JITTER[index % JITTER.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackjack_core::Rank;

    #[test]
    fn seat_places_run_right_to_left_and_stay_on_the_felt() {
        let places = seat_places(7);
        assert_eq!(places.len(), 7);
        // Seat 0 is first base: the viewer's right.
        for pair in places.windows(2) {
            assert!(pair[0].x > pair[1].x);
        }
        // Symmetric about the table center line.
        assert!((places[3].x - ARC_CX).abs() < 1e-9);
        assert!((places[0].x - ARC_CX + (places[6].x - ARC_CX)).abs() < 1e-6);
        // Every circle fits inside the viewBox with margin for the rail.
        for place in &places {
            assert!(place.x > 100.0 && place.x < VIEW_W - 100.0, "{place:?}");
            assert!(place.y > 400.0 && place.y < 900.0, "{place:?}");
            // Hands tilt to face the dealer: exactly against the seat
            // angle, so the seat frame's -y axis is the radial line.
            assert!((place.tilt + place.angle).abs() < 1e-9);
        }
    }

    #[test]
    fn a_single_seat_sits_dead_center() {
        let places = seat_places(1);
        assert_eq!(places.len(), 1);
        assert!((places[0].x - ARC_CX).abs() < 1e-9);
        assert!((places[0].angle).abs() < 1e-9);
    }

    #[test]
    fn arc_paths_span_their_endpoints() {
        let path = arc_path(420.0, -55.0, 55.0);
        assert!(path.starts_with("M "));
        assert!(path.contains("A 420.00 420.00"));
        // Endpoints are symmetric about the center line.
        let x1: f64 = path.split_whitespace().nth(1).unwrap().parse().unwrap();
        let x2: f64 = path
            .split_whitespace()
            .rev()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        assert!((x1 - ARC_CX + (x2 - ARC_CX)).abs() < 0.05);
        assert!(x1 < x2, "insurance lettering must run left to right");
    }

    #[test]
    fn pip_counts_match_rank_values() {
        for rank in Rank::ALL {
            let pips = pip_layout(rank);
            let expected = match rank {
                Rank::Ace => 1,
                Rank::Jack | Rank::Queen | Rank::King => 0,
                other => other.value() as usize,
            };
            assert_eq!(pips.len(), expected, "{rank:?}");
            for &(x, y) in pips {
                assert!((0.0..=1.0).contains(&x), "{rank:?} pip x {x}");
                assert!((0.0..=1.0).contains(&y), "{rank:?} pip y {y}");
            }
        }
    }

    #[test]
    fn courts_are_exactly_jack_queen_king() {
        let courts: Vec<Rank> = Rank::ALL.into_iter().filter(|&r| is_court(r)).collect();
        assert_eq!(courts, [Rank::Jack, Rank::Queen, Rank::King]);
    }

    #[test]
    fn rank_labels_match_card_printing() {
        assert_eq!(rank_label(Rank::Ace), "A");
        assert_eq!(rank_label(Rank::Ten), "10");
        assert_eq!(rank_label(Rank::Two), "2");
        assert_eq!(rank_label(Rank::King), "K");
    }

    #[test]
    fn hearts_and_diamonds_are_red_clubs_and_spades_black() {
        assert_eq!(suit_color(Suit::Hearts), suit_color(Suit::Diamonds));
        assert_eq!(suit_color(Suit::Clubs), suit_color(Suit::Spades));
        assert_ne!(suit_color(Suit::Hearts), suit_color(Suit::Spades));
        let glyphs: Vec<&str> = Suit::ALL.into_iter().map(suit_glyph).collect();
        assert_eq!(glyphs, ["\u{2663}", "\u{2666}", "\u{2665}", "\u{2660}"]);
    }

    #[test]
    fn chip_styles_are_distinct_per_denomination() {
        let bodies: Vec<&str> = Denomination::ALL
            .into_iter()
            .map(|d| chip_style(d).body)
            .collect();
        for (i, a) in bodies.iter().enumerate() {
            for b in &bodies[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn bet_piles_put_big_chips_on_the_bottom() {
        // $85 cuts as 3 green over 2 red — greens on the bottom.
        let pile = pile_chips(&ChipStack::change(85));
        assert_eq!(
            pile,
            [
                Denomination::TwentyFive,
                Denomination::TwentyFive,
                Denomination::TwentyFive,
                Denomination::Five,
                Denomination::Five,
            ]
        );
        assert_eq!(pile.len() as u32, ChipStack::change(85).chip_count());
        assert!(pile_chips(&ChipStack::new()).is_empty());
    }

    #[test]
    fn rack_piles_split_tall_stacks() {
        let mut stack = ChipStack::new();
        stack.add_chips(Denomination::Five, 25);
        stack.add_chips(Denomination::Hundred, 3);
        let piles = rack_piles(&stack, 10);
        assert_eq!(
            piles,
            [
                (Denomination::Hundred, 3),
                (Denomination::Five, 10),
                (Denomination::Five, 10),
                (Denomination::Five, 5),
            ]
        );
        let total: u32 = piles.iter().map(|&(_, n)| n).sum();
        assert_eq!(total, stack.chip_count());
    }

    #[test]
    fn shoe_depth_tracks_the_burn_down() {
        let fresh = ShoeStatus {
            cards_dealt: 0,
            cards_remaining: 312,
            discard_pile_size: 0,
            cut_card_reached: false,
        };
        assert!((shoe_fill(&fresh) - 1.0).abs() < 1e-9);
        assert!(discard_fill(&fresh).abs() < 1e-9);

        // Mid-round: 12 cards on the felt, 88 in the discard.
        let mid = ShoeStatus {
            cards_dealt: 100,
            cards_remaining: 212,
            discard_pile_size: 88,
            cut_card_reached: false,
        };
        assert!((shoe_fill(&mid) - 212.0 / 312.0).abs() < 1e-9);
        assert!((discard_fill(&mid) - 88.0 / 312.0).abs() < 1e-9);
        assert!(shoe_fill(&mid) + discard_fill(&mid) < 1.0);

        let empty = ShoeStatus {
            cards_dealt: 0,
            cards_remaining: 0,
            discard_pile_size: 0,
            cut_card_reached: false,
        };
        assert_eq!(shoe_fill(&empty), 0.0);
        assert_eq!(discard_fill(&empty), 0.0);
    }

    #[test]
    fn placard_reads_the_canonical_rules() {
        let lines = placard_lines(&Rules::canonical());
        assert_eq!(
            lines,
            [
                "BLACKJACK PAYS 3 TO 2",
                "DEALER STANDS ON SOFT 17",
                "$10 MIN \u{2014} $500 MAX",
                "DOUBLE ON ANY TWO CARDS",
                "DOUBLE AFTER SPLIT",
                "LATE SURRENDER",
            ]
        );
    }

    #[test]
    fn placard_reads_a_stingy_rule_set() {
        let rules = Rules {
            blackjack_payout: BlackjackPayout::SixToFive,
            soft_17: Soft17::Hit,
            double_on_any_two: false,
            double_after_split: false,
            late_surrender: false,
            min_bet: 25,
            max_bet: 1000,
            ..Rules::canonical()
        };
        let lines = placard_lines(&rules);
        assert_eq!(
            lines,
            [
                "BLACKJACK PAYS 6 TO 5",
                "DEALER HITS SOFT 17",
                "$25 MIN \u{2014} $1000 MAX",
                "DOUBLE ON 9 \u{00b7} 10 \u{00b7} 11 ONLY",
                "NO DOUBLE AFTER SPLIT",
                "NO SURRENDER",
            ]
        );
    }

    #[test]
    fn bust_jitter_is_deterministic_and_subtle() {
        for i in 0..12 {
            let (dx, dy, rot) = bust_jitter(i);
            assert_eq!(bust_jitter(i), bust_jitter(i + 6));
            assert!(dx.abs() <= 8.0 && dy.abs() <= 8.0 && rot.abs() <= 12.0);
        }
        // Neighboring cards lean opposite ways.
        assert!(bust_jitter(0).2 * bust_jitter(1).2 < 0.0);
    }

    #[test]
    fn table_outline_nests_with_inset() {
        let outer = table_outline(0.0);
        assert!(outer.starts_with("M 60.00 40.00 L 1540.00 40.00"));
        let inner = table_outline(20.0);
        assert!(inner.starts_with("M 80.00 60.00 L 1520.00 60.00"));
    }
}
