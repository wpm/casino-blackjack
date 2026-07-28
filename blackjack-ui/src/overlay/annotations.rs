//! Pure layout and wording for the help overlay's chalk annotations.
//!
//! Everything here is plain data derived from the gesture context, the
//! posted [`Rules`], and the zone constants in [`crate::input::geometry`]
//! — no DOM, no signals — so it all tests on the host. The rendering in
//! [`super::help`] is a thin view over these tables, exactly as the
//! scene components are thin views over [`crate::scene::geometry`].
//!
//! Each [`HelpNote`] is anchored inside the felt zone it describes (in
//! the human's seat frame or in global scene coordinates) and knows
//! whether it should be dimmed — an annotation for a gesture that is
//! meaningless right now dims but never vanishes, so the vocabulary
//! always reads as a whole.
//!
//! The lettering itself sits at a separate [`HelpNote::label`] point,
//! laddered into open felt around the crowded center seat; when a label
//! is far from its zone, the renderer draws a dashed chalk leader from
//! the lettering back to the [`HelpNote::anchor`]. Keeping the two
//! points separate is what lets every zone stay truthfully outlined
//! while the words spread out far enough to stay legible.

use blackjack_core::{ActionKind, Phase, Rules, Soft17};

use crate::input::geometry::{
    BEHIND_ZONE_Y_MAX, BEHIND_ZONE_Y_MIN, DOUBLE_ZONE_X, HAND_ZONE_Y_FAR, HAND_ZONE_Y_NEAR,
    RACK_REGION_TOP, RACK_X, SPLIT_ZONE_Y,
};
use crate::input::gesture::GestureCtx;
use crate::scene::geometry::{ARC_CX, ARC_CY, INSURANCE_R_OUTER};

/// Where a note's lettering is anchored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HelpAnchor {
    /// In the human's seat frame: origin at the betting-circle center,
    /// `-y` up the radial line toward the dealer (see
    /// [`crate::input::geometry`]).
    Seat {
        /// Seat-frame x.
        x: f64,
        /// Seat-frame y.
        y: f64,
    },
    /// In global scene (viewBox) coordinates.
    Global {
        /// Scene x.
        x: f64,
        /// Scene y.
        y: f64,
    },
}

/// One chalk annotation, ready to letter onto the glass.
#[derive(Debug, Clone, PartialEq)]
pub struct HelpNote {
    /// Stable identifier, for tests and keyed rendering.
    pub id: &'static str,
    /// The short chalk headline.
    pub title: &'static str,
    /// The one-line explanation under the headline.
    pub detail: &'static str,
    /// The point inside the felt zone this note describes — the leader
    /// line's target, always within the zone's own bounds.
    pub anchor: HelpAnchor,
    /// Where the lettering sits: open felt near the zone, chosen so no
    /// two labels collide. Same coordinate frame as `anchor`.
    pub label: HelpAnchor,
    /// A second detail line, for notes too wide for one (the key row).
    pub detail2: Option<&'static str>,
    /// Whether the described action is meaningless right now.
    pub dimmed: bool,
}

/// First line of the keyboard cheat rows, chalked along the top rail.
pub const KEY_ROW_1: &str =
    "H hit \u{b7} S stand \u{b7} D double \u{b7} P split \u{b7} R surrender";
/// Second line of the keyboard cheat rows.
pub const KEY_ROW_2: &str = "Y/N insurance \u{b7} Enter post \u{b7} W walk away";

/// Left edge of the placard-explanations block, scene x.
pub const PLACARD_NOTES_X: f64 = 120.0;
/// Baseline of the block's first line, scene y.
pub const PLACARD_NOTES_Y: f64 = 388.0;
/// Leading between the block's lines.
pub const PLACARD_NOTES_LEADING: f64 = 46.0;

