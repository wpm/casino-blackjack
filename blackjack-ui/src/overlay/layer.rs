//! The generic suspended-above-the-table layer.
//!
//! [`OverlayLayer`] is the reusable mechanism: one full-viewBox SVG
//! floating over the felt (visually above the scene *and* the gesture
//! layer) with a frosted, faintly darkened glass treatment, rendering
//! whatever annotation content its children provide while `active` is
//! true and unmounting completely when it is not. The layer takes
//! `pointer-events: none`, so every gesture passes straight through to
//! the input layer beneath — nothing on the glass is ever a control.
//!
//! Help (see [`super::help`]) is only the first tenant. A future
//! card-counting display reuses this exact idiom: its own activation
//! signal in, its own chalk content as children, the same glass.

use leptos::prelude::*;

use crate::scene::geometry::{VIEW_H, VIEW_W};

/// The translucent glass and its content, mounted only while `active`.
///
/// Children draw in the shared scene coordinate system (the
/// `0 0 1600 1000` viewBox with `xMidYMid meet` scaling), so anything
/// lettered onto the glass can anchor to the same felt geometry the
/// scene and gestures use.
#[component]
pub fn OverlayLayer(
    /// Whether the layer is up. When this goes false the glass and
    /// everything on it vanish completely — no residue, no chrome.
    #[prop(into)]
    active: Signal<bool>,
    /// The annotation content suspended over the table.
    children: ChildrenFn,
) -> impl IntoView {
    view! {
        <Show when=move || active.get()>
            <svg
                viewBox=format!("0 0 {VIEW_W} {VIEW_H}")
                preserveAspectRatio="xMidYMid meet"
                style="display:block;position:absolute;top:0;left:0;width:100vw;height:100vh;\
                       pointer-events:none;\
                       backdrop-filter:blur(2px) saturate(0.8);\
                       -webkit-backdrop-filter:blur(2px) saturate(0.8);"
            >
                // The darkened glass: subtle — the table beneath stays
                // legible; the frosting comes from the backdrop blur.
                <rect
                    x="0"
                    y="0"
                    width=VIEW_W
                    height=VIEW_H
                    fill="rgba(6, 10, 9, 0.42)"
                ></rect>
                {children()}
            </svg>
        </Show>
    }
}
