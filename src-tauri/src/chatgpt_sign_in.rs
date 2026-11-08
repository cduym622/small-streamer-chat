//! "Continue with ChatGPT": the OAuth sign-in that lets the Practice Chat use
//! the streamer's ChatGPT plan. It opens the system browser and waits for
//! OpenAI to redirect back to a listener on 127.0.0.1:1455.
//!
//! Docs: https://developers.openai.com/siwc/token-sharing-open-source/sign-in

use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    time::{sleep, timeout},
};

use crate::ai::chatgpt::{ISSUER, RESOURCE, TokenError, TokenResponse, token_request};

const AUTHORIZE_URL: &str = "https://auth.openai.com/api/accounts/authorize";
/// Registered with OpenAI the first time, so it has to stay the same.
const PORT: u16 = 1455;
const CALLBACK_PATH: &str = "/auth/callback";
/// The first sign-in registers this app under this name in the streamer's
/// ChatGPT settings.
const APP_NAME: &str = "Small Streamer Chat";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const PLAN_USAGE_SCOPE: &str = "chatgpt.tokens.use.direct";
/// Before registration this placeholder is sent instead of a client ID.
const REGISTER: &str = "dynamic_agent_client";
const GIVE_UP_AFTER: Duration = Duration::from_secs(10 * 60);

/// An earlier sign-in to sign in to again, after the sign-in ended.
pub struct Previous<'a> {
    pub client_id: &'a str,
    pub email: &'a str,
    pub subject: &'a str,
}

pub struct SignedIn {
    pub client_id: String,
    pub email: String,
    pub subject: String,
    pub tokens: TokenResponse,
    pub refresh_token: String,
}

/// An opaque ID for this computer, which OpenAI asks for so a client can be
/// used on several machines. Made once and kept in the settings.
pub fn new_host_id() -> String {
    let mut bytes = random_bytes::<16>();
    // UUID version 4, variant 1.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the OS has no random number source");
    bytes
}

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn redirect_uri() -> String {
    // OpenAI requires the IP address, not `localhost`.
    format!("http://127.0.0.1:{PORT}{CALLBACK_PATH}")
}

/// Signs in through the browser. `cancel` stops waiting.
pub async fn sign_in(
    http: &reqwest::Client,
    host_id: &str,
    previous: Option<Previous<'_>>,
    open_browser: impl FnOnce(&str) -> Result<(), String>,
    cancel: oneshot::Receiver<()>,
) -> Result<SignedIn, String> {
    let listener = listen().await?;
    let state = random_token();
    let nonce = random_token();
    let verifier = random_token();
    let redirect_uri = redirect_uri();

    let mut params = vec![
        (
            "client_id",
            previous.as_ref().map_or(REGISTER, |p| p.client_id),
        ),
        ("ext_agent_host_id", host_id),
        ("response_type", "code"),
        ("redirect_uri", &redirect_uri),
        ("scope", SCOPES),
        ("resource", RESOURCE),
        ("state", &state),
        ("nonce", &nonce),
        ("code_challenge_method", "S256"),
    ];
    let challenge = pkce_challenge(&verifier);
    params.push(("code_challenge", &challenge));
    match &previous {
        Some(previous) => params.push(("login_hint", previous.email)),
        None => params.push(("agent_name_hint", APP_NAME)),
    }
    let url = Url::parse_with_params(AUTHORIZE_URL, &params).expect("valid authorize URL");
    open_browser(url.as_str())?;

    let callback = tokio::select! {
        callback = wait_for_callback(listener, state) => callback?,
        _ = cancel => return Err("Sign-in cancelled.".into()),
        () = sleep(GIVE_UP_AFTER) => {
            return Err("Sign-in took too long. Press Continue with ChatGPT to try again.".into());
        }
    };

    let client_id = match (&previous, callback.client_id) {
        (None, Some(issued)) if issued != REGISTER => issued,
        (None, _) => {
            return Err(
                "ChatGPT didn't finish registering Small Streamer Chat. Please try again.".into(),
            );
        }
        (Some(previous), Some(issued)) if issued != previous.client_id => {
            return Err(
                "ChatGPT answered for a different registration. Sign out, then sign in again."
                    .into(),
            );
        }
        (Some(previous), _) => previous.client_id.to_string(),
    };

    let tokens = token_request(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", &callback.code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect_uri),
            ("resource", RESOURCE),
        ],
    )
    .await
    .map_err(|error| match error {
        TokenError::SignedOut => "ChatGPT didn't accept the sign-in. Please try again.".to_string(),
        TokenError::Other(error) => error,
    })?;

    let claims = tokens
        .id_token
        .as_deref()
        .ok_or_else(|| "ChatGPT didn't say who signed in.".to_string())
        .and_then(|token| verified_claims(token, &client_id, &nonce))?;
    if let Some(previous) = &previous
        && claims.sub != previous.subject
    {
        return Err(format!(
            "You signed in with a different ChatGPT account than {}. To switch accounts, sign out first.",
            previous.email
        ));
    }

    let scopes = tokens
        .scope
        .as_deref()
        .or(callback.scope.as_deref())
        .unwrap_or_default();
    if !scopes
        .split_whitespace()
        .any(|scope| scope == PLAN_USAGE_SCOPE)
    {
        return Err("You signed in, but didn't allow Small Streamer Chat to use your ChatGPT plan. Continue with ChatGPT again and allow plan usage, or use an Anthropic API key.".into());
    }
    let refresh_token = tokens
        .refresh_token
        .clone()
        .ok_or("ChatGPT didn't allow staying signed in. Please try again.")?;

    Ok(SignedIn {
        client_id,
        email: claims.email.unwrap_or_default(),
        subject: claims.sub,
        tokens,
        refresh_token,
    })
}

