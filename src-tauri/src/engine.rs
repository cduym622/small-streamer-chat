//! The Practice Chat engine. It runs in the Rust core (not the webview) so it
//! keeps going while the window is hidden or minimized (see ADR 0002).

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::{
    sync::{broadcast, mpsc},
    time::{Instant, sleep_until},
};

use crate::{chat::ChatMessage, personas::PERSONAS};

/// Shortest and longest wait between unprompted Practice Chat messages.
const MIN_GAP: Duration = Duration::from_secs(20);
const MAX_GAP: Duration = Duration::from_secs(45);
/// Delay before the first message after starting, so it's obvious it works.
const FIRST_MESSAGE_DELAY: Duration = Duration::from_secs(3);

pub const STATE_EVENT: &str = "engine-state";
pub const MESSAGE_EVENT: &str = "chat-message";

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineState {
    /// A Session is in progress.
    pub running: bool,
    /// The streamer has quieted the Practice Chat (Back-off).
    pub backed_off: bool,
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
}

/// Cheap-to-clone handle for talking to the engine from commands, hotkeys
/// and the Overlay server.
#[derive(Clone)]
pub struct EngineHandle {
    commands: mpsc::UnboundedSender<Command>,
    state: Arc<Mutex<EngineState>>,
    messages: broadcast::Sender<ChatMessage>,
}

impl EngineHandle {
    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn state(&self) -> EngineState {
        *self.state.lock().unwrap()
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
        state: Arc::new(Mutex::new(EngineState::default())),
        messages,
    };
    let engine = Engine {
        app,
        handle: handle.clone(),
        next_id: 0,
        last_persona: None,
    };
    tauri::async_runtime::spawn(engine.run(command_rx));
    handle
}

struct Engine<R: Runtime> {
    app: AppHandle<R>,
    handle: EngineHandle,
    next_id: u64,
    last_persona: Option<usize>,
}

impl<R: Runtime> Engine<R> {
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        // When the next unprompted message is due; `None` while not posting.
        let mut next_message: Option<Instant> = None;

        loop {
            let due = async move {
                match next_message {
                    Some(at) => sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };

            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    let before = self.handle.state();
                    let after = self.apply(command, before);
                    if after.posting() && !before.posting() {
                        let delay = if before.running { random_gap() } else { FIRST_MESSAGE_DELAY };
                        next_message = Some(Instant::now() + delay);
                    } else if !after.posting() {
                        next_message = None;
                    }
                }
                () = due => {
                    self.post();
                    next_message = Some(Instant::now() + random_gap());
                }
            }
        }
    }

    /// Applies a command and returns the new state.
    fn apply(&mut self, command: Command, mut state: EngineState) -> EngineState {
        match command {
            Command::SetRunning(running) => state.running = running,
            Command::ToggleRunning => state.running = !state.running,
            Command::SetBackOff(backed_off) => state.backed_off = backed_off,
            Command::ToggleBackOff => state.backed_off = !state.backed_off,
            // A Nudge is an explicit request, so it posts even during Back-off.
            Command::Nudge => {
                if state.running {
                    self.post();
                }
                return state;
            }
        }
        // A new Session always starts un-backed-off.
        if !state.running {
            state.backed_off = false;
        }
        *self.handle.state.lock().unwrap() = state;
        let _ = self.app.emit(STATE_EVENT, state);
        state
    }

    fn post(&mut self) {
        let persona_index = self.pick_persona();
        let persona = &PERSONAS[persona_index];
        let text = persona.canned_lines[fastrand::usize(..persona.canned_lines.len())];

        let message = ChatMessage {
            id: self.next_id,
            chatter: persona.username.to_string(),
            color: persona.color.to_string(),
            text: text.to_string(),
        };
        self.next_id += 1;

        let _ = self.app.emit(MESSAGE_EVENT, &message);
        // No Overlay connected is fine.
        let _ = self.handle.messages.send(message);
    }

    /// Picks a Persona, avoiding the same one twice in a row.
    fn pick_persona(&mut self) -> usize {
        let mut index = fastrand::usize(..PERSONAS.len());
        if Some(index) == self.last_persona {
            index = (index + 1) % PERSONAS.len();
        }
        self.last_persona = Some(index);
        index
    }
}

fn random_gap() -> Duration {
    let range = MAX_GAP - MIN_GAP;
    MIN_GAP + range.mul_f64(fastrand::f64())
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
        tauri::async_runtime::block_on(async {
            tokio::time::sleep(Duration::from_millis(100)).await
        });
        assert!(!engine.state().backed_off, "stopping should clear Back-off");
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
