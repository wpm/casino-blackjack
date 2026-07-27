# Casino Blackjack

An installable, configurationless casino blackjack game: one human player at a
living table of AI strangers, viewed top-down onto green velvet, with real
multi-deck mechanics (insurance, splits, double down, late surrender, 3:2
blackjack) — honest enough to practice card counting against. Chips are the
only scoreboard; house rules appear only on the table placard.

## Building

Prerequisites: [Rust](https://rustup.rs), [Trunk](https://trunkrs.dev)
(`cargo install trunk`), the `wasm32-unknown-unknown` target
(`rustup target add wasm32-unknown-unknown`), and the
[Tauri CLI](https://tauri.app) (`cargo install tauri-cli`).

```sh
cargo tauri dev      # run the desktop app in development mode
cargo tauri build    # release build
cargo test           # run the workspace test suite
```

The workspace has three crates: `blackjack-core` (pure rules engine),
`blackjack-ui` (Leptos frontend, built with Trunk), and `src-tauri`
(native shell).
