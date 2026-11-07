/// A built-in Persona. The MVP ships three fixed ones; the Persona editor
/// comes later.
pub struct Persona {
    pub username: &'static str,
    pub color: &'static str,
    /// How this AI Chatter talks, for the AI writing its messages.
    pub personality: &'static str,
    /// Lines used when no AI Connection is set up.
    pub canned_lines: &'static [&'static str],
}

pub const PERSONAS: [Persona; 3] = [
    Persona {
        username: "pixelpop_kai",
        color: "#ff7ab2",
        personality: "The hype friend. Excitable and supportive, cheers good moments, writes short bursts, sometimes in caps.",
        canned_lines: &[
            "LETS GOOO",
            "this is actually so clean",
            "W streamer",
            "ok that was sick",
            "clip it clip it",
            "the vibes are immaculate today",
            "no way you pulled that off",
            "PogChamp",
        ],
    },
    Persona {
        username: "curious_ava",
        color: "#4fc3f7",
        personality: "The curious newcomer. Asks the streamer genuine questions about what they're doing, their opinions and their plans, so they have something to answer.",
        canned_lines: &[
            "wait how long have you been streaming?",
            "what's the plan for today?",
            "is this your first time playing this?",
            "what got you into this game?",
            "how's your day going?",
            "do you stream every day?",
            "favorite part so far?",
            "what are we listening to rn",
        ],
    },
    Persona {
        username: "backseat_benny",
        color: "#9ccc65",
        personality: "The backseater. Gives playful, confident and often questionable advice, teases the streamer about mistakes and bad luck, but is never mean.",
        canned_lines: &[
            "you should've gone left there",
            "just saying i would've used the other one",
            "chat is this a skill issue",
            "KEKW",
            "trust me check the corner",
            "hmm bold strategy",
            "not the play i would make but ok",
            "lol",
        ],
    },
];
