//! The keyboard mirror of the felt gestures.
//!
//! # Key map
//!
//! | Key | Meaning |
//! |---|---|
//! | `H` | hit |
//! | `S` | stand |
//! | `D` | double |
//! | `P` | split |
//! | `R` | surrender |
//! | `Y` / `N` | take / decline insurance |
//! | `Enter` | post the staged bet (betting) or next round (round over) |
//! | `+` / `=` / `ArrowUp` | staged bet up one table minimum |
//! | `-` / `_` / `ArrowDown` | staged bet down one table minimum |
//! | `1`..`9` | staged bet = digit × table minimum |
//! | `W` | walk away (between rounds) |
//!
//! [`key_command`] is the pure key-name map; [`command_intent`] applies
//! the same legality gates as the pointer recognizers, so a key press
//! can no more produce an illegal action than a gesture can.

use blackjack_core::{ActionKind, Phase};

use super::gesture::{GestureCtx, Intent};
use super::staging::stage_ready;

/// A key press, decoded but not yet gated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCommand {
    /// `H`.
    Hit,
    /// `S`.
    Stand,
    /// `D`.
    Double,
    /// `P`.
    Split,
    /// `R`.
    Surrender,
    /// `Y`.
    InsuranceYes,
    /// `N`.
    InsuranceNo,
    /// `Enter`.
    Confirm,
    /// `+` / `=` / `ArrowUp`.
    BetUp,
    /// `-` / `_` / `ArrowDown`.
    BetDown,
    /// A digit: stage this many table minimums.
    BetUnits(u32),
    /// `W`.
    WalkAway,
}

/// Decode a `KeyboardEvent.key` value. Unknown keys are `None`.
pub fn key_command(key: &str) -> Option<KeyCommand> {
    match key {
        "h" | "H" => Some(KeyCommand::Hit),
        "s" | "S" => Some(KeyCommand::Stand),
        "d" | "D" => Some(KeyCommand::Double),
        "p" | "P" => Some(KeyCommand::Split),
        "r" | "R" => Some(KeyCommand::Surrender),
        "y" | "Y" => Some(KeyCommand::InsuranceYes),
        "n" | "N" => Some(KeyCommand::InsuranceNo),
        "Enter" => Some(KeyCommand::Confirm),
        "+" | "=" | "ArrowUp" => Some(KeyCommand::BetUp),
        "-" | "_" | "ArrowDown" => Some(KeyCommand::BetDown),
        "w" | "W" => Some(KeyCommand::WalkAway),
        digit => digit
            .parse::<u32>()
            .ok()
            .filter(|&n| (1..=9).contains(&n) && digit.len() == 1)
            .map(KeyCommand::BetUnits),
    }
}

