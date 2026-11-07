//! Audio processing that doesn't touch any device: converting the mic's audio
//! to what Whisper needs, and splitting it into utterances at pauses.

use std::{collections::VecDeque, f32::consts::PI, time::Duration};

/// Whisper wants 16 kHz mono audio.
pub const SAMPLE_RATE: u32 = 16_000;

/// Converts mono audio at the mic's sample rate to 16 kHz, a chunk at a time.
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Position of the next output sample, counted in input samples from
    /// `previous` (the last sample of the previous chunk).
    position: f64,
    previous: f32,
    low_pass: [LowPass; 2],
}

impl Resampler {
    pub fn new(input_rate: u32) -> Self {
        // Below half the output rate, so high sounds don't fold back down
        // (alias) into the speech range when downsampling.
        let cutoff = 0.45 * SAMPLE_RATE as f32;
        let low_pass = LowPass::new(cutoff, input_rate);
        Self {
            step: f64::from(input_rate) / f64::from(SAMPLE_RATE),
            position: 1.0,
            previous: 0.0,
            low_pass: [low_pass.clone(), low_pass],
        }
    }

    pub fn process(&mut self, input: &[f32], output: &mut Vec<f32>) {
        let Some(&last) = input.last() else { return };
        let filtered: Vec<f32> = if self.step > 1.0 {
            input
                .iter()
                .map(|&x| {
                    let once = self.low_pass[0].apply(x);
                    self.low_pass[1].apply(once)
                })
                .collect()
        } else {
            input.to_vec()
        };

        let len = filtered.len() as f64;
        while self.position < len {
            let index = self.position as usize;
            let fraction = (self.position - index as f64) as f32;
            let a = if index == 0 {
                self.previous
            } else {
                filtered[index - 1]
            };
            let b = filtered[index];
            output.push(a + (b - a) * fraction);
            self.position += self.step;
        }
        self.position -= len;
        self.previous = if self.step > 1.0 {
            *filtered.last().unwrap()
        } else {
            last
        };
    }
}

/// One-pole low-pass filter.
#[derive(Clone)]
struct LowPass {
    alpha: f32,
    state: f32,
}

impl LowPass {
    fn new(cutoff: f32, sample_rate: u32) -> Self {
        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * PI * cutoff);
        Self {
            alpha: dt / (rc + dt),
            state: 0.0,
        }
    }

    fn apply(&mut self, x: f32) -> f32 {
        self.state += self.alpha * (x - self.state);
        self.state
    }
}

/// Root mean square: how loud a stretch of audio is, from 0 to 1.
pub fn loudness(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
}

const fn frames(duration: Duration) -> usize {
    (duration.as_millis() as usize) / FRAME_MS
}

const FRAME_MS: usize = 20;
const FRAME_LEN: usize = SAMPLE_RATE as usize * FRAME_MS / 1000;
/// Silence this long after speech ends an utterance. It's also the "pause"
/// that lets the Practice Chat reply.
const PAUSE: usize = frames(Duration::from_millis(700));
/// Audio kept from just before speech starts, so the first word isn't cut.
const LEAD_IN: usize = frames(Duration::from_millis(300));
/// Silence kept at the end of an utterance.
const TAIL: usize = frames(Duration::from_millis(200));
/// Less speech than this is a cough or a click, not words.
const MIN_SPEECH: usize = frames(Duration::from_millis(300));
/// Someone who talks without pausing still gets transcribed this often.
const MAX_UTTERANCE: usize = frames(Duration::from_secs(15));
/// Quietest level that can count as speech, so a silent room isn't "speech".
const MIN_SPEECH_LOUDNESS: f32 = 0.008;
/// Speech must be this many times louder than the background noise.
const SPEECH_OVER_NOISE: f32 = 3.0;

/// Splits 16 kHz audio into utterances: stretches of speech that end at a
/// pause. Loudness is compared against a running estimate of the room's
/// background noise, so a fan or a quiet game doesn't count as talking.
pub struct PauseDetector {
    pending: Vec<f32>,
    lead_in: VecDeque<Vec<f32>>,
    utterance: Vec<Vec<f32>>,
    speech_frames: usize,
    silent_frames: usize,
    noise_floor: f32,
}

impl Default for PauseDetector {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            lead_in: VecDeque::new(),
            utterance: Vec::new(),
            speech_frames: 0,
            silent_frames: 0,
            noise_floor: 0.002,
        }
    }
}

