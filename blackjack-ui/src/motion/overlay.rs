//! The transient animation layer: one moving object over the scene.
//!
//! The scene renders the display snapshot; while a step plays, the
//! moving object (a card in flight, chips sliding, the shuffle shimmer)
//! lives here instead — a second SVG stacked over the table with the
//! same viewBox, so scene coordinates line up exactly and pointer
//! events pass straight through to the felt. The per-frame math
//! ([`frame`]) is pure and tests on the host; only the driver calls it
//! with wall-clock times.

use blackjack_core::ChipStack;
use leptos::prelude::*;

use crate::scene::geometry::{CARD_H, CARD_W, VIEW_H, VIEW_W};
use crate::scene::{CardBack, CardFace, ChipStackView};

use super::Motion;
use super::paths::SHOE_POS;
use super::plan::{StepKind, Visual};

/// What the animation layer draws this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overlay {
    /// The object in motion.
    pub visual: Visual,
    /// Scene x of the object's center.
    pub x: f64,
    /// Scene y of the object's center.
    pub y: f64,
    /// Rotation in SVG degrees.
    pub rot: f64,
    /// Horizontal scale (the flip narrows a card to its edge).
    pub scale_x: f64,
    /// Overall opacity.
    pub opacity: f64,
}

/// Ease-out cubic: objects leave the shoe fast and settle gently.
pub fn ease_out(t: f64) -> f64 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// The overlay for a step at progress `t` (`0.0..=1.0`), or `None` when
/// the step shows nothing (pauses, and any finished step).
pub fn frame(kind: &StepKind, t: f64) -> Option<Overlay> {
    let t = t.clamp(0.0, 1.0);
    match *kind {
        StepKind::Fly { visual, from, to } => {
            let p = ease_out(t);
            Some(Overlay {
                visual,
                x: from.x + (to.x - from.x) * p,
                y: from.y + (to.y - from.y) * p,
                rot: from.rot + (to.rot - from.rot) * p,
                scale_x: 1.0,
                opacity: 1.0,
            })
        }
        StepKind::Flip { card, at } => {
            // The back narrows to an edge, then the face grows from it.
            let (visual, scale_x) = if t < 0.5 {
                (Visual::Back, 1.0 - 2.0 * t)
            } else {
                (Visual::Card(card), 2.0 * t - 1.0)
            };
            Some(Overlay {
                visual,
                x: at.x,
                y: at.y,
                rot: at.rot,
                scale_x: scale_x.max(0.02),
                opacity: 1.0,
            })
        }
        StepKind::Shimmer => {
            // A flicker at the shoe: brightness ripples as the deck riffles.
            let ripple = (t * 28.0).sin().abs();
            Some(Overlay {
                visual: Visual::Shimmer,
                x: SHOE_POS.0,
                y: SHOE_POS.1,
                rot: 0.0,
                scale_x: 1.0,
                opacity: (1.0 - t) * (0.25 + 0.35 * ripple),
            })
        }
        StepKind::Pause => None,
    }
}

/// The animation layer: an SVG stacked over the table scene, same
/// viewBox, pointer-transparent. Paint ids (`card-face`, `card-back-clip`,
/// ...) resolve against the table scene's `<defs>` because both SVGs
/// share the document.
#[component]
pub fn MotionOverlay(motion: Motion) -> impl IntoView {
    let overlay = motion.overlay_signal();
    view! {
        <svg
            viewBox=format!("0 0 {VIEW_W} {VIEW_H}")
            preserveAspectRatio="xMidYMid meet"
            style="position:absolute;inset:0;display:block;width:100vw;height:100vh;pointer-events:none;"
        >
            {move || overlay.get().map(overlay_view)}
        </svg>
    }
}