/// Opens the callback listener. An earlier sign-in that was just cancelled
/// may still be letting go of the port.
async fn listen() -> Result<TcpListener, String> {
    let mut tries = 0;
    loop {
        match TcpListener::bind(("127.0.0.1", PORT)).await {
            Ok(listener) => return Ok(listener),
            Err(_) if tries < 10 => {
                tries += 1;
                sleep(Duration::from_millis(200)).await;
            }
            Err(e) => {
                return Err(format!(
                    "Couldn't sign in: port {PORT} is in use ({e}). Another app may be signing in to ChatGPT (such as Codex); finish or close that, then try again."
                ));
            }
        }
    }
}

struct Callback {
    code: String,
    client_id: Option<String>,
    scope: Option<String>,
}

/// Waits for the browser to come back with this sign-in's `state`.
async fn wait_for_callback(listener: TcpListener, state: String) -> Result<Callback, String> {
    let (results, mut received) = mpsc::unbounded_channel();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((connection, _)) = accepted else { continue };
                // Each connection on its own: browsers open idle ones too.
                let results = results.clone();
                let state = state.clone();
                tokio::spawn(async move {
                    if let Some(result) = handle_connection(connection, &state).await {
                        let _ = results.send(result);
                    }
                });
            }
            Some(result) = received.recv() => return result,
        }
    }
}

/// Answers one browser request. `Some` if it was this sign-in's callback.
async fn handle_connection(
    mut connection: TcpStream,
    state: &str,
) -> Option<Result<Callback, String>> {
    let target = timeout(
        Duration::from_secs(30),
        read_request_target(&mut connection),
    )
    .await
    .ok()
    .flatten()?;
    let url = Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != CALLBACK_PATH {
        respond(&mut connection, "404 Not Found", "Not found.").await;
        return None;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if param("state").as_deref() != Some(state) {
        respond(
            &mut connection,
            "400 Bad Request",
            "This sign-in link is out of date. Go back to Small Streamer Chat and press Continue with ChatGPT again.",
        )
        .await;
        return None;
    }
    if let Some(error) = param("error") {
        respond(
            &mut connection,
            "200 OK",
            "Sign-in cancelled. You can close this tab.",
        )
        .await;
        return Some(Err(if error == "access_denied" {
            "Sign-in cancelled.".into()
        } else {
            format!("ChatGPT sign-in failed ({error}).")
        }));
    }
    let Some(code) = param("code") else {
        respond(&mut connection, "400 Bad Request", "Sign-in failed.").await;
        return Some(Err("ChatGPT sign-in failed (no code).".into()));
    };
    respond(
        &mut connection,
        "200 OK",
        "You're signed in. You can close this tab and go back to Small Streamer Chat.",
    )
    .await;
    Some(Ok(Callback {
        code,
        client_id: param("client_id"),
        scope: param("scope"),
    }))
}

/// The path and query of an HTTP request, from `GET <target> HTTP/1.1`.
async fn read_request_target(connection: &mut TcpStream) -> Option<String> {
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        let read = connection.read(&mut buffer).await.ok()?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request);
    let mut parts = request.lines().next()?.split_whitespace();
    (parts.next()? == "GET").then_some(())?;
    Some(parts.next()?.to_string())
}

async fn respond(connection: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Small Streamer Chat</title>\
         <body style=\"font-family: system-ui, sans-serif; margin: 15vh auto; max-width: 32em; text-align: center\">\
         <h1>Small Streamer Chat</h1><p>{message}</p>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = connection.write_all(response.as_bytes()).await;
    let _ = connection.shutdown().await;
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    /// A string or a list of strings.
    aud: Value,
    exp: u64,
    nonce: Option<String>,
    email: Option<String>,
}

