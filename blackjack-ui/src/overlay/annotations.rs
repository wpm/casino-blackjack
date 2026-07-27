//! Pure layout and wording for the help overlay's chalk annotations.
//!
//! Everything here is plain data derived from the gesture context, the
//! posted [`Rules`], and the zone constants in [`crate::input::geometry`]
//! — no DOM, no signals — so it all tests on the host. The rendering in
//! [`super::help`] is a thin view over these tables, exactly as the
//! scene components are thin views over [`crate::scene::geometry`].
//!
//! Each [`HelpNote`] is anchored inside the felt zone it describes (in
//! the human's seat frame or in global scene coordinates), carries a
//! deterministic chalk-lettering tilt, and knows whether it should be
//! dimmed — an annotation for a gesture that is meaningless right now
//! dims but never vanishes, so the vocabulary always reads as a whole.
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
    /// Whether the described action is meaningless right now.
    pub dimmed: bool,
    /// Hand-lettering rotation jitter, degrees.
    pub tilt: f64,
}

/// The keyboard cheat row, chalked near the rack. Mirrors the key map
/// documented in [`crate::input::keyboard`].
pub const KEY_CHEAT_ROW: &str = "H hit \u{b7} S stand \u{b7} D double \u{b7} P split \u{b7} \
                                 R surrender \u{b7} Y/N insurance \u{b7} Enter post \u{b7} \
                                 W walk away";

/// Left edge of the placard-explanations block, scene x.
pub const PLACARD_NOTES_X: f64 = 120.0;
/// Baseline of the block's first line, scene y.
pub const PLACARD_NOTES_Y: f64 = 392.0;
/// Leading between the block's lines.
pub const PLACARD_NOTES_LEADING: f64 = 25.0;

/// Deterministic hand-lettering tilt for the note at `index`: small,
/// alternating, and stable so the chalk never shimmers between renders.
pub fn label_tilt(index: usize) -> f64 {
    const TILTS: [f64; 8] = [-1.8, 1.3, -1.0, 1.9, -1.4, 0.8, -2.1, 1.6];
    TILTS[index % TILTS.len()]
}

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
    // (id, title, detail, zone anchor, lettering label, dimmed). Labels
    // ladder into open felt: hit/double/split up the right side,
    // stand/surrender/rack-leave up the left, bet and insurance on the
    // center line above the circle, keys along the bottom rail.
    let raw: [(&str, &str, &str, HelpAnchor, HelpAnchor, bool); 9] = [
        (
            "bet-circle",
            "BETTING CIRCLE",
            "Drag chips from your rack into the circle; let them rest and the deal begins.",
            HelpAnchor::Seat { x: 0.0, y: 0.0 },
            HelpAnchor::Seat { x: 0.0, y: -115.0 },
            !betting_open,
        ),
        (
            "hit",
            "TAP FOR A CARD",
            "Tap the felt behind your cards to hit \u{2014} one more card.",
            HelpAnchor::Seat {
                x: 0.0,
                y: HAND_ZONE_Y_FAR + 60.0,
            },
            HelpAnchor::Seat {
                x: 330.0,
                y: -335.0,
            },
            !hand_live(ctx, ActionKind::Hit),
        ),
        (
            "stand",
            "WAVE IT OFF",
            "Sweep sideways across your cards to stand \u{2014} no more cards.",
            HelpAnchor::Seat {
                x: 0.0,
                y: HAND_ZONE_Y_NEAR - 60.0,
            },
            HelpAnchor::Seat {
                x: -350.0,
                y: -180.0,
            },
            !hand_live(ctx, ActionKind::Stand),
        ),
        (
            "double",
            "CHIPS BESIDE",
            "Drop a chip beside your bet to double down: bet doubled, exactly one more card.",
            HelpAnchor::Seat {
                x: DOUBLE_ZONE_X,
                y: 0.0,
            },
            HelpAnchor::Seat { x: 345.0, y: -30.0 },
            !hand_live(ctx, ActionKind::Double),
        ),
        (
            "split",
            "CHIPS BEHIND",
            "Drop a chip directly behind your bet to split a pair into two hands.",
            HelpAnchor::Seat {
                x: 0.0,
                y: SPLIT_ZONE_Y,
            },
            HelpAnchor::Seat { x: 345.0, y: 95.0 },
            !hand_live(ctx, ActionKind::Split),
        ),
        (
            "surrender",
            "DRAW THE LINE",
            "Drag a chip-free line behind your bet to surrender: half the bet back.",
            HelpAnchor::Seat {
                x: -96.0,
                y: (BEHIND_ZONE_Y_MIN + BEHIND_ZONE_Y_MAX) / 2.0,
            },
            HelpAnchor::Seat {
                x: -345.0,
                y: -30.0,
            },
            !hand_live(ctx, ActionKind::Surrender),
        ),
        (
            "insurance",
            "THE INSURANCE LINE",
            "A side bet that the dealer has blackjack \u{2014} drop chips on the line to take it.",
            HelpAnchor::Global {
                x: ARC_CX,
                y: ARC_CY + INSURANCE_R_OUTER,
            },
            HelpAnchor::Global {
                x: ARC_CX,
                y: ARC_CY + INSURANCE_R_OUTER,
            },
            !insurance_open,
        ),
        (
            "rack-leave",
            "YOUR RACK",
            "Drag the rack down off the felt to rack up and walk away.",
            HelpAnchor::Global {
                x: RACK_X,
                y: RACK_REGION_TOP + 22.0,
            },
            HelpAnchor::Global { x: 455.0, y: 885.0 },
            !between_rounds,
        ),
        (
            "keys",
            "KEYS",
            KEY_CHEAT_ROW,
            HelpAnchor::Global {
                x: RACK_X,
                y: RACK_REGION_TOP - 46.0,
            },
            HelpAnchor::Global {
                x: RACK_X,
                y: 950.0,
            },
            false,
        ),
    ];
    raw.into_iter()
        .enumerate()
        .map(|(i, (id, title, detail, anchor, label, dimmed))| HelpNote {
            id,
            title,
            detail,
            anchor,
            label,
            dimmed,
            tilt: label_tilt(i),
        })
        .collect()
}

