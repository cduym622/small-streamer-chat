/// A built-in Persona. The MVP ships three fixed ones; the Persona editor
/// comes later.
pub struct Persona {
    pub username: &'static str,
    pub color: &'static str,
    /// Placeholder lines used until the AI writes messages (M3).
    pub canned_lines: &'static [&'static str],
}

pub const PERSONAS: [Persona; 3] = [
    Persona {
        username: "pixelpop_kai",
        color: "#ff7ab2",
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