/// Render one overlay frame.
fn overlay_view(overlay: Overlay) -> AnyView {
    let Overlay {
        visual,
        x,
        y,
        rot,
        scale_x,
        opacity,
    } = overlay;
    match visual {
        Visual::Card(card) => view! {
            <g
                transform=format!(
                    "translate({x:.2} {y:.2}) rotate({rot:.2}) scale({scale_x:.3} 1) translate({:.2} {:.2})",
                    -CARD_W / 2.0,
                    -CARD_H / 2.0
                )
                opacity=opacity
            >
                <CardFace card=card />
            </g>
        }
        .into_any(),
        Visual::Back => view! {
            <g
                transform=format!(
                    "translate({x:.2} {y:.2}) rotate({rot:.2}) scale({scale_x:.3} 1) translate({:.2} {:.2})",
                    -CARD_W / 2.0,
                    -CARD_H / 2.0
                )
                opacity=opacity
            >
                <CardBack />
            </g>
        }
        .into_any(),
        Visual::Chips(amount) => view! {
            <g transform=format!("translate({x:.2} {y:.2})") opacity=opacity>
                <ChipStackView stack=ChipStack::change(amount) />
            </g>
        }
        .into_any(),
        Visual::Shimmer => view! {
            // Slivers of card edge catching the light as the deck riffles.
            <g transform=format!("translate({x:.2} {y:.2}) rotate(-8)") opacity=opacity>
                <rect x="-78" y="-38" width="152" height="64" rx="4" fill="#f4efdf"></rect>
                <rect x="-70" y="-30" width="136" height="10" fill="#ffffff" opacity="0.7"></rect>
                <rect x="-70" y="-8" width="136" height="10" fill="#ffffff" opacity="0.5"></rect>
                <rect x="-70" y="14" width="136" height="10" fill="#ffffff" opacity="0.6"></rect>
            </g>
        }
        .into_any(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::paths::Anchor;
    use blackjack_core::{Card, Rank, Suit};

    #[test]
    fn easing_starts_fast_and_settles() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert!(ease_out(0.5) > 0.5, "ease-out front-loads the motion");
        assert!(ease_out(-1.0) == 0.0 && ease_out(2.0) == 1.0);
    }

    #[test]
    fn a_fly_interpolates_between_its_anchors() {
        let kind = StepKind::Fly {
            visual: Visual::Back,
            from: Anchor {
                x: 0.0,
                y: 0.0,
                rot: -8.0,
            },
            to: Anchor {
                x: 100.0,
                y: 50.0,
                rot: 12.0,
            },
        };
        let start = frame(&kind, 0.0).unwrap();
        assert_eq!((start.x, start.y, start.rot), (0.0, 0.0, -8.0));
        let end = frame(&kind, 1.0).unwrap();
        assert_eq!((end.x, end.y, end.rot), (100.0, 50.0, 12.0));
        let mid = frame(&kind, 0.5).unwrap();
        assert!(mid.x > 50.0, "ease-out is past halfway at t=0.5");
        assert_eq!(mid.scale_x, 1.0);
    }

    #[test]
    fn the_flip_shows_the_back_then_the_face() {
        let card = Card::new(Rank::Queen, Suit::Hearts);
        let kind = StepKind::Flip {
            card,
            at: Anchor {
                x: 800.0,
                y: 268.0,
                rot: 0.0,
            },
        };
        let early = frame(&kind, 0.25).unwrap();
        assert_eq!(early.visual, Visual::Back);
        assert!((early.scale_x - 0.5).abs() < 1e-9);
        let late = frame(&kind, 0.75).unwrap();
        assert_eq!(late.visual, Visual::Card(card));
        assert!((late.scale_x - 0.5).abs() < 1e-9);
        // Never a degenerate zero-width transform.
        assert!(frame(&kind, 0.5).unwrap().scale_x > 0.0);
    }

    #[test]
    fn the_shimmer_sits_at_the_shoe_and_dies_away() {
        let start = frame(&StepKind::Shimmer, 0.05).unwrap();
        assert_eq!((start.x, start.y), SHOE_POS);
        assert!(start.opacity > 0.0 && start.opacity < 0.7);
        let done = frame(&StepKind::Shimmer, 1.0).unwrap();
        assert!(done.opacity.abs() < 1e-9);
    }

    #[test]
    fn pauses_draw_nothing() {
        assert_eq!(frame(&StepKind::Pause, 0.5), None);
    }
}
