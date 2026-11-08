//! The streamer's choices that survive a restart, stored as JSON in the app's
//! config folder.

use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::ai::Provider;

const FILE_NAME: &str = "settings.json";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The first-run setup was finished or skipped.
    pub setup_done: bool,
    /// Chosen microphone's device id; `None` follows the system default.
    pub microphone: Option<String>,
    /// What the stream is about, in the streamer's words (Stream Context).
    pub stream_context: String,
    /// The AI Connection the streamer picked to write messages, when more
    /// than one is set up.
    pub ai_choice: Option<Provider>,
    /// The ChatGPT account signed in with (its tokens are in the keychain).
    pub chatgpt: Option<ChatGptAccount>,
    /// This computer's ID for ChatGPT sign-in, made on first use.
    pub chatgpt_host_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptAccount {
    /// Issued to this app for this account when it was first signed in.
    pub client_id: String,
    pub email: String,
    /// OpenAI's ID for the account.
    pub subject: String,
    /// The chosen model; `None` uses the smallest, fastest one on offer.
    pub model: Option<String>,
}

fn path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(FILE_NAME))
}

/// Missing or unreadable settings fall back to the defaults.
pub fn load<R: Runtime>(app: &AppHandle<R>) -> Settings {
    path(app)
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save<R: Runtime>(app: &AppHandle<R>, settings: &Settings) -> io::Result<()> {
    let path = path(app).ok_or_else(|| io::Error::other("no config folder"))?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, serde_json::to_vec_pretty(settings)?)
}
