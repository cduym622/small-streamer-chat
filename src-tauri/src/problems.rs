//! Problems the streamer should see, such as the Overlay port being taken or
//! a hotkey owned by another app. Kept in a list because many happen during
//! startup, before the window is listening for events.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

pub const EVENT: &str = "problem";

#[derive(Default)]
pub struct Problems(Mutex<Vec<String>>);

impl Problems {
    pub fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

/// Shows a problem to the streamer. Repeats of one already shown are skipped.
pub fn report(app: &AppHandle, message: String) {
    eprintln!("{message}");
    let state = app.state::<Problems>();
    let mut problems = state.0.lock().unwrap();
    if problems.contains(&message) {
        return;
    }
    problems.push(message.clone());
    let _ = app.emit(EVENT, message);
}
