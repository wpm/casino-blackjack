//! The overlay layer: annotations suspended above the table.
//!
//! A general idiom, first used for help. The reusable mechanism is
//! [`OverlayLayer`] ([`layer`]): a full-viewBox SVG floating over the
//! scene and the gesture layer, frosted-glass darkened just enough to
//! lift chalk lettering off the felt while the table beneath stays
//! visible, with `pointer-events: none` so every gesture passes
//! through untouched. It takes its content as children and its
//! activation as a signal — help is merely the first tenant, and a
//! future card-counting display mounts the same layer with its own
//! signal and its own chalk.
//!
//! Help mode ([`help`]) letters the gesture vocabulary onto the glass
//! while `?` or `F1` is **held** — release and it vanishes completely.
//! The wording, anchors, and dimming are pure host-tested data in
//! [`annotations`]; annotations anchor to the exact zone geometry the
//! recognizers in [`crate::input`] read, and dim (never vanish) when
//! their gesture is meaningless in the current phase.

pub mod annotations;

mod help;
mod layer;

pub use help::{HelpOverlay, is_help_key, releases_help_key};
pub use layer::OverlayLayer;