/// The ID token's claims, after checking it's for this sign-in. It came
/// straight from OpenAI's token endpoint over TLS, which OpenID Connect
/// accepts in place of checking the signature.
fn verified_claims(id_token: &str, client_id: &str, nonce: &str) -> Result<Claims, String> {
    let invalid =
        |why: &str| format!("ChatGPT's sign-in didn't check out ({why}). Please try again.");
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| invalid("malformed"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|_| invalid("malformed"))?;
    let claims: Claims = serde_json::from_slice(&payload).map_err(|_| invalid("malformed"))?;

    let audience_matches = match &claims.aud {
        Value::String(aud) => aud == client_id,
        Value::Array(auds) => auds.iter().any(|aud| aud == client_id),
        _ => false,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    if claims.iss != ISSUER {
        return Err(invalid("issuer"));
    }
    if !audience_matches {
        return Err(invalid("audience"));
    }
    // A little leeway for a computer clock that's slightly off.
    if claims.exp + 300 < now {
        return Err(invalid("expired"));
    }
    if claims.nonce.as_deref() != Some(nonce) {
        return Err(invalid("nonce"));
    }
    Ok(claims)
}

/// Ends the sign-in on OpenAI's side. Already-invalid tokens count as done.
pub async fn revoke(
    http: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
) -> Result<(), String> {
    let failed = |e: String| {
        format!(
            "Couldn't confirm the sign-out with ChatGPT ({e}). To be sure, disconnect Small Streamer Chat in ChatGPT under Settings → Security and login."
        )
    };
    let configuration: Value = http
        .get(format!("{ISSUER}/.well-known/openid-configuration"))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| failed(e.to_string()))?
        .json()
        .await
        .map_err(|e| failed(e.to_string()))?;
    let endpoint = configuration["revocation_endpoint"]
        .as_str()
        .ok_or_else(|| failed("no revocation endpoint".into()))?;
    http.post(endpoint)
        .form(&[
            ("token", refresh_token),
            ("token_type_hint", "refresh_token"),
            ("client_id", client_id),
        ])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| failed(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id_token(claims: Value) -> String {
        format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(claims.to_string()))
    }

    fn claims(nonce: &str, aud: Value) -> Value {
        serde_json::json!({
            "iss": ISSUER,
            "sub": "user-1",
            "aud": aud,
            "exp": 4_000_000_000u64,
            "nonce": nonce,
            "email": "streamer@example.com",
        })
    }

    #[test]
    fn accepts_an_id_token_for_this_sign_in() {
        let token = id_token(claims("n1", "client-1".into()));
        let checked = verified_claims(&token, "client-1", "n1").unwrap();
        assert_eq!(checked.sub, "user-1");
        assert_eq!(checked.email.as_deref(), Some("streamer@example.com"));
        let listed = id_token(claims("n1", serde_json::json!(["client-1"])));
        assert!(verified_claims(&listed, "client-1", "n1").is_ok());
    }

    #[test]
    fn rejects_an_id_token_for_another_client_or_sign_in() {
        let token = id_token(claims("n1", "client-1".into()));
        assert!(verified_claims(&token, "client-2", "n1").is_err());
        assert!(verified_claims(&token, "client-1", "n2").is_err());
        assert!(verified_claims("garbage", "client-1", "n1").is_err());
    }

    #[test]
    fn makes_pkce_values_and_host_ids_in_the_right_format() {
        // The example from RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(random_token().len(), 43);
        let host = new_host_id();
        assert!(host.starts_with("urn:uuid:"));
        assert_eq!(host.len(), "urn:uuid:".len() + 36);
        assert_eq!(&host[23..24], "4");
    }

    #[test]
    fn the_callback_listener_checks_the_state() {
        tauri::async_runtime::block_on(async {
            let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let waiting = tokio::spawn(wait_for_callback(listener, "good".into()));
            let get = |target: String| async move {
                let mut connection = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
                connection
                    .write_all(format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut response = String::new();
                connection.read_to_string(&mut response).await.unwrap();
                response
            };
            assert!(get("/favicon.ico".into()).await.starts_with("HTTP/1.1 404"));
            assert!(
                get("/auth/callback?state=bad&code=c".into())
                    .await
                    .contains("out of date")
            );
            assert!(
                get("/auth/callback?state=good&code=c%2F1&client_id=app_1".into())
                    .await
                    .contains("signed in")
            );
            let callback = waiting.await.unwrap().unwrap();
            assert_eq!(callback.code, "c/1");
            assert_eq!(callback.client_id.as_deref(), Some("app_1"));
        });
    }
}
