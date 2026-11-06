use serde::Serialize;

/// One message in the Practice Chat, written by an AI Chatter.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: u64,
    /// The AI Chatter's username.
    pub chatter: String,
    /// The AI Chatter's username colour, as a CSS colour.
    pub color: String,
    pub text: String,
}
