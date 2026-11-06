//! The streamer's choices that survive a restart, stored as JSON in the app's
//! config folder.

use std::{fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

const FILE_NAME: &str = "settings.json";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Chosen microphone's device id; `None` follows the system default.
    pub microphone: Option<String>,
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
