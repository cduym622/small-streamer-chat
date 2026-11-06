//! Global hotkeys, so the streamer can control the Practice Chat from inside
//! a game. Fixed for the MVP; customizable later.

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::engine::{Command, EngineHandle};

#[derive(Clone, Copy)]
enum Action {
    StartStop,
    BackOff,
    Nudge,
}

const HOTKEYS: [(Action, Code, &str, &str); 3] = [
    (Action::StartStop, Code::F7, "Ctrl+Shift+F7", "Start / stop"),
    (Action::BackOff, Code::F8, "Ctrl+Shift+F8", "Back-off"),
    (Action::Nudge, Code::F9, "Ctrl+Shift+F9", "Nudge"),
];

fn shortcut(code: Code) -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), code)
}

#[derive(Clone, Serialize)]
pub struct HotkeyInfo {
    pub keys: &'static str,
    pub action: &'static str,
}

pub fn list() -> Vec<HotkeyInfo> {
    HOTKEYS
        .iter()
        .map(|&(_, _, keys, action)| HotkeyInfo { keys, action })
        .collect()
}

/// Installs the global-shortcut plugin and registers the hotkeys. A hotkey
/// another app already owns is reported to the UI instead of failing startup.
pub fn register(app: &AppHandle) -> tauri::Result<()> {
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, pressed, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                let Some(&(action, ..)) = HOTKEYS
                    .iter()
                    .find(|(_, code, ..)| &shortcut(*code) == pressed)
                else {
                    return;
                };
                let engine = app.state::<EngineHandle>();
                engine.send(match action {
                    Action::StartStop => Command::ToggleRunning,
                    Action::BackOff => Command::ToggleBackOff,
                    Action::Nudge => Command::Nudge,
                });
            })
            .build(),
    )?;

    for (_, code, keys, _) in HOTKEYS {
        if let Err(error) = app.global_shortcut().register(shortcut(code)) {
            crate::problems::report(app, format!("Couldn't register the {keys} hotkey: {error}"));
        }
    }
    Ok(())
}
