//! Secrets live in the OS credential store (macOS Keychain, Windows
//! Credential Manager), never in the settings file.

use keyring::{Entry, Error};

const SERVICE: &str = "io.github.cduym622.smallstreamerchat";
const ANTHROPIC_KEY: &str = "anthropic-api-key";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ANTHROPIC_KEY)
        .map_err(|e| format!("Couldn't open the system keychain: {e}"))
}

pub fn anthropic_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Couldn't read your API key from the keychain: {e}")),
    }
}

pub fn set_anthropic_key(key: &str) -> Result<(), String> {
    entry()?
        .set_password(key)
        .map_err(|e| format!("Couldn't save your API key in the keychain: {e}"))
}

pub fn remove_anthropic_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(format!(
            "Couldn't remove your API key from the keychain: {e}"
        )),
    }
}