/// Whether a hand-signal annotation (`kind`) is live right now: the
/// human's turn and the action legal — the same gate the recognizers
/// apply, read for lighting instead of acting.
fn hand_live(ctx: &GestureCtx, kind: ActionKind) -> bool {
    ctx.phase == Phase::PlayerTurn && ctx.human_active && ctx.allows(kind)
}

/// The full annotation set for the current context, in lettering order.
///
/// Anchors are derived from the zone constants in
/// [`crate::input::geometry`], so the chalk always sits on the exact
/// felt regions the recognizers read. Dimming derives from the
/// snapshot's `legal_actions` (through [`GestureCtx`]): a note is lit
/// only while its gesture could actually do something.
pub fn help_notes(ctx: &GestureCtx) -> Vec<HelpNote> {
    let betting_open = ctx.phase == Phase::Betting && ctx.allows(ActionKind::PlaceBet);
    let insurance_open = ctx.phase == Phase::InsuranceOffer
        && ctx.human_active
        && ctx.allows(ActionKind::TakeInsurance);
    let between_rounds = matches!(ctx.phase, Phase::Betting | Phase::RoundOver);
    // (id, title, detail(s), zone anchor, lettering label, dimmed).
    // Labels ladder into open felt: hit/double/split up the right side,
    // stand/surrender/rack up the left, bet and insurance on the center
    // line, the key rows along the top rail. Wording stays terse — the
    // lettering is sized to be read across the table.
    type Row = (
        &'static str,
        &'static str,
        &'static str,
        Option<&'static str>,
        HelpAnchor,
        HelpAnchor,
        bool,
    );
    let raw: [Row; 9] = [
        (
            "bet-circle",
            "BETTING CIRCLE",
            "Drag chips in; they deal.",
            None,
            HelpAnchor::Seat { x: 0.0, y: 0.0 },
            HelpAnchor::Seat {
                x: -60.0,
                y: -115.0,
            },
            !betting_open,
        ),
        (
            "hit",
            "TAP FOR A CARD",
            "Tap here for one more card.",
            None,
            HelpAnchor::Seat {
                x: 0.0,
                y: HAND_ZONE_Y_FAR + 60.0,
            },
            HelpAnchor::Seat {
                x: 365.0,
                y: -385.0,
            },
            !hand_live(ctx, ActionKind::Hit),
        ),
        (
            "stand",
            "WAVE IT OFF",
            "Sweep here to stand.",
            None,
            HelpAnchor::Seat {
                x: 0.0,
                y: HAND_ZONE_Y_NEAR - 60.0,
            },
            HelpAnchor::Seat {
                x: -520.0,
                y: -205.0,
            },
            !hand_live(ctx, ActionKind::Stand),
        ),
        (
            "double",
            "CHIPS BESIDE",
            "One chip here doubles.",
            None,
            HelpAnchor::Seat {
                x: DOUBLE_ZONE_X,
                y: 0.0,
            },
            HelpAnchor::Seat { x: 430.0, y: -10.0 },
            !hand_live(ctx, ActionKind::Double),
        ),
        (
            "split",
            "CHIPS BEHIND",
            "A chip here splits a pair.",
            None,
            HelpAnchor::Seat {
                x: 0.0,
                y: SPLIT_ZONE_Y,
            },
            HelpAnchor::Seat { x: 430.0, y: 115.0 },
            !hand_live(ctx, ActionKind::Split),
        ),
        (
            "surrender",
            "DRAW THE LINE",
            "A bare line surrenders.",
            None,
            HelpAnchor::Seat {
                x: -96.0,
                y: (BEHIND_ZONE_Y_MIN + BEHIND_ZONE_Y_MAX) / 2.0,
            },
            HelpAnchor::Seat { x: -440.0, y: 5.0 },
            !hand_live(ctx, ActionKind::Surrender),
        ),
        (
            "insurance",
            "INSURANCE",
            "Chips here insure, 2 to 1.",
            None,
            HelpAnchor::Global {
                x: ARC_CX,
                y: ARC_CY + INSURANCE_R_OUTER,
            },
            HelpAnchor::Global {
                x: 1165.0,
                y: 545.0,
            },
            !insurance_open,
        ),
        (
            "rack-leave",
            "YOUR RACK",
            "Drag off the felt to leave.",
            None,
            HelpAnchor::Global {
                x: RACK_X,
                y: RACK_REGION_TOP + 22.0,
            },
            HelpAnchor::Global { x: 390.0, y: 915.0 },
            !between_rounds,
        ),
        (
            "keys",
            "KEYS",
            KEY_ROW_1,
            Some(KEY_ROW_2),
            HelpAnchor::Global {
                x: RACK_X,
                y: RACK_REGION_TOP - 46.0,
            },
            HelpAnchor::Global { x: RACK_X, y: 65.0 },
            false,
        ),
    ];
    raw.into_iter()
        .map(
            |(id, title, detail, detail2, anchor, label, dimmed)| HelpNote {
                id,
                title,
                detail,
                detail2,
                anchor,
                label,
                dimmed,
            },
        )
        .collect()
}

