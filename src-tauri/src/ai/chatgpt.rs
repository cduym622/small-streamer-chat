//! A model on the streamer's ChatGPT plan, through OpenAI's "Sign in with
//! ChatGPT" for open-source apps: the OpenAI Responses API with an OAuth
//! access token instead of an API key. Requests must use `store: false` and
//! `stream: true`.
//!
//! Docs: https://developers.openai.com/siwc/token-sharing-open-source

use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::Instant;

use super::{Prompt, Reply, messages_schema, parse_messages, system_prompt, user_message};
use crate::keychain;

pub const ISSUER: &str = "https://auth.openai.com";
pub const TOKEN_URL: &str = "https://auth.openai.com/api/accounts/oauth/token";
/// The API the tokens are for, sent as `resource` in every token request.
pub const RESOURCE: &str = "https://api.openai.com/v1";

/// Refresh the access token this long before it expires.
const REFRESH_MARGIN: Duration = Duration::from_secs(5 * 60);
/// Long enough for a reasoning model to finish a few chat lines.
const TIMEOUT: Duration = Duration::from_secs(60);

const SIGNED_OUT: &str =
    "Your ChatGPT sign-in has ended. Continue with ChatGPT again under AI Connection.";
const LIMIT_REACHED: &str = "Usage limit reached. Review your plan or this app's limit in ChatGPT settings (Settings → Usage). The Practice Chat is paused until you press Start again.";
const NOT_ELIGIBLE: &str = "Your ChatGPT plan can't be used in other apps (this needs Go, Plus or Pro, and a workspace that allows it). You can use an Anthropic API key instead. The Practice Chat is paused until you press Start again.";

/// Optional request fields a model might not support. If the API rejects
/// one, it's left out from then on.
const OPTIONAL_FIELDS: [&str; 2] = ["text", "reasoning"];

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .expect("couldn't create the HTTP client")
}

/// The OAuth tokens of the signed-in account.
pub struct Tokens {
    /// Issued to this app for this account at registration.
    pub client_id: String,
    pub refresh_token: String,
    pub access_token: Option<String>,
    pub expires_at: Instant,
}

/// The ChatGPT sign-in, shared by every `ChatGpt` client so the rotating
/// refresh token is only ever used once.
pub struct Session {
    http: reqwest::Client,
    tokens: tokio::sync::Mutex<Option<Tokens>>,
    signed_in: AtomicBool,
    /// The plan's usage limit was hit (or the plan isn't eligible); no more
    /// requests until the next Session.
    paused: AtomicBool,
    unsupported: Mutex<HashSet<&'static str>>,
    on_signed_out: Box<dyn Fn() + Send + Sync>,
}

