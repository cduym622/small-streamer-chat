//! The Practice Chat engine. It runs in the Rust core (not the webview) so it
//! keeps going while the window is hidden or minimized (see ADR 0002).
//!
//! When to speak: after each round of messages the engine waits a random
//! 20-45 s, then for the streamer to pause (if they're mid-sentence), then
//! asks the AI for the next messages. A Nudge skips the wait.

use std::{collections::VecDeque, time::Duration};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::{
    sync::{broadcast, mpsc, watch},
    time::{Instant, sleep, sleep_until},
};

use crate::{
    ai::{Anthropic, Heard, Prompt, Reason, Reply},
    chat::ChatMessage,
    personas::PERSONAS,
    problems,
};

/// Shortest and longest wait between unprompted Practice Chat messages.
const MIN_GAP: Duration = Duration::from_secs(20);
const MAX_GAP: Duration = Duration::from_secs(45);
/// Delay before the first message after starting, so it's obvious it works.
const FIRST_MESSAGE_DELAY: Duration = Duration::from_secs(3);
/// Pause between messages written in the same round, so they don't land at
/// once.
const MIN_FOLLOW_UP: Duration = Duration::from_millis(1500);
const MAX_FOLLOW_UP: Duration = Duration::from_millis(4000);
/// What the AI sees of the streamer's recent speech and of the chat. Kept
/// in memory only, for the current Session.
const HEARD_WINDOW: Duration = Duration::from_secs(180);
const MAX_HEARD_LINES: usize = 20;
const MAX_RECENT_CHAT: usize = 12;

pub const STATE_EVENT: &str = "engine-state";
pub const MESSAGE_EVENT: &str = "chat-message";

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineState {
    /// A Session is in progress.
    pub running: bool,
    /// The streamer has quieted the Practice Chat (Back-off).
    pub backed_off: bool,
    /// AI cost of the current (or last) Session, in US dollars.
    pub session_cost: f64,
}

impl EngineState {
    fn posting(self) -> bool {
        self.running && !self.backed_off
    }
}

pub enum Command {
    SetRunning(bool),
    ToggleRunning,
    SetBackOff(bool),
    ToggleBackOff,
    Nudge,
    /// The AI Connection to write messages with; `None` uses canned lines.
    SetAi(Option<Anthropic>),
    SetStreamContext(String),
    /// The streamer started talking.
    SpeechStarted,
    /// The streamer stopped talking; the text if it was words.
    Heard(Option<String>),
    /// Internal: the AI finished writing a round of messages.
    Written {
        session: u64,
        nudged: bool,
        result: Result<Reply, String>,
    },
    /// Internal: a follow-up message from a round is due.
    Post {
        session: u64,
        nudged: bool,
        persona: usize,
        text: String,
    },
}

/// Cheap-to-clone handle for talking to the engine from commands, hotkeys,
/// hearing and the Overlay server.
#[derive(Clone)]
pub struct EngineHandle {
    commands: mpsc::UnboundedSender<Command>,
    state: watch::Sender<EngineState>,
    messages: broadcast::Sender<ChatMessage>,
}

impl EngineHandle {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn state(&self) -> EngineState {
        *self.state.borrow()
    }

    /// Follows state changes (Start/Stop, Back-off).
    pub fn watch_state(&self) -> watch::Receiver<EngineState> {
        self.state.subscribe()
    }

    /// New Practice Chat messages, as they're posted.
    pub fn subscribe(&self) -> broadcast::Receiver<ChatMessage> {
        self.messages.subscribe()
    }
}

pub fn spawn<R: Runtime>(app: AppHandle<R>) -> EngineHandle {
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (messages, _) = broadcast::channel(64);
    let handle = EngineHandle {
        commands,
        state: watch::Sender::new(EngineState::default()),
        messages,
    };
    let engine = Engine {
        app,
        handle: handle.clone(),
        next_id: 0,
        last_persona: None,
        ai: None,
        stream_context: String::new(),
        session: 0,
        next_round: None,
        waiting_for_pause: false,
        writing: false,
        speaking: false,
        heard: VecDeque::new(),
        recent_chat: VecDeque::new(),
        last_round: None,
        rounds: 0,
    };
    tauri::async_runtime::spawn(engine.run(command_rx));
    handle
}

