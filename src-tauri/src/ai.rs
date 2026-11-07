//! Writes Practice Chat messages with Claude Haiku through the Anthropic API.
//!
//! There's no official Anthropic SDK for Rust, so this calls the HTTP API
//! directly. One request asks for 1-3 short messages, each from one of the
//! Personas, as structured JSON.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{chat::ChatMessage, personas::PERSONAS};

const API: &str = "https://api.anthropic.com/v1";
const API_VERSION: &str = "2026-10-21";
const MODEL: &str = "claude-haiku-5-5";
/// Room for Haiku's (short, low-effort) thinking plus a few chat lines.
const MAX_TOKENS: u32 = 1024;
/// Chat messages longer than this are cut, like a real chat's length limit.
const MAX_MESSAGE_CHARS: usize = 200;
const MAX_MESSAGES: usize = 3;

/// Claude Haiku 5.5 prices in US dollars per million tokens, for prompts of
/// 100K tokens or fewer (ours are a few thousand).
const INPUT_PRICE: f64 = 0.10;
const OUTPUT_PRICE: f64 = 0.50;
const CACHE_WRITE_PRICE: f64 = INPUT_PRICE * 1.25;
const CACHE_READ_PRICE: f64 = INPUT_PRICE * 0.1;

/// Why the Practice Chat is about to say something.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reason {
    SessionStart,
    /// The streamer just stopped talking.
    Pause,
    /// The streamer hasn't said anything for a while.
    Quiet,
    Nudge,
}

/// Something the streamer said, and how long ago.
pub struct Heard {
    pub ago: Duration,
    pub text: String,
}

pub struct Prompt<'a> {
    pub stream_context: &'a str,
    pub heard: &'a [Heard],
    pub recent_chat: &'a [ChatMessage],
    pub reason: Reason,
}