impl Session {
    /// `on_signed_out` runs when OpenAI ends the sign-in (for example, the
    /// streamer disconnected the app in ChatGPT's settings).
    pub fn new(on_signed_out: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            http: http_client(),
            tokens: tokio::sync::Mutex::new(None),
            signed_in: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            unsupported: Mutex::default(),
            on_signed_out: Box::new(on_signed_out),
        }
    }

    pub fn is_signed_in(&self) -> bool {
        self.signed_in.load(Ordering::SeqCst)
    }

    pub async fn set_tokens(&self, tokens: Option<Tokens>) {
        let mut current = self.tokens.lock().await;
        self.signed_in.store(tokens.is_some(), Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
        self.unsupported.lock().unwrap().clear();
        *current = tokens;
    }

    /// The tokens, for signing out.
    pub async fn take_tokens(&self) -> Option<Tokens> {
        let mut current = self.tokens.lock().await;
        self.signed_in.store(false, Ordering::SeqCst);
        current.take()
    }

    /// A valid access token, refreshed if it's about to expire.
    async fn access_token(&self, force_refresh: bool) -> Result<String, String> {
        // Held while refreshing, so two requests never refresh at once.
        let mut current = self.tokens.lock().await;
        let Some(tokens) = current.as_mut() else {
            return Err(SIGNED_OUT.into());
        };
        if !force_refresh
            && let Some(access) = &tokens.access_token
            && tokens.expires_at > Instant::now() + REFRESH_MARGIN
        {
            return Ok(access.clone());
        }

        let refreshed = token_request(
            &self.http,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &tokens.client_id),
                ("refresh_token", &tokens.refresh_token),
                ("resource", RESOURCE),
            ],
        )
        .await;
        match refreshed {
            Ok(response) => {
                tokens.expires_at = Instant::now() + response.expires_in();
                if let Some(refresh_token) = response.refresh_token {
                    // The old refresh token no longer works, so the new one
                    // must be kept.
                    keychain::set(keychain::CHATGPT_REFRESH_TOKEN, &refresh_token)?;
                    tokens.refresh_token = refresh_token;
                }
                tokens.access_token = Some(response.access_token.clone());
                Ok(response.access_token)
            }
            Err(TokenError::SignedOut) => {
                *current = None;
                self.signed_in.store(false, Ordering::SeqCst);
                drop(current);
                let _ = keychain::remove(keychain::CHATGPT_REFRESH_TOKEN);
                (self.on_signed_out)();
                Err(SIGNED_OUT.into())
            }
            // Temporary problems (offline, OpenAI down) keep the sign-in.
            Err(TokenError::Other(error)) => Err(error),
        }
    }

    /// The models this account may use, in OpenAI's order.
    pub async fn models(&self) -> Result<Vec<Model>, String> {
        let token = self.access_token(false).await?;
        let response = self
            .http
            .get(format!("{RESOURCE}/models"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Couldn't reach ChatGPT: {e}"))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|e| format!("Unexpected model list from ChatGPT: {e}"))?;
        if !status.is_success() {
            return Err(Failure::from_body(status.as_u16(), &body).message(""));
        }
        Ok(parse_models(&body))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Model {
    pub slug: String,
    pub name: String,
}

fn parse_models(body: &Value) -> Vec<Model> {
    body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|model| model["visibility"].as_str().is_none_or(|v| v == "list"))
        .filter_map(|model| {
            let slug = model["slug"].as_str()?.to_string();
            let name = model["display_name"].as_str().unwrap_or(&slug).to_string();
            Some(Model { slug, name })
        })
        .collect()
}

/// The smallest, fastest model on offer: chat lines don't need a big model,
/// and a fast one replies while the moment is still fresh. Coding models are
/// a last resort.
pub fn default_model(models: &[Model]) -> Option<&Model> {
    let rank = |model: &&Model| {
        let slug = model.slug.to_lowercase();
        let size = if slug.contains("nano") {
            0
        } else if slug.contains("mini") {
            1
        } else {
            2
        };
        (slug.contains("codex"), size)
    };
    // `min_by_key` keeps the first of equals, so OpenAI's order breaks ties.
    models.iter().min_by_key(rank)
}

/// Writes messages with one model on the plan.
#[derive(Clone)]
pub struct ChatGpt {
    session: Arc<Session>,
    /// `None` uses the default model, looked up on first use.
    model: Option<String>,
    looked_up: Arc<Mutex<Option<String>>>,
}

impl ChatGpt {
    pub fn new(session: Arc<Session>, model: Option<String>) -> Self {
        Self {
            session,
            model,
            looked_up: Arc::default(),
        }
    }

    pub fn resume(&self) {
        self.session.paused.store(false, Ordering::SeqCst);
    }

    async fn model(&self) -> Result<String, String> {
        if let Some(model) = self
            .model
            .clone()
            .or_else(|| self.looked_up.lock().unwrap().clone())
        {
            return Ok(model);
        }
        let models = self.session.models().await?;
        let model = default_model(&models)
            .ok_or("Your ChatGPT plan didn't offer any models to use.")?
            .slug
            .clone();
        *self.looked_up.lock().unwrap() = Some(model.clone());
        Ok(model)
    }

    pub async fn write(&self, prompt: &Prompt<'_>) -> Result<Reply, String> {
        let session = &self.session;
        if session.paused.load(Ordering::SeqCst) {
            return Ok(Reply {
                messages: Vec::new(),
                cost_usd: 0.0,
            });
        }
        let model = self.model().await?;
        let mut force_refresh = false;
        // A retry after leaving out an unsupported field or refreshing a
        // rejected token.
        for _ in 0..=OPTIONAL_FIELDS.len() + 1 {
            let token = session.access_token(force_refresh).await?;
            let body = {
                let unsupported = session.unsupported.lock().unwrap();
                request_body(prompt, &model, &unsupported)
            };
            let failure = match self.send(&token, &body).await {
                Ok(Some(text)) => {
                    return Ok(Reply {
                        messages: parse_messages(&text),
                        cost_usd: 0.0,
                    });
                }
                // Cut off: post nothing this time.
                Ok(None) => {
                    return Ok(Reply {
                        messages: Vec::new(),
                        cost_usd: 0.0,
                    });
                }
                Err(failure) => failure,
            };

            match failure.code.as_deref() {
                Some("subscription_sharing_unsupported_capability") => {
                    let field = OPTIONAL_FIELDS.into_iter().find(|field| {
                        failure
                            .param
                            .as_deref()
                            .is_some_and(|p| p.starts_with(field))
                    });
                    let mut unsupported = session.unsupported.lock().unwrap();
                    if let Some(field) = field
                        && unsupported.insert(field)
                    {
                        continue;
                    }
                }
                Some("subscription_sharing_usage_limit_exceeded") => {
                    session.paused.store(true, Ordering::SeqCst);
                    return Err(LIMIT_REACHED.into());
                }
                Some("subscription_sharing_user_not_eligible") => {
                    session.paused.store(true, Ordering::SeqCst);
                    return Err(NOT_ELIGIBLE.into());
                }
                _ if failure.status == Some(401) && !force_refresh => {
                    force_refresh = true;
                    continue;
                }
                _ => {}
            }
            return Err(failure.message(&model));
        }
        Err("ChatGPT kept rejecting the request.".into())
    }

    /// Sends one request and reads the whole event stream. `Ok(None)` means
    /// the reply was cut off.
    async fn send(&self, token: &str, body: &Value) -> Result<Option<String>, Failure> {
        let response = self
            .session
            .http
            .post(format!("{RESOURCE}/responses"))
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|e| Failure::network(&e))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.json::<Value>().await.unwrap_or_default();
            return Err(Failure::from_body(status.as_u16(), &body));
        }
        let events = response.text().await.map_err(|e| Failure::network(&e))?;
        read_events(&events)
    }
}