/// Gate a decoded key against the current context, exactly as the
/// pointer recognizers gate strokes.
///
/// Bet-adjustment keys ([`KeyCommand::BetUp`], [`KeyCommand::BetDown`],
/// [`KeyCommand::BetUnits`]) return `None` here — they change only the
/// UI-local stage and are applied by the overlay through
/// [`super::staging`].
pub fn command_intent(ctx: &GestureCtx, command: KeyCommand, staged_total: u32) -> Option<Intent> {
    let hand = |kind: ActionKind, intent: Intent| {
        (ctx.phase == Phase::PlayerTurn && ctx.human_active && ctx.allows(kind)).then_some(intent)
    };
    match command {
        KeyCommand::Hit => hand(ActionKind::Hit, Intent::Hit),
        KeyCommand::Stand => hand(ActionKind::Stand, Intent::Stand),
        KeyCommand::Double => hand(ActionKind::Double, Intent::Double),
        KeyCommand::Split => hand(ActionKind::Split, Intent::Split),
        KeyCommand::Surrender => hand(ActionKind::Surrender, Intent::Surrender),
        KeyCommand::InsuranceYes => (ctx.phase == Phase::InsuranceOffer
            && ctx.human_active
            && ctx.allows(ActionKind::TakeInsurance))
        .then_some(Intent::TakeInsurance),
        KeyCommand::InsuranceNo => (ctx.phase == Phase::InsuranceOffer
            && ctx.human_active
            && ctx.allows(ActionKind::DeclineInsurance))
        .then_some(Intent::DeclineInsurance),
        KeyCommand::Confirm => match ctx.phase {
            Phase::Betting
                if ctx.allows(ActionKind::PlaceBet)
                    && stage_ready(staged_total, ctx.min_bet, ctx.max_bet) =>
            {
                Some(Intent::ConfirmBet(staged_total))
            }
            Phase::RoundOver if ctx.allows(ActionKind::NextRound) => Some(Intent::NextRound),
            _ => None,
        },
        KeyCommand::WalkAway => {
            matches!(ctx.phase, Phase::Betting | Phase::RoundOver).then_some(Intent::WalkAway)
        }
        KeyCommand::BetUp | KeyCommand::BetDown | KeyCommand::BetUnits(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::gesture::test_ctx;

    #[test]
    fn the_key_map_is_the_documented_one() {
        let pairs = [
            ("h", KeyCommand::Hit),
            ("H", KeyCommand::Hit),
            ("s", KeyCommand::Stand),
            ("d", KeyCommand::Double),
            ("p", KeyCommand::Split),
            ("r", KeyCommand::Surrender),
            ("y", KeyCommand::InsuranceYes),
            ("n", KeyCommand::InsuranceNo),
            ("Enter", KeyCommand::Confirm),
            ("+", KeyCommand::BetUp),
            ("=", KeyCommand::BetUp),
            ("ArrowUp", KeyCommand::BetUp),
            ("-", KeyCommand::BetDown),
            ("_", KeyCommand::BetDown),
            ("ArrowDown", KeyCommand::BetDown),
            ("w", KeyCommand::WalkAway),
            ("1", KeyCommand::BetUnits(1)),
            ("9", KeyCommand::BetUnits(9)),
        ];
        for (key, command) in pairs {
            assert_eq!(key_command(key), Some(command), "{key}");
        }
        for dead in ["q", "0", "10", "Escape", " ", "F1", "Tab"] {
            assert_eq!(key_command(dead), None, "{dead}");
        }
    }

    #[test]
    fn hand_keys_gate_on_turn_and_legality() {
        let legal = [ActionKind::Hit, ActionKind::Stand, ActionKind::Double];
        let ctx = test_ctx(Phase::PlayerTurn, &legal, true);
        assert_eq!(command_intent(&ctx, KeyCommand::Hit, 0), Some(Intent::Hit));
        assert_eq!(
            command_intent(&ctx, KeyCommand::Double, 0),
            Some(Intent::Double)
        );
        // Split is not legal (no pair): the key is dead.
        assert_eq!(command_intent(&ctx, KeyCommand::Split, 0), None);
        // Not the human's turn: everything is dead.
        let ctx_idle = test_ctx(Phase::PlayerTurn, &legal, false);
        assert_eq!(command_intent(&ctx_idle, KeyCommand::Hit, 0), None);
        // Wrong phase entirely.
        let ctx_bet = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        assert_eq!(command_intent(&ctx_bet, KeyCommand::Hit, 0), None);
    }

    #[test]
    fn enter_posts_bets_and_advances_rounds() {
        let ctx_bet = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        assert_eq!(
            command_intent(&ctx_bet, KeyCommand::Confirm, 50),
            Some(Intent::ConfirmBet(50))
        );
        // An unready stage does not post.
        assert_eq!(command_intent(&ctx_bet, KeyCommand::Confirm, 0), None);
        assert_eq!(command_intent(&ctx_bet, KeyCommand::Confirm, 5), None);
        let ctx_over = test_ctx(Phase::RoundOver, &[ActionKind::NextRound], false);
        assert_eq!(
            command_intent(&ctx_over, KeyCommand::Confirm, 0),
            Some(Intent::NextRound)
        );
        let ctx_play = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand],
            true,
        );
        assert_eq!(command_intent(&ctx_play, KeyCommand::Confirm, 0), None);
    }

    #[test]
    fn insurance_keys_answer_only_the_humans_open_offer() {
        let legal = [ActionKind::TakeInsurance, ActionKind::DeclineInsurance];
        let ctx = test_ctx(Phase::InsuranceOffer, &legal, true);
        assert_eq!(
            command_intent(&ctx, KeyCommand::InsuranceYes, 0),
            Some(Intent::TakeInsurance)
        );
        assert_eq!(
            command_intent(&ctx, KeyCommand::InsuranceNo, 0),
            Some(Intent::DeclineInsurance)
        );
        let ctx_idle = test_ctx(Phase::InsuranceOffer, &legal, false);
        assert_eq!(command_intent(&ctx_idle, KeyCommand::InsuranceYes, 0), None);
        assert_eq!(command_intent(&ctx_idle, KeyCommand::InsuranceNo, 0), None);
    }

    #[test]
    fn walk_away_and_bet_adjust_keys() {
        let ctx_bet = test_ctx(Phase::Betting, &[ActionKind::PlaceBet], false);
        assert_eq!(
            command_intent(&ctx_bet, KeyCommand::WalkAway, 0),
            Some(Intent::WalkAway)
        );
        let ctx_play = test_ctx(
            Phase::PlayerTurn,
            &[ActionKind::Hit, ActionKind::Stand],
            true,
        );
        assert_eq!(command_intent(&ctx_play, KeyCommand::WalkAway, 0), None);
        // Bet adjustments never emit intents; the overlay applies them
        // to the stage directly.
        assert_eq!(command_intent(&ctx_bet, KeyCommand::BetUp, 0), None);
        assert_eq!(command_intent(&ctx_bet, KeyCommand::BetDown, 20), None);
        assert_eq!(command_intent(&ctx_bet, KeyCommand::BetUnits(5), 0), None);
    }
}