pub struct Reply {
    /// (Persona index, message text), in the order to post them.
    pub messages: Vec<(usize, String)>,
    pub cost_usd: f64,
}

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
            .map_err(|e| format!("Couldn't reach Anthropic: {e}"))?;
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
            .map_err(|e| format!("Couldn't reach Anthropic: {e}"))?;
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
    let usernames: Vec<&str> = PERSONAS.iter().map(|p| p.username).collect();
    json!({
        "model": MODEL,
        "max_tokens": MAX_TOKENS,
        // Chat messages are short and simple; low effort keeps them fast
        // and cheap (the model can skip thinking entirely).
        "output_config": {
            "effort": "low",
            "format": {
                "type": "json_schema",
                "schema": {
                    "type": "object",
                    "properties": {
                        "messages": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "chatter": { "type": "string", "enum": usernames },
                                    "text": { "type": "string" }
                                },
                                "required": ["chatter", "text"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["messages"],
                    "additionalProperties": false
                }
            }
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

fn system_prompt() -> String {
    let chatters: String = PERSONAS
        .iter()
        .map(|p| format!("- {}: {}\n", p.username, p.personality))
        .collect();
    format!(
        "You write the messages of a practice live-stream chat. It keeps a small streamer company while they practise talking on stream with no viewers yet. The streamer knows every chatter is an AI, and each message is shown with an \"AI\" badge.

The chatters:
{chatters}
Each time, write the next 1 to {MAX_MESSAGES} chat messages. Usually write 1; write 2 or 3 only when several chatters would naturally react. Never use the same chatter twice in a row, and vary who speaks.

How the messages read:
- Like real Twitch chat: short (usually under 15 words), casual, lowercase is fine, no hashtags, at most one emoji.
- React to what the streamer actually said, specifically. Give them something to respond to, such as a question, a reaction or an opinion.
- Write in the language the streamer is speaking.
- Don't repeat or rephrase recent chat messages.
- The transcript comes from speech recognition and may have mistakes; don't point them out.

Content level, normal teasing: playful ribbing about gameplay, choices and luck is welcome. Keep it friendly; the goal is to help the streamer feel comfortable talking.

Always:
- Talk only to the streamer or to the other chatters listed above. There are no other viewers: never mention, greet or reply to anyone else, and never mention viewer counts, followers, subscriptions, donations or raids.
- No insults about the streamer's appearance, voice, body or identity, no slurs, no sexual content, no harassment, nothing about self-harm, no political or religious arguments.
- Don't ask for or mention personal information such as where the streamer lives.
- Don't claim to be human, and don't give medical, legal or financial advice."
    )
}

fn user_message(prompt: &Prompt) -> String {
    let stream_context = match prompt.stream_context.trim() {
        "" => "(not given)",
        context => context,
    };
    let heard = if prompt.heard.is_empty() {
        "(nothing yet)".to_string()
    } else {
        prompt
            .heard
            .iter()
            .map(|line| format!("[{}s ago] {}", line.ago.as_secs(), line.text))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let chat = if prompt.recent_chat.is_empty() {
        "(no messages yet)".to_string()
    } else {
        prompt
            .recent_chat
            .iter()
            .map(|message| format!("{}: {}", message.chatter, message.text))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let reason = match prompt.reason {
        Reason::SessionStart => {
            "The stream just started. Say hi like chatters who just arrived, and react to what the stream is about."
        }
        Reason::Pause => "The streamer just paused after talking. React to what they said.",
        Reason::Quiet => {
            "The streamer hasn't said anything for a while. Give them something to talk about, such as a question about what they're doing."
        }
        Reason::Nudge => "The streamer asked chat to say something right now.",
    };

    format!(
        "What the stream is about, in the streamer's words:\n{stream_context}\n\nWhat the streamer said recently (speech recognition):\n{heard}\n\nRecent chat:\n{chat}\n\n{reason}"
    )
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

#[derive(Deserialize)]
struct Written {
    messages: Vec<WrittenMessage>,
}

#[derive(Deserialize)]
struct WrittenMessage {
    chatter: String,
    text: String,
}

/// Turns the API's reply into messages. A refusal, a cut-off reply or
/// anything unexpected just means nothing gets posted this time.
fn parse_reply(response: ApiResponse) -> Reply {
    let cost_usd = response.usage.cost_usd();
    let complete = response.stop_reason.as_deref() == Some("end_turn");
    let written = response
        .content
        .iter()
        .find(|block| block.kind == "text")
        .and_then(|block| block.text.as_deref())
        .filter(|_| complete)
        .and_then(|text| serde_json::from_str::<Written>(text).ok());

    let messages = written
        .map(|written| written.messages)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|message| {
            let persona = PERSONAS
                .iter()
                .position(|p| p.username == message.chatter)?;
            let text = clean_text(&message.text, PERSONAS[persona].username)?;
            Some((persona, text))
        })
        .take(MAX_MESSAGES)
        .collect();
    Reply { messages, cost_usd }
}

fn clean_text(text: &str, username: &str) -> Option<String> {
    let text = text.trim();
    // The model sometimes writes "username: message" itself.
    let text = text
        .strip_prefix(username)
        .and_then(|rest| rest.strip_prefix(':'))
        .map_or(text, str::trim);
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(MAX_MESSAGE_CHARS).collect())
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
    fn reads_messages_from_the_reply() {
        let reply = parse_reply(response(
            "end_turn",
            r#"{"messages":[{"chatter":"curious_ava","text":" what game is this? "},{"chatter":"pixelpop_kai","text":"pixelpop_kai: LETS GO"}]}"#,
        ));
        assert_eq!(
            reply.messages,
            vec![(1, "what game is this?".into()), (0, "LETS GO".into())]
        );
    }

    #[test]
    fn unknown_chatters_and_empty_messages_are_dropped() {
        let reply = parse_reply(response(
            "end_turn",
            r#"{"messages":[{"chatter":"someone_else","text":"hi"},{"chatter":"backseat_benny","text":"  "}]}"#,
        ));
        assert!(reply.messages.is_empty());
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
    fn the_prompt_includes_what_the_streamer_said_and_the_stream_context() {
        let heard = [Heard {
            ago: Duration::from_secs(12),
            text: "this boss is impossible".into(),
        }];
        let body = request_body(&Prompt {
            stream_context: "Elden Ring, first playthrough",
            heard: &heard,
            recent_chat: &[],
            reason: Reason::Pause,
        });
        let user = body["messages"][0]["content"].as_str().unwrap();
        assert!(user.contains("Elden Ring, first playthrough"));
        assert!(user.contains("[12s ago] this boss is impossible"));
        let system = body["system"][0]["text"].as_str().unwrap();
        for persona in &PERSONAS {
            assert!(system.contains(persona.username));
        }
        // Sampling parameters are rejected by this API version.
        assert!(body.get("temperature").is_none());
    }
}
