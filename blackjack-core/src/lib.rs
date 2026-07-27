//! Pure casino blackjack rules engine.
//!
//! This crate has no UI or platform dependencies. All game state lives here;
//! frontends consume snapshots and never compute rules themselves.

/// Engine version, exposed so shells can report what they embed.
pub const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_version_matches_crate() {
        assert_eq!(ENGINE_VERSION, env!("CARGO_PKG_VERSION"));
    }
}
