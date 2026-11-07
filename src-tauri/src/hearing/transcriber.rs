//! Speech to text with Whisper (whisper.cpp), on this computer. Uses the GPU
//! through Metal on macOS and the CPU on Windows.

use std::path::Path;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use super::signal::SAMPLE_RATE;

/// Segments Whisper thinks are probably not speech are dropped.
const MAX_NO_SPEECH_PROBABILITY: f32 = 0.6;

pub struct Transcriber {
    state: WhisperState,
    threads: i32,
}

impl Transcriber {
    pub fn load(model: &Path) -> Result<Self, String> {
        // Route whisper.cpp's console logging away from stderr.
        whisper_rs::install_logging_hooks();
        let context = WhisperContext::new_with_params(model, WhisperContextParameters::default())
            .map_err(|e| format!("Couldn't load the speech model: {e}"))?;
        let state = context
            .create_state()
            .map_err(|e| format!("Couldn't start the speech model: {e}"))?;
        // Leave most cores to the game being streamed.
        let threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).clamp(1, 4))
            .unwrap_or(2) as i32;
        Ok(Self { state, threads })
    }

    /// Returns what was said, or `None` if it wasn't words.
    pub fn transcribe(&mut self, audio: &[f32]) -> Result<Option<String>, String> {
        // Whisper ignores clips under a second; pad short ones with silence.
        let min_len = SAMPLE_RATE as usize * 11 / 10;
        let mut padded;
        let audio = if audio.len() < min_len {
            padded = audio.to_vec();
            padded.resize(min_len, 0.0);
            &padded[..]
        } else {
            audio
        };

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.threads);
        // Detect the language each time; streamers switch languages.
        params.set_language(Some("auto"));
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_no_timestamps(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        self.state
            .full(params, audio)
            .map_err(|e| format!("Speech recognition failed: {e}"))?;

        let text = self
            .state
            .as_iter()
            .filter(|segment| segment.no_speech_probability() <= MAX_NO_SPEECH_PROBABILITY)
            .filter_map(|segment| segment.to_str_lossy().ok().map(|text| text.into_owned()))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(clean(&text))
    }
}

/// Drops empty results and Whisper's markers for non-speech, like
/// "[BLANK_AUDIO]" or "(music)".
fn clean(text: &str) -> Option<String> {
    let text = text.trim();
    let is_marker = |open, close| text.starts_with(open) && text.ends_with(close);
    if text.is_empty() || is_marker('[', ']') || is_marker('(', ')') || is_marker('*', '*') {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Transcribes a real recording and prints how long it took. Needs the
    /// downloaded model and a 16 kHz mono 16-bit WAV file:
    ///
    /// ```sh
    /// SSC_MODEL=path/to/ggml-base-q5_1.bin SSC_WAV=speech.wav \
    ///   cargo test --release transcribes_a_recording -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs the speech model and a recording"]
    fn transcribes_a_recording() {
        let model = std::env::var("SSC_MODEL").expect("set SSC_MODEL");
        let wav = std::fs::read(std::env::var("SSC_WAV").expect("set SSC_WAV")).unwrap();
        let data = wav
            .windows(4)
            .position(|w| w == b"data")
            .expect("not a WAV file")
            + 8;
        let audio: Vec<f32> = wav[data..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&bytes| f32::from(i16::from_le_bytes(bytes)) / 32768.0)
            .collect();

        let mut transcriber = Transcriber::load(Path::new(&model)).unwrap();
        let started = std::time::Instant::now();
        let text = transcriber.transcribe(&audio).unwrap();
        println!(
            "{:.1} s of audio took {:?} on {} threads: {text:?}",
            audio.len() as f32 / SAMPLE_RATE as f32,
            started.elapsed(),
            transcriber.threads
        );
        assert!(text.is_some());
    }

    #[test]
    fn non_speech_markers_are_dropped() {
        assert_eq!(clean(" [BLANK_AUDIO]"), None);
        assert_eq!(clean("(music)"), None);
        assert_eq!(clean("*laughs*"), None);
        assert_eq!(clean("   "), None);
        assert_eq!(clean(" Let's go! "), Some("Let's go!".into()));
    }
}
