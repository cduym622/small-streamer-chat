//! The AI Connection: an Anthropic API key, a ChatGPT sign-in, or neither
//! (canned lines). When both are set up, the streamer picks which one
//! writes the messages.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_opener::OpenerExt;
use tokio::{sync::oneshot, time::Instant};

use crate::{
    ai::{
        Ai, Anthropic, ChatGpt, Provider,
        chatgpt::{self, Model, Session, Tokens},
    },
    chatgpt_sign_in::{self, Previous},
    engine::{Command, EngineHandle},
    keychain, problems,
    settings::{self, ChatGptAccount},
};

/// Tells the UI the AI Connection changed.
pub const STATUS_EVENT: &str = "ai-status";

/// Stops a sign-in that's waiting for the browser.
#[derive(Default)]
pub struct PendingSignIn(Mutex<Option<oneshot::Sender<()>>>);

/// Sets up the shared ChatGPT sign-in and starts the saved AI Connection.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let on_signed_out = {
        let app = app.clone();
        move || connect(&app)
    };
    let session = Arc::new(Session::new(on_signed_out));
    let settings = settings::load(app);
    if let Some(account) = &settings.chatgpt {
        match keychain::get(keychain::CHATGPT_REFRESH_TOKEN) {
            Ok(Some(refresh_token)) => {
                let tokens = Tokens {
                    client_id: account.client_id.clone(),
                    refresh_token,
                    access_token: None,
                    expires_at: Instant::now(),
                };
                tauri::async_runtime::block_on(session.set_tokens(Some(tokens)));
            }
            Ok(None) => {}
            Err(error) => problems::report(app, error),
        }
    }
    app.manage(session);
    app.manage(PendingSignIn::default());
    connect(app);
}

/// The connection that writes messages: the streamer's choice if it's set
/// up, otherwise whichever one is.
fn active(choice: Option<Provider>, chatgpt_ready: bool, has_key: bool) -> Provider {
    let ready = |provider| match provider {
        Provider::ChatGpt => chatgpt_ready,
        Provider::Anthropic => has_key,
        Provider::None => true,
    };
    match choice {
        Some(choice) if ready(choice) => choice,
        _ if chatgpt_ready => Provider::ChatGpt,
        _ if has_key => Provider::Anthropic,
        _ => Provider::None,
    }
}

/// Hands the engine the AI Connection to use, after any change.
fn connect<R: Runtime>(app: &AppHandle<R>) {
    let session = app.state::<Arc<Session>>().inner().clone();
    let settings = settings::load(app);
    let key = keychain::get(keychain::ANTHROPIC_KEY).unwrap_or_else(|error| {
        problems::report(app, error);
        None
    });
    let chatgpt_ready = settings.chatgpt.is_some() && session.is_signed_in();
    let ai = match active(settings.ai_choice, chatgpt_ready, key.is_some()) {
        Provider::ChatGpt => Some(Ai::ChatGpt(ChatGpt::new(
            session,
            settings.chatgpt.and_then(|account| account.model),
        ))),
        Provider::Anthropic => key.map(|key| Ai::Anthropic(Anthropic::new(key))),
        Provider::None => None,
    };
    app.state::<EngineHandle>().send(Command::SetAi(ai));
    let _ = app.emit(STATUS_EVENT, ());
}

/// Saves which connection writes the messages.
fn choose<R: Runtime>(app: &AppHandle<R>, provider: Provider) -> Result<(), String> {
    let mut settings = settings::load(app);
    settings.ai_choice = Some(provider);
    settings::save(app, &settings).map_err(|e| format!("Couldn't save your choice: {e}"))
}

/// The streamer picked which connection writes the messages.
#[tauri::command]
pub fn set_ai_choice(provider: Provider, app: AppHandle) -> Result<(), String> {
    choose(&app, provider)?;
    connect(&app);
    Ok(())
}

/// What the UI shows. Secrets never go to the UI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    /// The end of the saved Anthropic key, e.g. "…a1b2".
    anthropic_key_hint: Option<String>,
    chatgpt: Option<ChatGptStatus>,
    /// Which one writes the messages.
    active: Provider,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptStatus {
    email: String,
    /// `false` after the sign-in ended; signing in again keeps the account.
    signed_in: bool,
    model: Option<String>,
}

fn key_hint(key: &str) -> String {
    let tail: Vec<char> = key.chars().rev().take(4).collect();
    format!("…{}", tail.into_iter().rev().collect::<String>())
}

#[tauri::command]
pub fn ai_status(app: AppHandle, session: State<Arc<Session>>) -> Result<AiStatus, String> {
    let key = keychain::get(keychain::ANTHROPIC_KEY)?;
    let settings = settings::load(&app);
    let chatgpt = settings.chatgpt.map(|account| ChatGptStatus {
        email: account.email,
        signed_in: session.is_signed_in(),
        model: account.model,
    });
    let chatgpt_ready = chatgpt.as_ref().is_some_and(|status| status.signed_in);
    Ok(AiStatus {
        anthropic_key_hint: key.as_deref().map(key_hint),
        chatgpt,
        active: active(settings.ai_choice, chatgpt_ready, key.is_some()),
    })
}

/// Checks the key with Anthropic, then saves it.
#[tauri::command]
pub async fn save_anthropic_key(key: String, app: AppHandle) -> Result<(), String> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("Paste your Anthropic API key first.".into());
    }
    Anthropic::new(key.clone()).check_key().await?;
    keychain::set(keychain::ANTHROPIC_KEY, &key)?;
    // Connecting something means wanting to use it.
    choose(&app, Provider::Anthropic)?;
    connect(&app);
    Ok(())
}