struct Engine<R: Runtime> {
    app: AppHandle<R>,
    handle: EngineHandle,
    next_id: u64,
    last_persona: Option<usize>,
    ai: Option<Anthropic>,
    stream_context: String,
    /// Counts Sessions, so replies from an earlier Session are ignored.
    session: u64,
    /// When the next unprompted round is due; `None` while not posting or
    /// while a round is underway.
    next_round: Option<Instant>,
    /// A round is due, but the streamer is mid-sentence.
    waiting_for_pause: bool,
    /// The AI is writing a round.
    writing: bool,
    speaking: bool,
    heard: VecDeque<(Instant, String)>,
    recent_chat: VecDeque<ChatMessage>,
    last_round: Option<Instant>,
    /// Rounds so far this Session.
    rounds: u64,
}

impl<R: Runtime> Engine<R> {
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        loop {
            let next_round = self.next_round;
            let due = async move {
                match next_round {
                    Some(at) => sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };

            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    self.handle(command);
                }
                () = due => {
                    self.next_round = None;
                    if self.speaking {
                        self.waiting_for_pause = true;
                    } else {
                        self.start_round(false);
                    }
                }
            }
        }
    }

    fn state(&self) -> EngineState {
        self.handle.state()
    }

    fn set_state(&self, change: impl FnOnce(&mut EngineState)) {
        let mut state = self.state();
        change(&mut state);
        self.handle.state.send_replace(state);
        let _ = self.app.emit(STATE_EVENT, state);
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::SetRunning(running) => self.set_running(running),
            Command::ToggleRunning => self.set_running(!self.state().running),
            Command::SetBackOff(backed_off) => self.set_back_off(backed_off),
            Command::ToggleBackOff => self.set_back_off(!self.state().backed_off),
            // A Nudge is an explicit request, so it works even during
            // Back-off.
            Command::Nudge => {
                if self.state().running {
                    self.start_round(true);
                }
            }
            Command::SetAi(ai) => self.ai = ai,
            Command::SetStreamContext(context) => self.stream_context = context,
            Command::SpeechStarted => self.speaking = true,
            Command::Heard(text) => {
                self.speaking = false;
                if let Some(text) = text
                    && self.state().running
                {
                    self.remember_heard(text);
                }
                if self.waiting_for_pause {
                    self.waiting_for_pause = false;
                    self.start_round(false);
                }
            }
            Command::Written {
                session,
                nudged,
                result,
            } => self.written(session, nudged, result),
            Command::Post {
                session,
                nudged,
                persona,
                text,
            } => {
                if session == self.session && self.may_post(nudged) {
                    self.post(persona, text);
                }
            }
        }
    }

    fn set_running(&mut self, running: bool) {
        let before = self.state();
        if running == before.running {
            return;
        }
        if running {
            self.session += 1;
            // A reply still on its way belongs to the previous Session.
            self.writing = false;
            self.rounds = 0;
            self.heard.clear();
            self.recent_chat.clear();
            self.last_round = None;
            self.next_round = Some(Instant::now() + FIRST_MESSAGE_DELAY);
        } else {
            self.next_round = None;
            self.waiting_for_pause = false;
            self.speaking = false;
        }
        self.set_state(|state| {
            state.running = running;
            // A new Session always starts un-backed-off, with its own cost.
            state.backed_off = false;
            if running {
                state.session_cost = 0.0;
            }
        });
    }

    fn set_back_off(&mut self, backed_off: bool) {
        let before = self.state();
        if !before.running || backed_off == before.backed_off {
            return;
        }
        if backed_off {
            self.next_round = None;
            self.waiting_for_pause = false;
        } else if !self.writing {
            self.next_round = Some(Instant::now() + random_gap());
        }
        self.set_state(|state| state.backed_off = backed_off);
    }

    fn may_post(&self, nudged: bool) -> bool {
        let state = self.state();
        state.running && (nudged || !state.backed_off)
    }

    fn remember_heard(&mut self, text: String) {
        let now = Instant::now();
        self.heard.push_back((now, text));
        while self.heard.len() > MAX_HEARD_LINES
            || self
                .heard
                .front()
                .is_some_and(|(at, _)| now - *at > HEARD_WINDOW)
        {
            self.heard.pop_front();
        }
    }

    /// Starts a round of messages: canned at once, or written by the AI.
    fn start_round(&mut self, nudged: bool) {
        if self.writing {
            return;
        }
        self.next_round = None;
        let reason = self.reason(nudged);
        self.rounds += 1;
        self.last_round = Some(Instant::now());

        let Some(ai) = self.ai.clone() else {
            let persona = self.pick_persona();
            let lines = PERSONAS[persona].canned_lines;
            let text = lines[fastrand::usize(..lines.len())].to_string();
            self.post(persona, text);
            self.schedule_next_round();
            return;
        };

        self.writing = true;
        let now = Instant::now();
        let heard: Vec<Heard> = self
            .heard
            .iter()
            .map(|(at, text)| Heard {
                ago: now - *at,
                text: text.clone(),
            })
            .collect();
        let recent_chat: Vec<ChatMessage> = self.recent_chat.iter().cloned().collect();
        let stream_context = self.stream_context.clone();
        let session = self.session;
        let commands = self.handle.commands.clone();
        tauri::async_runtime::spawn(async move {
            let result = ai
                .write(&Prompt {
                    stream_context: &stream_context,
                    heard: &heard,
                    recent_chat: &recent_chat,
                    reason,
                })
                .await;
            let _ = commands.send(Command::Written {
                session,
                nudged,
                result,
            });
        });
    }

    fn reason(&self, nudged: bool) -> Reason {
        if nudged {
            Reason::Nudge
        } else if self.rounds == 0 {
            Reason::SessionStart
        } else if self
            .heard
            .back()
            .is_some_and(|(at, _)| self.last_round.is_none_or(|round| *at > round))
        {
            Reason::Pause
        } else {
            Reason::Quiet
        }
    }

    fn written(&mut self, session: u64, nudged: bool, result: Result<Reply, String>) {
        if session != self.session {
            return;
        }
        self.writing = false;
        match result {
            Ok(reply) => {
                self.set_state(|state| state.session_cost += reply.cost_usd);
                if self.may_post(nudged) {
                    self.post_round(nudged, reply.messages);
                }
            }
            Err(error) => problems::report(&self.app, error),
        }
        self.schedule_next_round();
    }

    /// Posts the first message now and the rest a few seconds apart.
    fn post_round(&mut self, nudged: bool, messages: Vec<(usize, String)>) {
        let mut messages = messages.into_iter();
        let Some((persona, text)) = messages.next() else {
            return;
        };
        self.post(persona, text);

        let follow_ups: Vec<_> = messages.collect();
        if follow_ups.is_empty() {
            return;
        }
        let session = self.session;
        let commands = self.handle.commands.clone();
        tauri::async_runtime::spawn(async move {
            for (persona, text) in follow_ups {
                sleep(random_between(MIN_FOLLOW_UP, MAX_FOLLOW_UP)).await;
                let _ = commands.send(Command::Post {
                    session,
                    nudged,
                    persona,
                    text,
                });
            }
        });
    }

    fn schedule_next_round(&mut self) {
        if self.state().posting() {
            self.next_round = Some(Instant::now() + random_gap());
        }
    }

    fn post(&mut self, persona: usize, text: String) {
        let persona_info = &PERSONAS[persona];
        self.last_persona = Some(persona);
        let message = ChatMessage {
            id: self.next_id,
            chatter: persona_info.username.to_string(),
            color: persona_info.color.to_string(),
            text,
        };
        self.next_id += 1;

        self.recent_chat.push_back(message.clone());
        if self.recent_chat.len() > MAX_RECENT_CHAT {
            self.recent_chat.pop_front();
        }
        let _ = self.app.emit(MESSAGE_EVENT, &message);
        // No Overlay connected is fine.
        let _ = self.handle.messages.send(message);
    }

    /// Picks a Persona for a canned line, avoiding the same one twice in a
    /// row.
    fn pick_persona(&mut self) -> usize {
        let mut index = fastrand::usize(..PERSONAS.len());
        if Some(index) == self.last_persona {
            index = (index + 1) % PERSONAS.len();
        }
        index
    }
}

