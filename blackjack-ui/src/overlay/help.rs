//! Help mode: the first tenant of the overlay glass.
//!
//! # Activation
//!
//! **Hold** `?` (or `F1`) and the glass rises with every annotation on
//! it; release and it vanishes completely. There is no toggle, no
//! button, no persistent chrome — the table itself never grows
//! tooltips. Because `?` needs Shift, the release may arrive as `/` or
//! `Shift` rather than `?`; [`releases_help_key`] accepts all of them
//! so the glass can never stick. Key repeats are filtered; losing
//! window focus also drops the glass.
//!
//! # Rendering
//!
//! Pure render from `(snapshot, active)`: the annotation set and its
//! dimming come from [`super::annotations`] (host-tested), and this
//! module only letters them onto the glass — chalk-white lines, dashed
//! strokes, a cursive-leaning system font stack. Annotations for
//! gestures that are meaningless in the current phase dim but never
//! vanish.

use blackjack_core::Snapshot;
use leptos::prelude::*;

use super::annotations::{
    CHIP_LEGEND_X, CHIP_LEGEND_Y, HelpAnchor, HelpNote, PLACARD_NOTES_LEADING, PLACARD_NOTES_X,
    PLACARD_NOTES_Y, chip_legend, help_notes, placard_notes,
};
use super::layer::OverlayLayer;
use crate::input::geometry::{
    BAND_HALF_W, BEHIND_ZONE_Y_MAX, BEHIND_ZONE_Y_MIN, BET_ZONE_R, DOUBLE_ZONE_X, HAND_ZONE_Y_FAR,
    HAND_ZONE_Y_NEAR, RACK_HALF_W, RACK_REGION_TOP, RACK_X, SIDE_ZONE_R, SPLIT_ZONE_Y,
};
use crate::input::gesture::{GestureCtx, human_seat};
use crate::scene::ChipView;
use crate::scene::geometry::{INSURANCE_R_INNER, INSURANCE_R_OUTER, SeatPlace, arc_path};

/// Chalk-leaning system font stack — no external fonts, ever.
const CHALK_FONT: &str = "'Chalkboard SE', 'Segoe Print', 'Comic Sans MS', 'Bradley Hand', \
                          cursive";
/// The chalky off-white every line and letter uses.
const CHALK_INK: &str = "#ece7d9";

/// Whether a `KeyboardEvent.key` raises the help glass: `?` or `F1`.
pub fn is_help_key(key: &str) -> bool {
    matches!(key, "?" | "F1")
}

/// Whether releasing this key drops the help glass. `?` is a shifted
/// key, so its keyup may report `/` (Shift already up) or `Shift`
/// (released first); accepting all of them keeps the glass from
/// sticking.
pub fn releases_help_key(key: &str) -> bool {
    matches!(key, "?" | "/" | "F1" | "Shift")
}

/// Dashed chalk outlines tracing every gesture zone the recognizers
/// read, so each annotation visibly belongs to a region of felt.
#[component]
fn ChalkZones(place: SeatPlace) -> impl IntoView {
    let mid = (INSURANCE_R_INNER + INSURANCE_R_OUTER) / 2.0;
    view! {
        <g
            stroke=CHALK_INK
            stroke-width="1.7"
            fill="none"
            opacity="0.4"
            stroke-linecap="round"
        >
            <g transform=format!(
                "translate({:.2} {:.2}) rotate({:.2})",
                place.x,
                place.y,
                place.tilt
            )>
                // The betting circle and its two chip drop zones.
                <circle r=BET_ZONE_R stroke-dasharray="5 8"></circle>
                <circle cx=DOUBLE_ZONE_X cy="0" r=SIDE_ZONE_R stroke-dasharray="4 7"></circle>
                <circle cx="0" cy=SPLIT_ZONE_Y r=SIDE_ZONE_R stroke-dasharray="4 7"></circle>
                // The hit/stand band over the cards.
                <rect
                    x=-BAND_HALF_W
                    y=HAND_ZONE_Y_FAR
                    width=2.0 * BAND_HALF_W
                    height=HAND_ZONE_Y_NEAR - HAND_ZONE_Y_FAR
                    rx="20"
                    stroke-dasharray="7 10"
                ></rect>
                // The surrender/split band behind the bet.
                <rect
                    x=-BAND_HALF_W
                    y=BEHIND_ZONE_Y_MIN
                    width=2.0 * BAND_HALF_W
                    height=BEHIND_ZONE_Y_MAX - BEHIND_ZONE_Y_MIN
                    rx="14"
                    stroke-dasharray="7 10"
                ></rect>
            </g>
            // The insurance band's centerline and the rack's shelf.
            <path d=arc_path(mid, -52.0, 52.0) stroke-dasharray="8 7"></path>
            <line
                x1=RACK_X - RACK_HALF_W
                y1=RACK_REGION_TOP
                x2=RACK_X + RACK_HALF_W
                y2=RACK_REGION_TOP
                stroke-dasharray="6 6"
            ></line>
        </g>
    }
}