fn request_body(prompt: &Prompt, model: &str, unsupported: &HashSet<&str>) -> Value {
    let mut body = json!({
        "model": model,
        "instructions": format!(
            "{}\n\nReply with only JSON like {{\"messages\": [{{\"chatter\": \"username\", \"text\": \"message\"}}]}}.",
            system_prompt()
        ),
        "input": [{ "role": "user", "content": user_message(prompt) }],
        "store": false,
        "stream": true,
    });
    if !unsupported.contains("text") {
        body["text"] = json!({
            "format": {
                "type": "json_schema",
                "name": "chat_messages",
                "strict": true,
                "schema": messages_schema()
            }
        });
    }
    if !unsupported.contains("reasoning") {
        // Chat lines are simple; little reasoning keeps replies fast.
        body["reasoning"] = json!({ "effort": "low" });
    }
    body
}

/// Reads a server-sent event stream: the reply's text once it's complete.
fn read_events(stream: &str) -> Result<Option<String>, Failure> {
    let mut text = String::new();
    for data in stream.lines().filter_map(|line| line.strip_prefix("data:")) {
        let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        match event["type"].as_str() {
            Some("response.output_text.delta") => {
                text.push_str(event["delta"].as_str().unwrap_or_default());
            }
            Some("response.completed") => return Ok(Some(text)),
            Some("response.incomplete") => return Ok(None),
            Some("response.failed") => {
                return Err(Failure::from_error(None, &event["response"]["error"]));
            }
            Some("error") => {
                let error = if event["error"].is_object() {
                    &event["error"]
                } else {
                    &event
                };
                return Err(Failure::from_error(None, error));
            }
            _ => {}
        }
    }
    Err(Failure {
        status: None,
        code: None,
        param: None,
        detail: Some("the reply stopped before it was finished".into()),
    })
}

