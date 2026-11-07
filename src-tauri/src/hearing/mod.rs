//! Hearing the streamer: the microphone is open while a Session is running,
//! speech is split into utterances at pauses, and each utterance is turned
//! into text with Whisper. Nothing is recorded or saved to disk.
//!
//! The mic, the pause detection and Whisper all run on one dedicated thread
//! (Whisper blocks while it works). Everything else talks to it through
//! messages via `HearingHandle`.

mod capture;
mod signal;
mod transcriber;

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use capture::Capture;
pub use capture::{Microphone, microphones};
use signal::{PauseDetector, Resampler, loudness};
use transcriber::Transcriber;

use crate::{
    engine::{Command, EngineHandle},
    problems, speech_model,
};

pub const STATUS_EVENT: &str = "hearing-status";
pub const TRANSCRIPT_EVENT: &str = "transcript";
pub const LEVEL_EVENT: &str = "mic-level";

/// How often the mic level meter updates.
const LEVEL_INTERVAL: Duration = Duration::from_millis(100);
/// A mic that has sent only exact silence for this long is probably blocked
/// (macOS sends silence when microphone access is denied).
const SILENT_MIC_WARNING: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ModelStatus {
    Checking,
    Downloading { percent: u8 },
    Loading,
    Ready,
    Failed { error: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HearingStatus {
    pub model: ModelStatus,
    /// The mic is open (a Session is running).
    pub listening: bool,
    /// The streamer's chosen mic; `None` follows the system default.
    pub microphone: Option<String>,
    /// Name of the mic actually in use while listening.
    pub microphone_name: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    pub id: u64,
    pub text: String,
}

enum Message {
    Listen(bool),
    SetMicrophone(Option<String>),
    Model(ModelStatus),
    ModelDownloaded(PathBuf),
    Audio { stream: u64, samples: Vec<f32> },
    StreamFailed { stream: u64, error: String },
}

/// Cheap-to-clone handle for talking to the hearing thread.
#[derive(Clone)]
pub struct HearingHandle {
    messages: mpsc::Sender<Message>,
    status: Arc<Mutex<HearingStatus>>,
    model_requested: Arc<AtomicBool>,
}

impl HearingHandle {
    pub fn status(&self) -> HearingStatus {
        self.status.lock().unwrap().clone()
    }

    /// Opens or closes the mic.
    pub fn listen(&self, listen: bool) {
        let _ = self.messages.send(Message::Listen(listen));
    }

    pub fn set_microphone(&self, id: Option<String>) {
        let _ = self.messages.send(Message::SetMicrophone(id));
    }

    /// Downloads the speech model if needed, then loads it. Does nothing if
    /// it's already loaded or on its way.
    pub fn prepare_model(&self, app: AppHandle) {
        let retrying = matches!(self.status().model, ModelStatus::Failed { .. });
        if self.model_requested.swap(true, Ordering::SeqCst) && !retrying {
            return;
        }
        let messages = self.messages.clone();
        let _ = messages.send(Message::Model(ModelStatus::Checking));
        tauri::async_runtime::spawn(async move {
            let mut last_percent = None;
            let result = speech_model::ensure(&app, |downloaded, total| {
                let percent = (downloaded * 100 / total.max(1)) as u8;
                if last_percent != Some(percent) {
                    last_percent = Some(percent);
                    let _ = messages.send(Message::Model(ModelStatus::Downloading { percent }));
                }
            })
            .await;
            let _ = messages.send(match result {
                Ok(path) => Message::ModelDownloaded(path),
                Err(error) => Message::Model(ModelStatus::Failed { error }),
            });
        });
    }
}

/// What's heard is passed to the `engine`, which decides when to reply.
pub fn spawn(app: AppHandle, microphone: Option<String>, engine: EngineHandle) -> HearingHandle {
    let (messages, inbox) = mpsc::channel();
    let status = Arc::new(Mutex::new(HearingStatus {
        model: ModelStatus::Checking,
        listening: false,
        microphone,
        microphone_name: None,
    }));
    let handle = HearingHandle {
        messages: messages.clone(),
        status: status.clone(),
        model_requested: Arc::default(),
    };
    let worker = Worker {
        app,
        engine,
        messages,
        status,
        transcriber: None,
        mic: None,
        next_stream: 0,
        next_line: 0,
        transcription_failed: false,
    };
    thread::Builder::new()
        .name("hearing".into())
        .spawn(move || worker.run(inbox))
        .expect("couldn't start the hearing thread");
    handle
}

/// An open mic and the processing of its audio.
struct Mic {
    _capture: Capture,
    stream: u64,
    resampler: Resampler,
    detector: PauseDetector,
    opened: Instant,
    heard_sound: bool,
    warned_silent: bool,
    level: f32,
    level_sent: Instant,
}

struct Worker {
    app: AppHandle,
    engine: EngineHandle,
    /// For the mic's callbacks to post audio back to this thread.
    messages: mpsc::Sender<Message>,
    status: Arc<Mutex<HearingStatus>>,
    transcriber: Option<Transcriber>,
    mic: Option<Mic>,
    next_stream: u64,
    next_line: u64,
    transcription_failed: bool,
}

impl Worker {
    fn run(mut self, inbox: mpsc::Receiver<Message>) {
        for message in inbox {
            match message {
                Message::Listen(true) if self.mic.is_none() => self.open_mic(),
                Message::Listen(true) => {}
                Message::Listen(false) => self.close_mic(),
                Message::SetMicrophone(id) => {
                    self.update(|status| status.microphone = id);
                    if self.mic.is_some() {
                        self.open_mic();
                    }
                }
                Message::Model(model) => self.update(|status| status.model = model),
                Message::ModelDownloaded(path) => self.load_model(&path),
                Message::Audio { stream, samples } => self.hear(stream, &samples),
                Message::StreamFailed { stream, error } => {
                    if self.mic.as_ref().is_some_and(|mic| mic.stream == stream) {
                        self.close_mic();
                        self.report(format!("Lost the microphone: {error}"));
                    }
                }
            }
        }
    }

    /// Changes the status and tells the UI.
    fn update(&self, change: impl FnOnce(&mut HearingStatus)) {
        let mut status = self.status.lock().unwrap();
        change(&mut status);
        let _ = self.app.emit(STATUS_EVENT, &*status);
    }

    fn report(&self, message: String) {
        problems::report(&self.app, message);
    }

    fn load_model(&mut self, path: &std::path::Path) {
        self.update(|status| status.model = ModelStatus::Loading);
        match Transcriber::load(path) {
            Ok(transcriber) => {
                self.transcriber = Some(transcriber);
                self.update(|status| status.model = ModelStatus::Ready);
            }
            Err(error) => self.update(|status| status.model = ModelStatus::Failed { error }),
        }
    }

    fn open_mic(&mut self) {
        self.mic = None;
        self.next_stream += 1;
        let stream = self.next_stream;
        let audio = self.messages.clone();
        let errors = self.messages.clone();
        let microphone = self.status.lock().unwrap().microphone.clone();

        let opened = capture::open(
            microphone.as_deref(),
            move |samples| {
                let _ = audio.send(Message::Audio { stream, samples });
            },
            move |error| {
                let _ = errors.send(Message::StreamFailed { stream, error });
            },
        );
        match opened {
            Ok(capture) => {
                let name = capture.name.clone();
                self.mic = Some(Mic {
                    resampler: Resampler::new(capture.sample_rate),
                    _capture: capture,
                    stream,
                    detector: PauseDetector::default(),
                    opened: Instant::now(),
                    heard_sound: false,
                    warned_silent: false,
                    level: 0.0,
                    level_sent: Instant::now(),
                });
                self.update(|status| {
                    status.listening = true;
                    status.microphone_name = Some(name);
                });
            }
            Err(error) => {
                self.close_mic();
                self.report(error);
            }
        }
    }

    fn close_mic(&mut self) {
        // Closing mid-sentence still ends the sentence.
        if self
            .mic
            .take()
            .is_some_and(|mic| mic.detector.is_speaking())
        {
            self.engine.send(Command::Heard(None));
        }
        let _ = self.app.emit(LEVEL_EVENT, 0.0);
        self.update(|status| {
            status.listening = false;
            status.microphone_name = None;
        });
    }

    fn hear(&mut self, stream: u64, samples: &[f32]) {
        let Some(mic) = self.mic.as_mut().filter(|mic| mic.stream == stream) else {
            // Audio still in flight from a mic that was closed.
            return;
        };

        mic.heard_sound |= samples.iter().any(|&s| s != 0.0);
        if !mic.heard_sound && !mic.warned_silent && mic.opened.elapsed() >= SILENT_MIC_WARNING {
            mic.warned_silent = true;
            let place = if cfg!(target_os = "macos") {
                "System Settings → Privacy & Security → Microphone"
            } else {
                "Settings → Privacy & security → Microphone"
            };
            problems::report(
                &self.app,
                format!(
                    "Your microphone is sending complete silence. If it isn't muted, allow microphone access for Small Streamer Chat in {place}, then press Stop and Start."
                ),
            );
        }

        mic.level = mic.level.max(loudness(samples));
        if mic.level_sent.elapsed() >= LEVEL_INTERVAL {
            let _ = self.app.emit(LEVEL_EVENT, mic.level);
            mic.level = 0.0;
            mic.level_sent = Instant::now();
        }

        let mut resampled = Vec::with_capacity(samples.len());
        mic.resampler.process(samples, &mut resampled);
        let was_speaking = mic.detector.is_speaking();
        let utterances = mic.detector.push(&resampled);
        let is_speaking = mic.detector.is_speaking();

        // Tell the engine when the streamer starts and stops talking, so the
        // Practice Chat replies at pauses instead of talking over them.
        let ended = was_speaking && !is_speaking;
        if ended && utterances.is_empty() {
            // Too short to be words, like a cough.
            self.engine.send(Command::Heard(None));
        }
        for utterance in &utterances {
            let text = self.transcribe(utterance);
            self.engine.send(Command::Heard(text));
        }
        if is_speaking && (!was_speaking || !utterances.is_empty()) {
            self.engine.send(Command::SpeechStarted);
        }
    }

    fn transcribe(&mut self, utterance: &[f32]) -> Option<String> {
        // Without the model there's nothing to do yet; the UI shows why.
        let transcriber = self.transcriber.as_mut()?;
        match transcriber.transcribe(utterance) {
            Ok(Some(text)) => {
                let line = TranscriptLine {
                    id: self.next_line,
                    text: text.clone(),
                };
                self.next_line += 1;
                let _ = self.app.emit(TRANSCRIPT_EVENT, line);
                Some(text)
            }
            Ok(None) => None,
            Err(error) if !self.transcription_failed => {
                self.transcription_failed = true;
                self.report(error);
                None
            }
            Err(error) => {
                eprintln!("{error}");
                None
            }
        }
    }
}