/// How far lettering may drift from its zone before the leader line
/// appears to tie the two back together.
const LEADER_MIN_DISTANCE: f64 = 170.0;

/// One chalk annotation: headline, dashed underline, detail lines —
/// lettered at the note's label point, tied back to its felt zone by a
/// dashed leader when the two are far apart — dimmed when its gesture
/// is meaningless right now.
#[component]
fn ChalkNote(note: HelpNote, place: SeatPlace) -> impl IntoView {
    let seat_frame = format!(
        "translate({:.2} {:.2}) rotate({:.2})",
        place.x, place.y, place.tilt
    );
    let (frame, x, y) = match note.label {
        HelpAnchor::Seat { x, y } => (seat_frame.clone(), x, y),
        HelpAnchor::Global { x, y } => (String::new(), x, y),
    };
    // The leader runs label→zone inside the label's own frame; a note's
    // anchor and label always share a frame by construction.
    let (ax, ay) = match note.anchor {
        HelpAnchor::Seat { x, y } | HelpAnchor::Global { x, y } => (x, y),
    };
    let (dx, dy) = (ax - x, ay - y);
    let distance = dx.hypot(dy);
    let leader = (distance > LEADER_MIN_DISTANCE).then(|| {
        // Start clear of the lettering: step out along the leader's
        // direction from the underline's midpoint, and stop short of
        // the zone point so the chalk never touches the felt marking
        // it names.
        let (ux, uy) = (dx / distance, dy / distance);
        let (sx, sy) = (ux * 190.0, 26.0 + uy * 40.0);
        let (ex, ey) = (dx - ux * 32.0, dy - uy * 32.0);
        view! {
            <line
                x1=format!("{sx:.2}")
                y1=format!("{sy:.2}")
                x2=format!("{ex:.2}")
                y2=format!("{ey:.2}")
                stroke=CHALK_INK
                stroke-width="1.4"
                stroke-dasharray="2 8"
                stroke-linecap="round"
                opacity="0.5"
            ></line>
        }
    });
    view! {
        <g transform=frame opacity=if note.dimmed { 0.3 } else { 0.95 }>
            <g transform=format!("translate({x:.2} {y:.2})")>
                {leader}
                <text
                    text-anchor="middle"
                    font-family=CHALK_FONT
                    font-size="34"
                    letter-spacing="3"
                    fill=CHALK_INK
                >
                    {note.title}
                </text>
                <line
                    x1="-115"
                    y1="12"
                    x2="115"
                    y2="12"
                    stroke=CHALK_INK
                    stroke-width="1.6"
                    stroke-dasharray="7 5"
                    opacity="0.55"
                ></line>
                {note
                    .details
                    .iter()
                    .enumerate()
                    .map(|(i, line)| {
                        view! {
                            <text
                                y=48.0 + (i as f64) * 40.0
                                text-anchor="middle"
                                font-family=CHALK_FONT
                                font-size="25"
                                fill=CHALK_INK
                                opacity="0.9"
                            >
                                {*line}
                            </text>
                        }
                    })
                    .collect_view()}
            </g>
        </g>
    }
}

/// The chip legend: one of each denomination with its dollar value
/// chalked beneath, laid out under the dealer tray where the real
/// chips are racked.
#[component]
fn ChipLegend() -> impl IntoView {
    let entries = chip_legend();
    let count = entries.len();
    view! {
        <g transform=format!("translate({CHIP_LEGEND_X:.2} {CHIP_LEGEND_Y:.2})") opacity="0.95">
            {entries
                .into_iter()
                .enumerate()
                .map(|(i, (denomination, label))| {
                    let x = ((i as f64) - ((count - 1) as f64) / 2.0) * 80.0;
                    view! {
                        <g transform=format!("translate({x:.2} 0)")>
                            <ChipView denomination=denomination top=true />
                            <text
                                y="52"
                                text-anchor="middle"
                                font-family=CHALK_FONT
                                font-size="25"
                                fill=CHALK_INK
                                opacity="0.9"
                            >
                                {label}
                            </text>
                        </g>
                    }
                })
                .collect_view()}
        </g>
    }
}

