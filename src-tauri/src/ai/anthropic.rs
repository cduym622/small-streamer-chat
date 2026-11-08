//! Claude Haiku through the Anthropic API, with the streamer's API key.
//!
//! There's no official Anthropic SDK for Rust, so this calls the HTTP API
//! directly.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Prompt, Reply, messages_schema, network_error, parse_messages, system_prompt, user_message,
};

const API: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2026-10-21";
const MODEL: &str = "claude-haiku-5-5";
/// Room for Haiku's (short, low-effort) thinking plus a few chat lines.
const MAX_TOKENS: u32 = 1024;

/// Claude Haiku 5.5 prices in US dollars per million tokens, for prompts of
/// 100K tokens or fewer (ours are a few thousand).
const INPUT_PRICE: f64 = 0.10;
const OUTPUT_PRICE: f64 = 0.50;
const CACHE_WRITE_PRICE: f64 = INPUT_PRICE * 1.25;
const CACHE_READ_PRICE: f64 = INPUT_PRICE * 0.1;

#[derive(Clone)]
pub struct Anthropic {
    http: reqwest::Client,
    key: String,
}

impl Anthropic {
    pub fn new(key: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("couldn't create the HTTP client");
        Self { http, key }
    }

    /// Checks the key without spending anything (looking up a model is free).
    pub async fn check_key(&self) -> Result<(), String> {
        let response = self
            .http
            .get(format!("{API}/models/{MODEL}"))
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION)
            .send()
            .await
            .map_err(|e| network_error("Anthropic", &e))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(error_message(response).await)
        }
    }

    pub async fn write(&self, prompt: &Prompt<'_>) -> Result<Reply, String> {
        let response = self
            .http
            .post(format!("{API}/messages"))
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION)
            .json(&request_body(prompt))
            .send()
            .await
            .map_err(|e| network_error("Anthropic", &e))?;
        if !response.status().is_success() {
            return Err(error_message(response).await);
        }
        let response: ApiResponse = response
            .json()
            .await
            .map_err(|e| format!("Unexpected reply from Anthropic: {e}"))?;
        Ok(parse_reply(response))
    }
}

fn request_body(prompt: &Prompt) -> Value {
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        // Chat messages are short and simple; low effort keeps them fast
        // and cheap (the model can skip thinking entirely).
        "output_config": {
            "effort": "low",
            "format": { "type": "json_schema", "schema": messages_schema() }
        },
        // The system prompt never changes, so it's cached across requests.
        "system": [{
            "type": "text",
            "text": system_prompt(),
            "cache_control": { "type": "ephemeral" }
        }],
        "messages": [{ "role": "user", "content": user_message(prompt) }]
    })
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<Block>,
    stop_reason: Option<String>,
    usage: Usage,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

impl Usage {
    fn cost_usd(&self) -> f64 {
        let per_token = |tokens: u64, price: f64| tokens as f64 * price / 1_000_000.0;
        per_token(self.input_tokens, INPUT_PRICE)
            + per_token(self.output_tokens, OUTPUT_PRICE)
            + per_token(
                self.cache_creation_input_tokens.unwrap_or(0),
                CACHE_WRITE_PRICE,
            )
            + per_token(self.cache_read_input_tokens.unwrap_or(0), CACHE_READ_PRICE)
    }
}

/// A refusal or a cut-off reply posts nothing, but still costs.
fn parse_reply(response: ApiResponse) -> Reply {
    let cost_usd = response.usage.cost_usd();
    let complete = response.stop_reason.as_deref() == Some("end_turn");
    let messages = response
        .content
        .iter()
        .find(|block| block.kind == "text")
        .and_then(|block| block.text.as_deref())
        .filter(|_| complete)
        .map(parse_messages)
        .unwrap_or_default();
    Reply { messages, cost_usd }
}

/// A message for the streamer from an API error response.
async fn error_message(response: reqwest::Response) -> String {
    let status = response.status().as_u16();
    let detail = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|body| body["error"]["message"].as_str().map(str::to_string));
    match status {
        401 => "Anthropic didn't accept your API key. Check it under AI Connection.".into(),
        403 => "Your Anthropic API key isn't allowed to use Claude Haiku.".into(),
        429 => "Anthropic is limiting how fast this key can send requests, so the chat will be slower for a bit.".into(),
        500..=599 => "Anthropic is having trouble right now, so the chat will be quieter for a bit.".into(),
        _ => format!(
            "Anthropic returned an error: {}",
            detail.unwrap_or_else(|| format!("HTTP {status}"))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ai::{Heard, Reason},
        personas::PERSONAS,
    };

    fn response(stop_reason: &str, text: &str) -> ApiResponse {
        ApiResponse {
            content: vec![
                Block {
                    kind: "thinking".into(),
                    text: None,
                },
                Block {
                    kind: "text".into(),
                    text: Some(text.into()),
                },
            ],
            stop_reason: Some(stop_reason.into()),
            usage: Usage {
                input_tokens: 1_000,
                output_tokens: 100,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: Some(2_000),
            },
        }
    }

    /// Sends one real request (costs a fraction of a cent):
    ///
    /// ```sh
    /// SSC_ANTHROPIC_KEY=sk-ant-... cargo test writes_real_messages -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs an Anthropic API key and spends a little of its credit"]
    fn writes_real_messages() {
        let ai = Anthropic::new(std::env::var("SSC_ANTHROPIC_KEY").expect("set SSC_ANTHROPIC_KEY"));
        let heard = [Heard {
            ago: Duration::from_secs(5),
            text: "Okay chat, last boss. I've died here like six times, wish me luck.".into(),
        }];
        let prompt = Prompt {
            stream_context: "Elden Ring, first playthrough",
            heard: &heard,
            recent_chat: &[],
            reason: Reason::Pause,
        };
        tauri::async_runtime::block_on(async {
            ai.check_key().await.expect("key check failed");
            let started = std::time::Instant::now();
            let reply = ai.write(&prompt).await.expect("request failed");
            println!("took {:?}, cost ${:.6}", started.elapsed(), reply.cost_usd);
            for (persona, text) in &reply.messages {
                println!("{}: {text}", PERSONAS[*persona].username);
            }
            assert!(!reply.messages.is_empty());
        });
    }

    #[test]
    fn refusals_and_cut_off_replies_post_nothing_but_still_cost() {
        for stop_reason in ["refusal", "max_tokens"] {
            let reply = parse_reply(response(
                stop_reason,
                r#"{"messages":[{"chatter":"curious_ava","text":"hi"}]}"#,
            ));
            assert!(reply.messages.is_empty());
            assert!(reply.cost_usd > 0.0);
        }
    }

    #[test]
    fn cost_counts_every_kind_of_token() {
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_input_tokens: Some(1_000_000),
            cache_read_input_tokens: Some(1_000_000),
        };
        let expected = INPUT_PRICE + OUTPUT_PRICE + CACHE_WRITE_PRICE + CACHE_READ_PRICE;
        assert!((usage.cost_usd() - expected).abs() < 1e-9);
    }

    #[test]
    fn the_request_has_the_prompt_and_no_sampling_parameters() {
        let body = request_body(&Prompt {
            stream_context: "Elden Ring, first playthrough",
            heard: &[],
            recent_chat: &[],
            reason: Reason::Pause,
        });
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("Elden Ring, first playthrough"));
        assert!(
            body["system"][0]["text"]
                .as_str()
                .unwrap()
                .contains("curious_ava")
        );
        // Sampling parameters are rejected by this API version.
        assert!(body.get("temperature").is_none());
    }
}
