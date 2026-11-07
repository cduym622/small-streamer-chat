//! Microphone access through cpal (Core Audio on macOS, WASAPI on Windows).

use cpal::{
    Device, ErrorKind, FromSample, Sample, SizedSample, Stream,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Microphone {
    pub id: String,
    pub name: String,
}

pub fn microphones() -> Vec<Microphone> {
    let Ok(devices) = cpal::default_host().input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            Some(Microphone {
                id: device.id().ok()?.to_string(),
                name: device.description().ok()?.name().to_string(),
            })
        })
        .collect()
}

/// A running microphone stream. Audio stops when it's dropped.
pub struct Capture {
    _stream: Stream,
    pub sample_rate: u32,
    pub name: String,
}

/// Opens the microphone with this id, or the system default for `None` or a
/// mic that's no longer plugged in. `on_audio` gets mono samples at
/// `Capture::sample_rate`; `on_error` is called if the stream breaks.
pub fn open(
    id: Option<&str>,
    on_audio: impl FnMut(Vec<f32>) + Send + 'static,
    on_error: impl FnMut(String) + Send + 'static,
) -> Result<Capture, String> {
    let host = cpal::default_host();
    let device = id
        .and_then(|id| host.device_by_id(&id.parse().ok()?))
        .or_else(|| host.default_input_device())
        .ok_or("No microphone found.")?;
    let name = device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| "Microphone".into());
    let config = device
        .default_input_config()
        .map_err(|e| format!("Couldn't use {name}: {e}"))?;
    let sample_rate = config.sample_rate();

    use cpal::SampleFormat as F;
    let stream = match config.sample_format() {
        F::F32 => build::<f32>(&device, config.into(), on_audio, on_error),
        F::I16 => build::<i16>(&device, config.into(), on_audio, on_error),
        F::I32 => build::<i32>(&device, config.into(), on_audio, on_error),
        F::U16 => build::<u16>(&device, config.into(), on_audio, on_error),
        F::I8 => build::<i8>(&device, config.into(), on_audio, on_error),
        F::U8 => build::<u8>(&device, config.into(), on_audio, on_error),
        other => {
            return Err(format!(
                "{name} uses an unsupported audio format ({other})."
            ));
        }
    }
    .map_err(|e| format!("Couldn't open {name}: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Couldn't start {name}: {e}"))?;

    Ok(Capture {
        _stream: stream,
        sample_rate,
        name,
    })
}

fn build<T>(
    device: &Device,
    config: cpal::StreamConfig,
    mut on_audio: impl FnMut(Vec<f32>) + Send + 'static,
    mut on_error: impl FnMut(String) + Send + 'static,
) -> Result<Stream, cpal::Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
    device.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            // Mix all channels down to mono.
            let mono = data
                .chunks(channels)
                .map(|frame| {
                    frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() / channels as f32
                })
                .collect();
            on_audio(mono);
        },
        move |error: cpal::Error| match error.kind() {
            // The stream keeps going after these.
            ErrorKind::DeviceChanged | ErrorKind::Xrun | ErrorKind::RealtimeDenied => {}
            _ => on_error(error.to_string()),
        },
        None,
    )
}
