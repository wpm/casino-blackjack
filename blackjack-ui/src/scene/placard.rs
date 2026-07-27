//! The table placard: the plaque posting limits and house rules.
//!
//! Every line comes from [`placard_lines`], which reads the frozen
//! [`Rules`] out of the snapshot — nothing on the plaque is ever
//! hardcoded. This is the only place text and numerals appear during
//! play (the insurance lettering and card indices are felt and card
//! markings).

use blackjack_core::Rules;
use leptos::prelude::*;

use super::geometry::placard_lines;

/// Serif stack for the plaque's engraved lettering.
const PLACARD_FONT: &str = "Georgia, 'Palatino Linotype', 'Times New Roman', serif";

/// The plaque, centered on the local origin. Callers position and tilt
/// it with a `transform`.
#[component]
pub fn Placard(rules: Rules) -> impl IntoView {
    let [payout, soft17, limits, double, das, surrender] = placard_lines(&rules);
    // (text, y, size, weight, letter-spacing)
    let lines: [(String, f64, f64, &str, f64); 6] = [
        (payout, -74.0, 19.0, "bold", 0.5),
        (soft17, -44.0, 14.5, "normal", 0.5),
        (limits, -8.0, 21.0, "bold", 1.0),
        (double, 30.0, 12.5, "normal", 0.4),
        (das, 56.0, 12.5, "normal", 0.4),
        (surrender, 82.0, 12.5, "normal", 0.4),
    ];
    view! {
        // Shadow, brass frame, and cream face.
        <rect
            x="-146"
            y="-108"
            width="300"
            height="230"
            rx="12"
            fill="rgba(8, 18, 12, 0.35)"
        ></rect>
        <rect
            x="-150"
            y="-115"
            width="300"
            height="230"
            rx="12"
            fill="url(#plaque-face)"
            stroke="#8a7444"
            stroke-width="3"
        ></rect>
        <rect
            x="-139"
            y="-104"
            width="278"
            height="208"
            rx="8"
            fill="none"
            stroke="rgba(138, 116, 68, 0.55)"
            stroke-width="1.2"
        ></rect>
        // Divider under the headline rules.
        <line
            x1="-112"
            y1="-28"
            x2="112"
            y2="-28"
            stroke="rgba(138, 116, 68, 0.6)"
            stroke-width="1"
        ></line>
        <line
            x1="-112"
            y1="12"
            x2="112"
            y2="12"
            stroke="rgba(138, 116, 68, 0.6)"
            stroke-width="1"
        ></line>
        {lines
            .into_iter()
            .map(|(text, y, size, weight, spacing)| {
                view! {
                    <text
                        x="2"
                        y=y
                        text-anchor="middle"
                        font-family=PLACARD_FONT
                        font-size=size
                        font-weight=weight
                        letter-spacing=spacing
                        fill="#463a20"
                    >
                        {text}
                    </text>
                }
            })
            .collect_view()}
    }
}
