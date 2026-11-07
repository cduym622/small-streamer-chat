//! Secrets live in the OS credential store (macOS Keychain, Windows
//! Credential Manager), never in the settings file.

use keyring::{Entry, Error};

const SERVICE: &str = "io.github.cduym622.smallstreamerchat";

/// A secret in the credential store, and what to call it in messages.
#[derive(Clone, Copy)]
pub struct Secret {
    account: &'static str,
    label: &'static str,
}

pub const ANTHROPIC_KEY: Secret = Secret {
    account: "anthropic-api-key",
    label: "API key",
};

/// Only the refresh token is stored: it's small enough for Windows
/// Credential Manager, and the short-lived access token is fetched with it.
pub const CHATGPT_REFRESH_TOKEN: Secret = Secret {
    account: "chatgpt-refresh-token",
    label: "ChatGPT sign-in",
};

fn entry(secret: Secret) -> Result<Entry, String> {
    Entry::new(SERVICE, secret.account)
        .map_err(|e| format!("Couldn't open the system keychain: {e}"))
}

pub fn get(secret: Secret) -> Result<Option<String>, String> {
    match entry(secret)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(format!(
            "Couldn't read your {} from the keychain: {e}",
            secret.label
        )),
    }
}

pub fn set(secret: Secret, value: &str) -> Result<(), String> {
    entry(secret)?
        .set_password(value)
        .map_err(|e| format!("Couldn't save your {} in the keychain: {e}", secret.label))
}

pub fn remove(secret: Secret) -> Result<(), String> {
    match entry(secret)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(format!(
            "Couldn't remove your {} from the keychain: {e}",
            secret.label
        )),
    }
}
