//! Writes Practice Chat messages with an AI Connection: Claude Haiku with
//! the streamer's Anthropic API key, or a model on their ChatGPT plan. Both
//! get the same prompt and answer with the same JSON: 1-3 short messages,
//! each from one of the Personas.

mod anthropic;
pub mod chatgpt;

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{chat::ChatMessage, personas::PERSONAS};

pub use anthropic::Anthropic;
pub use chatgpt::ChatGpt;

/// Chat messages longer than this are cut, like a real chat's length limit.
const MAX_MESSAGE_CHARS: usize = 200;
const MAX_MESSAGES: usize = 3;

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
    /// What the request cost; zero on a ChatGPT plan.
    pub cost_usd: f64,
}

/// Which kind of AI Connection is in use, for the UI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    /// Canned lines.
    #[default]
    None,
    Anthropic,
    ChatGpt,
}

#[derive(Clone)]
pub enum Ai {
    Anthropic(Anthropic),
    ChatGpt(ChatGpt),
}

impl Ai {
    pub fn provider(&self) -> Provider {
        match self {
            Ai::Anthropic(_) => Provider::Anthropic,
            Ai::ChatGpt(_) => Provider::ChatGpt,
        }
    }

    pub async fn write(&self, prompt: &Prompt<'_>) -> Result<Reply, String> {
        match self {
            Ai::Anthropic(ai) => ai.write(prompt).await,
            Ai::ChatGpt(ai) => ai.write(prompt).await,
        }
    }

    /// A new Session gets a fresh try after a plan's usage limit was hit.
    pub fn session_started(&self) {
        if let Ai::ChatGpt(ai) = self {
            ai.resume();
        }
    }
}

/// A message for a request that didn't get an answer. The engine keeps
/// trying at the next round, so a short outage fixes itself.
pub fn network_error(service: &str, error: &reqwest::Error) -> String {
    if error.is_timeout() {
        format!("{service} took too long to answer, so the chat will be quieter for a bit.")
    } else if error.is_connect() {
        format!(
            "Couldn't reach {service}. Check your internet connection; the chat will keep trying."
        )
    } else {
        format!("Couldn't reach {service}: {error}")
    }
}

/// The JSON Schema of a reply: `{"messages": [{"chatter", "text"}]}`.
fn messages_schema() -> Value {
    let usernames: Vec<&str> = PERSONAS.iter().map(|p| p.username).collect();
    json!({
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
struct Written {
    messages: Vec<WrittenMessage>,
}

#[derive(Deserialize)]
struct WrittenMessage {
    chatter: String,
    text: String,
}

/// Reads the messages from the model's JSON. Anything unexpected just means
/// nothing gets posted this time.
fn parse_messages(text: &str) -> Vec<(usize, String)> {
    // Without enforced structured output a model may wrap the JSON in prose
    // or a code fence, so read from the first `{` to the last `}`.
    let json = match (text.find('{'), text.rfind('}')) {
        (Some(start), Some(end)) if start < end => &text[start..=end],
        _ => return Vec::new(),
    };
    let Ok(written) = serde_json::from_str::<Written>(json) else {
        return Vec::new();
    };
    written
        .messages
        .into_iter()
        .filter_map(|message| {
            let persona = PERSONAS
                .iter()
                .position(|p| p.username == message.chatter)?;
            let text = clean_text(&message.text, PERSONAS[persona].username)?;
            Some((persona, text))
        })
        .take(MAX_MESSAGES)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_messages_from_the_reply() {
        let messages = parse_messages(
            r#"{"messages":[{"chatter":"curious_ava","text":" what game is this? "},{"chatter":"pixelpop_kai","text":"pixelpop_kai: LETS GO"}]}"#,
        );
        assert_eq!(
            messages,
            vec![(1, "what game is this?".into()), (0, "LETS GO".into())]
        );
    }

    #[test]
    fn reads_json_wrapped_in_a_code_fence() {
        let messages = parse_messages(
            "```json\n{\"messages\":[{\"chatter\":\"curious_ava\",\"text\":\"hi\"}]}\n```",
        );
        assert_eq!(messages, vec![(1, "hi".into())]);
    }

    #[test]
    fn unknown_chatters_and_empty_messages_are_dropped() {
        let messages = parse_messages(
            r#"{"messages":[{"chatter":"someone_else","text":"hi"},{"chatter":"backseat_benny","text":"  "}]}"#,
        );
        assert!(messages.is_empty());
        assert!(parse_messages("I can't help with that.").is_empty());
    }

    #[test]
    fn the_prompt_includes_what_the_streamer_said_and_the_stream_context() {
        let heard = [Heard {
            ago: Duration::from_secs(12),
            text: "this boss is impossible".into(),
        }];
        let user = user_message(&Prompt {
            stream_context: "Elden Ring, first playthrough",
            heard: &heard,
            recent_chat: &[],
            reason: Reason::Pause,
        });
        assert!(user.contains("Elden Ring, first playthrough"));
        assert!(user.contains("[12s ago] this boss is impossible"));
        let system = system_prompt();
        for persona in &PERSONAS {
            assert!(system.contains(persona.username));
        }
    }
}