/// The placard conventions, explained in plain language — one terse
/// line each, every word derived from the posted [`Rules`]. Order:
/// soft 17, double after split, surrender, insurance.
pub fn placard_notes(rules: &Rules) -> [String; 4] {
    [
        match rules.soft_17 {
            Soft17::Stand => {
                "\u{201c}Stands on soft 17\u{201d}: on ace-6 the dealer must stop \u{2014} \
                 a touch better for you."
            }
            Soft17::Hit => {
                "\u{201c}Hits soft 17\u{201d}: on ace-6 the dealer draws again \u{2014} \
                 a touch worse for you."
            }
        }
        .to_string(),
        if rules.double_after_split {
            "\u{201c}Double after split\u{201d}: you may double down on hands made by \
             splitting a pair."
        } else {
            "No doubling down on hands made by splitting a pair."
        }
        .to_string(),
        if rules.late_surrender {
            "\u{201c}Late surrender\u{201d}: once the dealer checks for blackjack, you may \
             give up your hand for half the bet back."
        } else {
            "No surrender here: every hand plays to the end."
        }
        .to_string(),
        if rules.insurance_offered {
            "Insurance is offered when the dealer shows an ace: a side bet, paying 2 to 1, \
             that the dealer has blackjack."
        } else {
            "This table offers no insurance bet."
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
    /// title (17px, wide letter-spacing) over centered detail (12.5px).
    fn label_box(note: &HelpNote, place: SeatPlace) -> LabelBox {
        let p = label_point(note, place);
        let width = (note.title.len() as f64 * 13.2).max(note.detail.len() as f64 * 6.6);
        (p.x - width / 2.0, p.y - 16.0, p.x + width / 2.0, p.y + 32.0)
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
            PLACARD_NOTES_Y - 45.0,
            PLACARD_NOTES_X + 730.0,
            PLACARD_NOTES_Y + 3.0 * PLACARD_NOTES_LEADING + 10.0,
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
        assert!(generous[0].contains("Stands on soft 17"));
        assert!(generous[0].contains("ace-6"));
        assert!(generous[1].contains("Double after split"));
        assert!(generous[2].contains("Late surrender"));
        assert!(generous[3].contains("ace") && generous[3].contains("2 to 1"));

        let stingy = placard_notes(&Rules {
            soft_17: Soft17::Hit,
            double_after_split: false,
            late_surrender: false,
            insurance_offered: false,
            ..Rules::canonical()
        });
        assert!(stingy[0].contains("Hits soft 17"));
        assert_ne!(generous[0], stingy[0]);
        assert!(stingy[1].contains("No doubling"));
        assert!(stingy[2].contains("No surrender"));
        assert!(stingy[3].contains("no insurance"));
    }

    #[test]
    fn the_cheat_row_covers_the_documented_key_map() {
        for key in ["H ", "S ", "D ", "P ", "R ", "Y/N", "Enter", "W "] {
            assert!(KEY_CHEAT_ROW.contains(key), "{key}");
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
            assert!(KEY_CHEAT_ROW.contains(word), "{word}");
        }
    }

    #[test]
    fn lettering_tilt_is_deterministic_and_subtle() {
        for i in 0..20 {
            assert_eq!(label_tilt(i), label_tilt(i + 8));
            assert!(label_tilt(i).abs() <= 3.0);
            // Neighboring labels lean opposite ways.
            assert!(label_tilt(i) * label_tilt(i + 1) < 0.0);
        }
        let notes = help_notes(&test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false));
        for (i, n) in notes.iter().enumerate() {
            assert_eq!(n.tilt, label_tilt(i), "{}", n.id);
        }
    }
}