/// The placard's conventions explained in plain language, chalked
/// beside the plaque itself.
#[component]
fn PlacardChalk(lines: [String; 4]) -> impl IntoView {
    view! {
        <g transform=format!("translate({PLACARD_NOTES_X:.2} {PLACARD_NOTES_Y:.2})") opacity="0.95">
            <text
                y=-56.0
                font-family=CHALK_FONT
                font-size="30"
                letter-spacing="2.5"
                fill=CHALK_INK
            >
                "THE PLACARD, PLAINLY"
            </text>
            <line
                x1="0"
                y1="-44"
                x2="350"
                y2="-44"
                stroke=CHALK_INK
                stroke-width="1.6"
                stroke-dasharray="7 5"
                opacity="0.55"
            ></line>
            {lines
                .into_iter()
                .enumerate()
                .map(|(i, line)| {
                    view! {
                        <text
                            y=(i as f64) * PLACARD_NOTES_LEADING
                            font-family=CHALK_FONT
                            font-size="25"
                            fill=CHALK_INK
                            opacity="0.9"
                        >
                            {line}
                        </text>
                    }
                })
                .collect_view()}
        </g>
    }
}

/// Everything chalked on the glass, a pure function of the snapshot.
#[component]
fn HelpContent(snapshot: RwSignal<Option<Snapshot>>) -> impl IntoView {
    move || {
        snapshot.get().map(|snap| {
            let seat = human_seat(&snap);
            let ctx = GestureCtx::from_snapshot(&snap, seat);
            let place = ctx.place;
            let notes = help_notes(&ctx);
            let placard = placard_notes(&snap.rules);
            view! {
                <ChalkZones place=place />
                <ChipLegend />
                {notes
                    .into_iter()
                    .map(|note| view! { <ChalkNote note=note place=place /> })
                    .collect_view()}
                <PlacardChalk lines=placard />
            }
        })
    }
}

/// The help layer: hold-key state plus [`OverlayLayer`] wrapping the
/// chalk content. Mount it after the input layer so the glass sits
/// visually above everything; its `pointer-events: none` lets every
/// gesture fall through.
///
/// Dwell-hover (pointer idle over a zone showing just that zone's note)
/// is deliberately out of scope here: the gesture layer owns the
/// pointer, and threading an idle timer through it while #10 reworks
/// that neighborhood buys little — the hold-a-key chord already covers
/// discoverability without adding pointer state.
#[component]
pub fn HelpOverlay(
    /// The app's snapshot signal (read-only here).
    snapshot: RwSignal<Option<Snapshot>>,
) -> impl IntoView {
    let active = RwSignal::new(false);

    let down = window_event_listener(leptos::ev::keydown, move |ev| {
        if is_help_key(&ev.key()) {
            ev.prevent_default();
            if !ev.repeat() {
                active.set(true);
            }
        }
    });
    let up = window_event_listener(leptos::ev::keyup, move |ev| {
        if releases_help_key(&ev.key()) {
            active.set(false);
        }
    });
    let blur = window_event_listener(leptos::ev::blur, move |_| active.set(false));
    on_cleanup(move || {
        down.remove();
        up.remove();
        blur.remove();
    });

    view! {
        <OverlayLayer active=Signal::derive(move || active.get())>
            <HelpContent snapshot=snapshot />
        </OverlayLayer>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_holds_on_question_mark_and_f1_only() {
        assert!(is_help_key("?"));
        assert!(is_help_key("F1"));
        for key in ["h", "s", "/", "Shift", "Enter", "F2", " "] {
            assert!(!is_help_key(key), "{key}");
        }
    }

    #[test]
    fn release_accepts_the_shifted_key_coming_apart() {
        for key in ["?", "/", "F1", "Shift"] {
            assert!(releases_help_key(key), "{key}");
        }
        // Gameplay keys never drop the glass.
        for key in ["h", "s", "d", "p", "r", "y", "n", "w", "Enter"] {
            assert!(!releases_help_key(key), "{key}");
        }
    }
}
