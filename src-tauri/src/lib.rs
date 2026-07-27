//! Casino Blackjack native shell: Tauri app setup and the IPC command
//! layer over the engine.
//!
//! The commands are deliberately thin — one line each over the plain
//! functions in [`session`] — so the integration tests exercise the same
//! code path the frontend reaches through `invoke`.

pub mod session;

use blackjack_core::{Action, Transition};
use blackjack_protocol::BackendError;
use session::SessionState;

#[tauri::command]
fn start_session(state: tauri::State<'_, SessionState>) -> Result<Transition, BackendError> {
    session::start_session(&state)
}

#[tauri::command]
fn snapshot(state: tauri::State<'_, SessionState>) -> Result<Transition, BackendError> {
    session::snapshot(&state)
}

#[tauri::command]
fn submit_action(
    state: tauri::State<'_, SessionState>,
    seat: usize,
    action: Action,
) -> Result<Transition, BackendError> {
    session::submit_action(&state, seat, action)
}

/// Build and run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .manage(SessionState::default())
        .invoke_handler(tauri::generate_handler![
            start_session,
            snapshot,
            submit_action
        ])
        .run(tauri::generate_context!())
        .expect("error while running Casino Blackjack");
}