fn random_between(min: Duration, max: Duration) -> Duration {
    min + (max - min).mul_f64(fastrand::f64())
}

fn random_gap() -> Duration {
    random_between(MIN_GAP, MAX_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    fn start_engine() -> (tauri::App<tauri::test::MockRuntime>, EngineHandle) {
        let app = tauri::test::mock_app();
        let engine = spawn(app.handle().clone());
        (app, engine)
    }

    async fn next_message(
        rx: &mut broadcast::Receiver<ChatMessage>,
        within: Duration,
    ) -> Option<ChatMessage> {
        timeout(within, rx.recv()).await.ok().and_then(Result::ok)
    }

    fn wait(duration: Duration) {
        tauri::async_runtime::block_on(async { tokio::time::sleep(duration).await });
    }

    #[test]
    fn starting_posts_a_first_message_quickly() {
        let (_app, engine) = start_engine();
        let mut rx = engine.subscribe();
        engine.send(Command::SetRunning(true));
        let message = tauri::async_runtime::block_on(next_message(
            &mut rx,
            FIRST_MESSAGE_DELAY + Duration::from_secs(2),
        ));
        assert!(message.is_some(), "no message after starting");
        assert!(engine.state().running);
    }

    #[test]
    fn nudge_posts_immediately_but_only_while_running() {
        let (_app, engine) = start_engine();
        let mut rx = engine.subscribe();

        engine.send(Command::Nudge);
        let stopped =
            tauri::async_runtime::block_on(next_message(&mut rx, Duration::from_millis(300)));
        assert!(stopped.is_none(), "nudge posted while stopped");

        engine.send(Command::SetRunning(true));
        engine.send(Command::Nudge);
        let nudged =
            tauri::async_runtime::block_on(next_message(&mut rx, Duration::from_millis(500)));
        assert!(nudged.is_some(), "nudge didn't post while running");
    }

    #[test]
    fn back_off_stops_unprompted_messages_and_stop_clears_it() {
        let (_app, engine) = start_engine();
        let mut rx = engine.subscribe();
        engine.send(Command::SetRunning(true));
        engine.send(Command::SetBackOff(true));
        let during_back_off = tauri::async_runtime::block_on(next_message(
            &mut rx,
            FIRST_MESSAGE_DELAY + Duration::from_secs(1),
        ));
        assert!(during_back_off.is_none(), "posted during Back-off");
        assert!(engine.state().backed_off);

        engine.send(Command::SetRunning(false));
        wait(Duration::from_millis(100));
        assert!(!engine.state().backed_off, "stopping should clear Back-off");
    }

    #[test]
    fn waits_for_the_streamer_to_pause() {
        let (_app, engine) = start_engine();
        let mut rx = engine.subscribe();
        engine.send(Command::SetRunning(true));
        engine.send(Command::SpeechStarted);
        let mid_sentence = tauri::async_runtime::block_on(next_message(
            &mut rx,
            FIRST_MESSAGE_DELAY + Duration::from_secs(1),
        ));
        assert!(mid_sentence.is_none(), "talked over the streamer");

        engine.send(Command::Heard(Some("okay let's go".into())));
        let after_pause =
            tauri::async_runtime::block_on(next_message(&mut rx, Duration::from_millis(500)));
        assert!(after_pause.is_some(), "didn't speak at the pause");
    }

    #[test]
    fn never_picks_the_same_persona_twice_in_a_row() {
        let (_app, engine) = start_engine();
        let mut rx = engine.subscribe();
        engine.send(Command::SetRunning(true));
        let mut last = None;
        for _ in 0..30 {
            engine.send(Command::Nudge);
            let message =
                tauri::async_runtime::block_on(next_message(&mut rx, Duration::from_millis(500)))
                    .unwrap();
            assert_ne!(Some(message.chatter.clone()), last);
            last = Some(message.chatter);
        }
    }
}
