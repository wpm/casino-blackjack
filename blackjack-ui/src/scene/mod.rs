//! The table scene: a top-down casino blackjack table as one SVG,
//! rendered purely from a [`Snapshot`](blackjack_core::Snapshot).
//!
//! Scene components never touch the backend — they take data as props
//! and draw it. The pure layout math and styling tables live in
//! [`geometry`], which tests on the host; the components are thin views
//! over it, verified by the wasm build.
//!
//! # SVG structure (for #9 gestures and #10 motion)
//!
//! One `<svg viewBox="0 0 1600 1000">` scales to the window. All arcs
//! share the focus ([`geometry::ARC_CX`], [`geometry::ARC_CY`]); seat
//! `i`'s betting circle center comes from [`geometry::seat_places`].
//! Objects are positioned by `transform` on plain `<g>` wrappers, so
//! animation can retarget transforms without touching the drawings.
//! Shared paint/filter/clip ids are documented on `SceneDefs` in
//! `table.rs`.

pub mod geometry;

mod card;
mod chips;
mod dealer;
mod placard;
mod seat;
mod table;

pub use card::{CardBack, CardFace};
pub use chips::{ChipPile, ChipStackView, ChipView, RackView};
pub use dealer::{DealerTray, DealerView, DiscardTrayView, ShoeView};
pub use placard::Placard;
pub use seat::{HandView, SeatView};
pub use table::TableScene;