#[tauri::command]
pub fn remove_anthropic_key(app: AppHandle) -> Result<(), String> {
    keychain::remove(keychain::ANTHROPIC_KEY)?;
    connect(&app);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInResult {
    /// The first sign-in with this account, so the UI explains plan usage.
    first_time: bool,
}

/// "Continue with ChatGPT": signs in through the browser.
#[tauri::command]
pub async fn sign_in_with_chatgpt(
    app: AppHandle,
    pending: State<'_, PendingSignIn>,
    session: State<'_, Arc<Session>>,
) -> Result<SignInResult, String> {
    let (cancel, cancelled) = oneshot::channel();
    // A new sign-in replaces one still waiting.
    if let Some(earlier) = pending.0.lock().unwrap().replace(cancel) {
        let _ = earlier.send(());
    }

    let mut settings = settings::load(&app);
    let host_id = settings
        .chatgpt_host_id
        .get_or_insert_with(chatgpt_sign_in::new_host_id)
        .clone();
    settings::save(&app, &settings).map_err(|e| format!("Couldn't save the settings: {e}"))?;

    // Signing in again after the sign-in ended keeps the same registration.
    let previous = settings.chatgpt.as_ref().map(|account| Previous {
        client_id: &account.client_id,
        email: &account.email,
        subject: &account.subject,
    });
    let first_time = previous.is_none();
    let http = chatgpt::http_client();
    let signed_in = chatgpt_sign_in::sign_in(
        &http,
        &host_id,
        previous,
        |url| {
            app.opener()
                .open_url(url, None::<&str>)
                .map_err(|e| format!("Couldn't open your browser: {e}"))
        },
        cancelled,
    )
    .await?;

    keychain::set(keychain::CHATGPT_REFRESH_TOKEN, &signed_in.refresh_token)?;
    let model = settings.chatgpt.and_then(|account| account.model);
    settings.chatgpt = Some(ChatGptAccount {
        client_id: signed_in.client_id.clone(),
        email: signed_in.email,
        subject: signed_in.subject,
        model,
    });
    // Connecting something means wanting to use it.
    settings.ai_choice = Some(Provider::ChatGpt);
    settings::save(&app, &settings).map_err(|e| format!("Couldn't save the settings: {e}"))?;
    session
        .set_tokens(Some(Tokens {
            client_id: signed_in.client_id,
            refresh_token: signed_in.refresh_token,
            access_token: Some(signed_in.tokens.access_token.clone()),
            expires_at: Instant::now() + signed_in.tokens.expires_in(),
        }))
        .await;
    connect(&app);
    Ok(SignInResult { first_time })
}

#[tauri::command]
pub fn cancel_chatgpt_sign_in(pending: State<PendingSignIn>) {
    if let Some(waiting) = pending.0.lock().unwrap().take() {
        let _ = waiting.send(());
    }
}

/// Signs out and forgets the account. Returns a warning if OpenAI couldn't
/// confirm it.
#[tauri::command]
pub async fn sign_out_of_chatgpt(
    app: AppHandle,
    session: State<'_, Arc<Session>>,
) -> Result<Option<String>, String> {
    let tokens = session.take_tokens().await;
    let mut settings = settings::load(&app);
    settings.chatgpt = None;
    settings::save(&app, &settings).map_err(|e| format!("Couldn't save the settings: {e}"))?;
    keychain::remove(keychain::CHATGPT_REFRESH_TOKEN)?;
    connect(&app);

    let Some(tokens) = tokens else {
        return Ok(None);
    };
    let revoked = chatgpt_sign_in::revoke(
        &chatgpt::http_client(),
        &tokens.client_id,
        &tokens.refresh_token,
    )
    .await;
    Ok(revoked.err())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatGptModels {
    models: Vec<Model>,
    /// The chosen model, or the default when none was chosen.
    selected: Option<String>,
}

#[tauri::command]
pub async fn chatgpt_models(
    app: AppHandle,
    session: State<'_, Arc<Session>>,
) -> Result<ChatGptModels, String> {
    let models = session.models().await?;
    let selected = settings::load(&app)
        .chatgpt
        .and_then(|account| account.model)
        .or_else(|| chatgpt::default_model(&models).map(|model| model.slug.clone()));
    Ok(ChatGptModels { models, selected })
}

#[tauri::command]
pub fn set_chatgpt_model(model: String, app: AppHandle) -> Result<(), String> {
    let mut settings = settings::load(&app);
    let Some(account) = settings.chatgpt.as_mut() else {
        return Err("Sign in with ChatGPT first.".into());
    };
    account.model = Some(model);
    settings::save(&app, &settings).map_err(|e| format!("Couldn't save your choice: {e}"))?;
    connect(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_hints_show_only_the_end() {
        assert_eq!(key_hint("sk-ant-secret-a1b2"), "…a1b2");
    }

    #[test]
    fn uses_the_chosen_connection_while_it_is_set_up() {
        let (chatgpt, anthropic) = (Some(Provider::ChatGpt), Some(Provider::Anthropic));
        assert_eq!(active(anthropic, true, true), Provider::Anthropic);
        assert_eq!(active(chatgpt, true, true), Provider::ChatGpt);
        // The chosen one went away, so the other one takes over.
        assert_eq!(active(chatgpt, false, true), Provider::Anthropic);
        assert_eq!(active(anthropic, true, false), Provider::ChatGpt);
        assert_eq!(active(None, false, false), Provider::None);
        assert_eq!(active(Some(Provider::None), true, true), Provider::None);
    }
}