/// A failed request, from the HTTP status or an error in the event stream.
#[derive(Debug)]
struct Failure {
    status: Option<u16>,
    code: Option<String>,
    param: Option<String>,
    detail: Option<String>,
}

impl Failure {
    fn network(error: &reqwest::Error) -> Self {
        Self {
            status: None,
            code: Some("network".into()),
            param: None,
            detail: Some(error.to_string()),
        }
    }

    fn from_body(status: u16, body: &Value) -> Self {
        if body["error"].is_object() {
            return Self::from_error(Some(status), &body["error"]);
        }
        // Some errors come as `{"detail": "..."}` instead.
        Self {
            status: Some(status),
            code: None,
            param: None,
            detail: body["detail"].as_str().map(str::to_string),
        }
    }

    fn from_error(status: Option<u16>, error: &Value) -> Self {
        let text = |key: &str| error[key].as_str().map(str::to_string);
        Self {
            status,
            code: text("code"),
            param: text("param"),
            detail: text("message"),
        }
    }

    fn message(&self, model: &str) -> String {
        let detail = self.detail.clone().unwrap_or_default();
        match (self.code.as_deref(), self.status) {
            (Some("network"), _) => format!("Couldn't reach ChatGPT: {detail}"),
            (Some("subscription_sharing_unsupported_capability"), _) => format!(
                "ChatGPT can't write chat messages with {model} ({detail}). Pick another model under AI Connection."
            ),
            (
                Some("subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable"),
                _,
            )
            | (_, Some(503)) => {
                "ChatGPT couldn't check your plan's usage right now, so the chat will be quieter for a bit.".into()
            }
            (_, Some(401)) => SIGNED_OUT.into(),
            (_, Some(429)) => {
                "ChatGPT is limiting how fast requests can be sent, so the chat will be slower for a bit.".into()
            }
            (_, Some(500..=599)) => {
                "ChatGPT is having trouble right now, so the chat will be quieter for a bit.".into()
            }
            _ if detail.is_empty() => format!(
                "ChatGPT returned an error ({}).",
                self.code
                    .clone()
                    .or(self.status.map(|status| format!("HTTP {status}")))
                    .unwrap_or_else(|| "unknown".into())
            ),
            _ => format!("ChatGPT returned an error: {detail}"),
        }
    }
}

#[derive(Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_in: Option<u64>,
    /// Space-separated scopes granted.
    pub scope: Option<String>,
}

impl TokenResponse {
    pub fn expires_in(&self) -> Duration {
        Duration::from_secs(self.expires_in.unwrap_or(3600))
    }
}

pub enum TokenError {
    /// The sign-in can't be used any more; the streamer has to sign in again.
    SignedOut,
    Other(String),
}