/// The placard conventions, explained in plain language — one terse
/// line each, every word derived from the posted [`Rules`]. Order:
/// soft 17, double after split, surrender, insurance.
pub fn placard_notes(rules: &Rules) -> [String; 4] {
    [
        match rules.soft_17 {
            Soft17::Stand => "Soft 17: the dealer stands on ace-6.",
            Soft17::Hit => "Soft 17: the dealer hits ace-6.",
        }
        .to_string(),
        if rules.double_after_split {
            "You may double after splitting."
        } else {
            "No doubling after a split."
        }
        .to_string(),
        if rules.late_surrender {
            "Surrender after the peek: keep half."
        } else {
            "No surrender at this table."
        }
        .to_string(),
        if rules.insurance_offered {
            "Insurance: 2 to 1 the dealer has it."
        } else {
            "No insurance at this table."
        }
        .to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::geometry::{
        BAND_HALF_W, BET_ZONE_R, Point, RACK_HALF_W, SIDE_ZONE_R, in_behind_band, in_hand_zone,
        in_insurance_band, in_rack_region,
    };
    use crate::input::gesture::test_ctx;
    use crate::scene::geometry::SeatPlace;

    /// A note's label point in global scene coordinates.
    fn label_point(note: &HelpNote, place: SeatPlace) -> Point {
        match note.label {
            HelpAnchor::Seat { x, y } => {
                let rad = place.tilt.to_radians();
                Point::new(
                    place.x + x * rad.cos() - y * rad.sin(),
                    place.y + x * rad.sin() + y * rad.cos(),
                )
            }
            HelpAnchor::Global { x, y } => Point::new(x, y),
        }
    }

    /// Conservative bounding box for a note's chalk lettering: centered
    /// title (44px, wide letter-spacing) over centered detail lines
    /// (38px, one or two of them).
    fn label_box(note: &HelpNote, place: SeatPlace) -> LabelBox {
        let p = label_point(note, place);
        let width = (note.title.len() as f64 * 32.0)
            .max(note.detail.len() as f64 * 20.0)
            .max(note.detail2.map_or(0.0, |d| d.len() as f64 * 20.0));
        let bottom = if note.detail2.is_some() { 164.0 } else { 80.0 };
        (
            p.x - width / 2.0,
            p.y - 35.0,
            p.x + width / 2.0,
            p.y + bottom,
        )
    }

    /// A lettering bounding box: (left, top, right, bottom).
    type LabelBox = (f64, f64, f64, f64);

    fn boxes_overlap(a: LabelBox, b: LabelBox) -> bool {
        a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
    }

    fn seat_anchor(note: &HelpNote) -> Point {
        match note.anchor {
            HelpAnchor::Seat { x, y } => Point::new(x, y),
            HelpAnchor::Global { .. } => panic!("{} should anchor in the seat frame", note.id),
        }
    }

    fn global_anchor(note: &HelpNote) -> Point {
        match note.anchor {
            HelpAnchor::Global { x, y } => Point::new(x, y),
            HelpAnchor::Seat { .. } => panic!("{} should anchor globally", note.id),
        }
    }

    fn note<'a>(notes: &'a [HelpNote], id: &str) -> &'a HelpNote {
        notes
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("missing note {id}"))
    }

    #[test]
    fn notes_cover_the_whole_gesture_vocabulary() {
        let notes = help_notes(&test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false));
        let ids: Vec<&str> = notes.iter().map(|n| n.id).collect();
        assert_eq!(
            ids,
            [
                "bet-circle",
                "hit",
                "stand",
                "double",
                "split",
                "surrender",
                "insurance",
                "rack-leave",
                "keys",
            ]
        );
        for n in &notes {
            assert!(!n.title.is_empty() && !n.detail.is_empty(), "{}", n.id);
        }
    }

    #[test]
    fn every_note_sits_in_the_zone_it_describes() {
        let notes = help_notes(&test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false));
        let origin = Point::new(0.0, 0.0);
        // The bet note letters onto the circle itself.
        assert!(seat_anchor(note(&notes, "bet-circle")).distance(origin) <= BET_ZONE_R);
        // Hit and stand letter inside the hit/stand band, apart.
        let hit = seat_anchor(note(&notes, "hit"));
        let stand = seat_anchor(note(&notes, "stand"));
        assert!(in_hand_zone(hit) && in_hand_zone(stand));
        assert!(hit.distance(stand) > 80.0);
        // Double and split letter inside their drop zones.
        let double = seat_anchor(note(&notes, "double"));
        assert!(double.distance(Point::new(DOUBLE_ZONE_X, 0.0)) <= SIDE_ZONE_R);
        let split = seat_anchor(note(&notes, "split"));
        assert!(split.distance(Point::new(0.0, SPLIT_ZONE_Y)) <= SIDE_ZONE_R);
        // Surrender letters in the behind band but off the split zone.
        let surrender = seat_anchor(note(&notes, "surrender"));
        assert!(in_behind_band(surrender));
        assert!(surrender.x.abs() <= BAND_HALF_W);
        assert!(surrender.distance(Point::new(0.0, SPLIT_ZONE_Y)) > SIDE_ZONE_R);
        // Insurance letters on the printed band.
        assert!(in_insurance_band(global_anchor(note(&notes, "insurance"))));
        // The rack note letters inside the rack's pointer region; the
        // key cheat row chalks just above it on the rail.
        assert!(in_rack_region(global_anchor(note(&notes, "rack-leave"))));
        let keys = global_anchor(note(&notes, "keys"));
        assert!((keys.x - RACK_X).abs() <= RACK_HALF_W);
        assert!((RACK_REGION_TOP - 80.0..RACK_REGION_TOP).contains(&keys.y));
    }

    #[test]
    fn labels_never_collide_on_the_canonical_table() {
        // Betting lights the densest note set, and the canonical
        // seven-seat table puts the human dead center where crowding is
        // worst — this is the layout that regressed once before.
        let ctx = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        let notes = help_notes(&ctx);
        let boxes: Vec<(&str, LabelBox)> = notes
            .iter()
            .map(|n| (n.id, label_box(n, ctx.place)))
            .collect();
        // The placard explainer block's own footprint (left-aligned
        // lines up to ~110 chars at 12.5px, plus its headline above).
        let placard = (
            PLACARD_NOTES_X - 10.0,
            PLACARD_NOTES_Y - 106.0,
            PLACARD_NOTES_X + 700.0,
            PLACARD_NOTES_Y + 3.0 * PLACARD_NOTES_LEADING + 12.0,
        );
        for (i, (id_a, a)) in boxes.iter().enumerate() {
            assert!(
                a.0 >= 8.0 && a.2 <= 1592.0 && a.1 >= 30.0 && a.3 <= 998.0,
                "{id_a} lettering leaves the glass: {a:?}"
            );
            assert!(
                !boxes_overlap(*a, placard),
                "{id_a} collides with the placard explainer: {a:?}"
            );
            for (id_b, b) in boxes.iter().skip(i + 1) {
                assert!(
                    !boxes_overlap(*a, *b),
                    "{id_a} collides with {id_b}: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn dimming_follows_legal_actions_during_the_player_turn() {
        let legal = [ActionKind::Hit, ActionKind::Stand, ActionKind::Double];
        let notes = help_notes(&test_ctx(Phase::PlayerTurn, &legal, true));
        for (id, dimmed) in [
            ("hit", false),
            ("stand", false),
            ("double", false),
            // No pair, so splitting is not legal: dimmed, not gone.
            ("split", true),
            ("surrender", true),
            ("bet-circle", true),
            ("insurance", true),
            ("rack-leave", true),
            ("keys", false),
        ] {
            assert_eq!(note(&notes, id).dimmed, dimmed, "{id}");
        }
        // Not the human's turn: every hand signal dims.
        let idle = help_notes(&test_ctx(Phase::PlayerTurn, &legal, false));
        for id in ["hit", "stand", "double", "split", "surrender"] {
            assert!(note(&idle, id).dimmed, "{id}");
        }
    }

    #[test]
    fn dimming_in_betting_and_insurance_phases() {
        let betting = help_notes(&test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false));
        assert!(!note(&betting, "bet-circle").dimmed);
        assert!(!note(&betting, "rack-leave").dimmed);
        assert!(note(&betting, "hit").dimmed);
        assert!(note(&betting, "insurance").dimmed);

        let legal = [ActionKind::TakeInsurance, ActionKind::DeclineInsurance];
        let offer = help_notes(&test_ctx(Phase::InsuranceOffer, &legal, true));
        assert!(!note(&offer, "insurance").dimmed);
        assert!(note(&offer, "bet-circle").dimmed);
        // The offer is open but not the human's: dimmed.
        let idle = help_notes(&test_ctx(Phase::InsuranceOffer, &legal, false));
        assert!(note(&idle, "insurance").dimmed);

        let over = help_notes(&test_ctx(Phase::RoundOver, &[ActionKind::NextRound], false));
        assert!(!note(&over, "rack-leave").dimmed);
    }

    #[test]
    fn placard_notes_read_the_posted_rules() {
        let generous = placard_notes(&Rules::canonical());
        assert!(generous[0].contains("stands on ace-6"));
        assert!(generous[1].contains("double after splitting"));
        assert!(generous[2].contains("Surrender after the peek"));
        assert!(generous[3].contains("2 to 1"));

        let stingy = placard_notes(&Rules {
            soft_17: Soft17::Hit,
            double_after_split: false,
            late_surrender: false,
            insurance_offered: false,
            ..Rules::canonical()
        });
        assert!(stingy[0].contains("hits ace-6"));
        assert_ne!(generous[0], stingy[0]);
        assert!(stingy[1].contains("No doubling"));
        assert!(stingy[2].contains("No surrender"));
        assert!(stingy[3].contains("No insurance"));
    }

    #[test]
    fn the_cheat_rows_cover_the_documented_key_map() {
        let rows = format!("{KEY_ROW_1} {KEY_ROW_2}");
        for key in ["H ", "S ", "D ", "P ", "R ", "Y/N", "Enter", "W "] {
            assert!(rows.contains(key), "{key}");
        }
        for word in [
            "hit",
            "stand",
            "double",
            "split",
            "surrender",
            "insurance",
            "post",
            "walk away",
        ] {
            assert!(rows.contains(word), "{word}");
        }
    }
}