impl PauseDetector {
    /// Feeds audio in and returns any utterances it completed.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Vec<f32>> {
        self.pending.extend_from_slice(samples);
        let mut finished = Vec::new();
        let whole_frames = self.pending.len() / FRAME_LEN * FRAME_LEN;
        let frames: Vec<Vec<f32>> = self
            .pending
            .drain(..whole_frames)
            .collect::<Vec<_>>()
            .chunks(FRAME_LEN)
            .map(<[f32]>::to_vec)
            .collect();
        for frame in frames {
            if let Some(utterance) = self.push_frame(frame) {
                finished.push(utterance);
            }
        }
        finished
    }

    fn push_frame(&mut self, frame: Vec<f32>) -> Option<Vec<f32>> {
        let level = loudness(&frame);
        let is_speech = level >= (self.noise_floor * SPEECH_OVER_NOISE).max(MIN_SPEECH_LOUDNESS);

        self.track_noise(level);

        if self.utterance.is_empty() {
            if is_speech {
                self.utterance.extend(self.lead_in.drain(..));
                self.utterance.push(frame);
                self.speech_frames = 1;
                self.silent_frames = 0;
            } else {
                self.lead_in.push_back(frame);
                if self.lead_in.len() > LEAD_IN {
                    self.lead_in.pop_front();
                }
            }
            return None;
        }

        self.utterance.push(frame);
        if is_speech {
            self.speech_frames += 1;
            self.silent_frames = 0;
        } else {
            self.silent_frames += 1;
        }

        if self.silent_frames >= PAUSE {
            let trailing_silence = self.silent_frames - TAIL;
            self.utterance
                .truncate(self.utterance.len() - trailing_silence);
            return self.finish();
        }
        if self.utterance.len() >= MAX_UTTERANCE {
            return self.finish();
        }
        None
    }

    fn finish(&mut self) -> Option<Vec<f32>> {
        let frames = std::mem::take(&mut self.utterance);
        let speech_frames = std::mem::take(&mut self.speech_frames);
        self.silent_frames = 0;
        (speech_frames >= MIN_SPEECH).then(|| frames.concat())
    }

    fn track_noise(&mut self, level: f32) {
        // Falls quickly when it's quieter (speech always has short gaps
        // between syllables), rises slowly when it's louder. So a steady hum
        // becomes "background" after a second or two, but talking doesn't.
        let rate = if level < self.noise_floor { 0.2 } else { 0.005 };
        self.noise_floor += rate * (level - self.noise_floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, seconds: f32, frequency: f32, amplitude: f32) -> Vec<f32> {
        let count = (rate as f32 * seconds) as usize;
        (0..count)
            .map(|i| amplitude * (2.0 * PI * frequency * i as f32 / rate as f32).sin())
            .collect()
    }

    /// Speech-like audio: a tone whose loudness rises and falls three times
    /// a second, like syllables.
    fn speech(seconds: f32) -> Vec<f32> {
        tone(SAMPLE_RATE, seconds, 300.0, 0.4)
            .into_iter()
            .enumerate()
            .map(|(i, x)| {
                let t = i as f32 / SAMPLE_RATE as f32;
                x * (2.0 * PI * 3.0 * t).sin().abs()
            })
            .collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (SAMPLE_RATE as f32 * seconds) as usize]
    }

    #[test]
    fn resampling_48k_gives_a_third_as_many_samples() {
        let input = tone(48_000, 1.0, 440.0, 0.5);
        let mut resampler = Resampler::new(48_000);
        let mut output = Vec::new();
        // Uneven chunk sizes, like a real audio callback.
        for chunk in input.chunks(441) {
            resampler.process(chunk, &mut output);
        }
        assert!(
            (output.len() as i64 - 16_000).abs() <= 1,
            "{}",
            output.len()
        );
    }

    #[test]
    fn resampling_keeps_speech_frequencies() {
        let input = tone(44_100, 1.0, 440.0, 0.5);
        let mut output = Vec::new();
        Resampler::new(44_100).process(&input, &mut output);
        let level = loudness(&output[1600..]);
        // A 0.5 sine has an RMS of ~0.354.
        assert!((level - 0.354).abs() < 0.03, "{level}");
    }

    #[test]
    fn resampling_removes_frequencies_too_high_for_16k() {
        let input = tone(48_000, 1.0, 15_000.0, 0.5);
        let mut output = Vec::new();
        Resampler::new(48_000).process(&input, &mut output);
        assert!(loudness(&output[1600..]) < 0.1);
    }

    #[test]
    fn an_utterance_ends_at_a_pause() {
        let mut detector = PauseDetector::default();
        assert!(detector.push(&silence(1.0)).is_empty());
        assert!(detector.push(&speech(1.5)).is_empty());
        let utterances = detector.push(&silence(1.0));
        assert_eq!(utterances.len(), 1);
        let seconds = utterances[0].len() as f32 / SAMPLE_RATE as f32;
        // 1.5 s of speech plus lead-in and tail.
        assert!((1.5..=2.1).contains(&seconds), "{seconds}");
    }

    #[test]
    fn a_short_gap_between_words_is_not_a_pause() {
        let mut detector = PauseDetector::default();
        let mut audio = speech(1.0);
        audio.extend(silence(0.3));
        audio.extend(speech(1.0));
        audio.extend(silence(1.0));
        assert_eq!(detector.push(&audio).len(), 1);
    }

    #[test]
    fn a_click_is_not_an_utterance() {
        let mut detector = PauseDetector::default();
        let mut audio = silence(0.5);
        audio.extend(tone(SAMPLE_RATE, 0.1, 300.0, 0.5));
        audio.extend(silence(1.0));
        assert!(detector.push(&audio).is_empty());
    }

    #[test]
    fn steady_background_noise_is_not_speech() {
        let mut detector = PauseDetector::default();
        // A fan-like hum, well above the minimum speech loudness. Once the
        // detector has heard it for a few seconds, it's background.
        let hum = || tone(SAMPLE_RATE, 5.0, 120.0, 0.03);
        detector.push(&hum());
        assert!(detector.push(&hum()).is_empty());
        // Talking over it still counts.
        let talking: Vec<f32> = speech(1.0).iter().zip(hum()).map(|(a, b)| a + b).collect();
        assert!(detector.push(&talking).is_empty());
        assert_eq!(detector.push(&hum()).len(), 1);
    }

    #[test]
    fn non_stop_talking_is_still_cut_into_utterances() {
        let mut detector = PauseDetector::default();
        let utterances = detector.push(&speech(31.0));
        assert_eq!(utterances.len(), 2);
    }
}