/// A form-encoded request to the OAuth token endpoint.
pub async fn token_request(
    http: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<TokenResponse, TokenError> {
    let response = http
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| TokenError::Other(format!("Couldn't reach ChatGPT: {e}")))?;
    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .await
            .map_err(|e| TokenError::Other(format!("Unexpected reply from ChatGPT: {e}")));
    }
    let body = response.json::<Value>().await.unwrap_or_default();
    // Usually `{"error": "invalid_grant"}`, sometimes `{"error": {"code": ...}}`.
    let code = body["error"]
        .as_str()
        .or(body["error"]["code"].as_str())
        .unwrap_or_default();
    match code {
        "invalid_grant"
        | "invalid_refresh_token"
        | "token_expired"
        | "refresh_token_expired"
        | "refresh_token_invalidated"
        | "refresh_token_reused"
        | "invalid_client" => Err(TokenError::SignedOut),
        _ => Err(TokenError::Other(format!(
            "ChatGPT sign-in failed ({}).",
            if code.is_empty() {
                format!("HTTP {status}")
            } else {
                code.to_string()
            }
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::Reason;

    fn model(slug: &str) -> Model {
        Model {
            slug: slug.into(),
            name: slug.into(),
        }
    }

    #[test]
    fn reads_the_text_of_a_completed_stream() {
        let stream = "event: response.created\ndata: {\"type\":\"response.created\"}\n\n\
            data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\\\"messages\\\":\"}\n\n\
            data: {\"type\":\"response.output_text.delta\",\"delta\":\"[]}\"}\n\n\
            data: {\"type\":\"response.completed\",\"response\":{}}\n\n";
        assert_eq!(
            read_events(stream).unwrap(),
            Some("{\"messages\":[]}".into())
        );
    }

    #[test]
    fn a_cut_off_stream_posts_nothing_and_a_broken_one_is_an_error() {
        let incomplete = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\"}\n\
            data: {\"type\":\"response.incomplete\"}\n";
        assert_eq!(read_events(incomplete).unwrap(), None);
        assert!(
            read_events("data: {\"type\":\"response.output_text.delta\",\"delta\":\"{\"}\n")
                .is_err()
        );
    }

    #[test]
    fn reads_a_usage_limit_error_from_the_stream() {
        let stream = "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"subscription_sharing_usage_limit_exceeded\",\"message\":\"limit\"}}}\n";
        let failure = read_events(stream).unwrap_err();
        assert_eq!(
            failure.code.as_deref(),
            Some("subscription_sharing_usage_limit_exceeded")
        );
    }

    #[test]
    fn requests_follow_the_plan_usage_rules() {
        let prompt = Prompt {
            stream_context: "",
            heard: &[],
            recent_chat: &[],
            reason: Reason::Quiet,
        };
        let body = request_body(&prompt, "some-model", &HashSet::new());
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"][0]["role"], "user");
        assert!(body["text"]["format"]["schema"].is_object());
        // Fields the preview rejects.
        for field in [
            "temperature",
            "max_output_tokens",
            "previous_response_id",
            "user",
        ] {
            assert!(body.get(field).is_none(), "{field} is sent");
        }

        let without = request_body(&prompt, "some-model", &HashSet::from(["text", "reasoning"]));
        assert!(without.get("text").is_none());
        assert!(without.get("reasoning").is_none());
    }

    #[test]
    fn lists_only_models_meant_for_display() {
        let body = json!({ "models": [
            { "slug": "gpt-a", "display_name": "GPT A", "visibility": "list" },
            { "slug": "gpt-hidden", "display_name": "Hidden", "visibility": "hide" },
        ]});
        assert_eq!(
            parse_models(&body),
            vec![Model {
                slug: "gpt-a".into(),
                name: "GPT A".into()
            }]
        );
    }

    #[test]
    fn prefers_a_small_general_model() {
        let models = [
            model("gpt-6.1-sol"),
            model("gpt-6-codex-mini"),
            model("gpt-6-mini"),
            model("gpt-6-mini-2"),
        ];
        assert_eq!(default_model(&models).unwrap().slug, "gpt-6-mini");
        assert_eq!(
            default_model(&[model("gpt-6-codex"), model("gpt-6")])
                .unwrap()
                .slug,
            "gpt-6"
        );
        assert!(default_model(&[]).is_none());
    }

    #[test]
    fn explains_errors() {
        let failure = Failure::from_body(503, &json!({ "detail": "direct routing unavailable" }));
        assert!(failure.message("m").contains("quieter"));
        let failure = Failure::from_body(
            400,
            &json!({ "error": { "code": "subscription_sharing_unsupported_capability", "param": "reasoning.effort", "message": "nope" } }),
        );
        assert!(failure.message("gpt-x").contains("gpt-x"));
        assert_eq!(failure.param.as_deref(), Some("reasoning.effort"));
    }
}
