//! Casino Blackjack native shell: Tauri app setup and the IPC command
//! layer over the session arc.
//!
//! The commands are deliberately thin — one line each over the plain
//! functions in [`session`] — so the integration tests exercise the same
//! code path the frontend reaches through `invoke`.

pub mod session;

use blackjack_core::Action;
use blackjack_protocol::{BackendError, SessionView};
use session::SessionState;

#[tauri::command]
fn start_session(state: tauri::State<'_, SessionState>) -> Result<SessionView, BackendError> {
    session::start_session(&state)
}

#[tauri::command]
fn view(state: tauri::State<'_, SessionState>) -> Result<SessionView, BackendError> {
    session::view(&state)
}

#[tauri::command]
fn advance(state: tauri::State<'_, SessionState>) -> Result<SessionView, BackendError> {
    session::advance(&state)
}

#[tauri::command]
fn human_action(
    state: tauri::State<'_, SessionState>,
    action: Action,
) -> Result<SessionView, BackendError> {
    session::human_action(&state, action)
}

#[tauri::command]
fn walk_away(state: tauri::State<'_, SessionState>) -> Result<SessionView, BackendError> {
    session::walk_away(&state)
}

/// Build and run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .manage(SessionState::default())
        .invoke_handler(tauri::generate_handler![
            start_session,
            view,
            advance,
            human_action,
            walk_away
        ])
        .run(tauri::generate_context!())
        .expect("error while running Casino Blackjack")
}
