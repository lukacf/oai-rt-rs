#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::result_large_err,
    clippy::suboptimal_flops,
    clippy::use_self
)]

use base64::Engine as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use futures::StreamExt;
use oai_rt_rs::{ClientEvent, GPT_REALTIME_TRANSLATE, Realtime, Result, SdkEvent, SessionHandle};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const SAMPLE_RATE: u32 = 24_000;
const CHUNK_SAMPLES: usize = 2_400;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        oai_rt_rs::Error::InvalidClientEvent("OPENAI_API_KEY must be set".to_string())
    })?;
    let language = std::env::args().nth(1).ok_or_else(|| {
        oai_rt_rs::Error::InvalidClientEvent(
            "usage: cargo run --example ws_translate_mic -- <target-language>".to_string(),
        )
    })?;

    println!("connecting to {GPT_REALTIME_TRANSLATE}; target language: {language}");
    let mut session = Realtime::translation_builder()
        .api_key(api_key)
        .translation_language(language)
        .connect_ws()
        .await?;

    let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<i16>>(16);
    let stream = start_microphone(audio_tx)?;
    stream.play().map_err(invalid_client_event)?;

    println!("listening. Speak into the selected microphone; press Ctrl-C to stop.");
    let handle = session.handle();
    let mut events = session.events();
    let mut input = TranscriptLine::new("source");
    let mut output = TranscriptLine::new("translation");

    loop {
        tokio::select! {
            chunk = audio_rx.recv() => {
                let Some(chunk) = chunk else {
                    break;
                };
                send_translation_audio(&handle, &chunk).await?;
            }
            event = events.next() => {
                let Some(event) = event else {
                    break;
                };
                match event {
                    SdkEvent::TranslationInputTranscriptDelta { delta } => input.push(&delta),
                    SdkEvent::TranslationOutputTranscriptDelta { delta } => output.push(&delta),
                    SdkEvent::Error { error, .. } => {
                        input.break_line();
                        output.break_line();
                        eprintln!("server error: {error:?}");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn send_translation_audio(handle: &SessionHandle, samples: &[i16]) -> Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let audio = base64::engine::general_purpose::STANDARD.encode(bytes);
    handle
        .send_raw(ClientEvent::SessionInputAudioBufferAppend {
            event_id: None,
            audio,
        })
        .await
}

fn start_microphone(sender: mpsc::Sender<Vec<i16>>) -> Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| {
        oai_rt_rs::Error::InvalidClientEvent("no default input device found".to_string())
    })?;
    let supported = device
        .default_input_config()
        .map_err(invalid_client_event)?;
    let config: StreamConfig = supported.clone().into();
    let sample_format = supported.sample_format();
    let sample_rate = config.sample_rate;
    let channels = config.channels;
    println!(
        "microphone: {sample_rate} Hz, {channels} channel(s); converting capture to 24 kHz mono"
    );
    let buffer = Arc::new(Mutex::new(Vec::<i16>::with_capacity(CHUNK_SAMPLES * 2)));
    let err_fn = |err| eprintln!("microphone stream error: {err}");

    match sample_format {
        SampleFormat::I16 => build_input_stream::<i16>(
            &device,
            &config,
            sender,
            buffer,
            sample_rate,
            channels,
            err_fn,
        ),
        SampleFormat::U16 => build_input_stream::<u16>(
            &device,
            &config,
            sender,
            buffer,
            sample_rate,
            channels,
            err_fn,
        ),
        SampleFormat::F32 => build_input_stream::<f32>(
            &device,
            &config,
            sender,
            buffer,
            sample_rate,
            channels,
            err_fn,
        ),
        _ => Err(oai_rt_rs::Error::InvalidClientEvent(format!(
            "unsupported microphone sample format: {sample_format:?}"
        ))),
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    sender: mpsc::Sender<Vec<i16>>,
    buffer: Arc<Mutex<Vec<i16>>>,
    source_rate: u32,
    channels: u16,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + Copy + Send + 'static,
    i16: FromSample<T>,
{
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if let Ok(mut pending) = buffer.lock() {
                    let mono = input_to_mono_pcm16(data, channels);
                    let converted = resample_linear(&mono, source_rate, SAMPLE_RATE);
                    for sample in converted {
                        pending.push(sample);
                        if pending.len() >= CHUNK_SAMPLES {
                            let chunk = pending.drain(..CHUNK_SAMPLES).collect::<Vec<_>>();
                            if sender.blocking_send(chunk).is_err() {
                                return;
                            }
                        }
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(invalid_client_event)?;
    Ok(stream)
}

fn input_to_mono_pcm16<T>(data: &[T], channels: u16) -> Vec<i16>
where
    T: Copy,
    i16: FromSample<T>,
{
    let channels = usize::from(channels.max(1));
    let mut mono = Vec::with_capacity(data.len() / channels);
    for frame in data.chunks_exact(channels) {
        let sum: i32 = frame
            .iter()
            .map(|sample| i32::from(i16::from_sample(*sample)))
            .sum();
        mono.push((sum / i32::try_from(channels).unwrap_or(1)) as i16);
    }
    mono
}

fn resample_linear(samples: &[i16], source_rate: u32, target_rate: u32) -> Vec<i16> {
    if samples.len() < 2 || source_rate == target_rate {
        return samples.to_vec();
    }

    let output_len = (samples.len() as u64 * u64::from(target_rate))
        .div_ceil(u64::from(source_rate))
        .max(1) as usize;
    let mut output = Vec::with_capacity(output_len);
    let step = f64::from(source_rate) / f64::from(target_rate);
    for index in 0..output_len {
        let source = index as f64 * step;
        let left = source.floor() as usize;
        let right = (left + 1).min(samples.len() - 1);
        let frac = source - left as f64;
        let sample = f64::from(samples[left]) * (1.0 - frac) + f64::from(samples[right]) * frac;
        output.push(
            sample
                .round()
                .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16,
        );
    }
    output
}

fn invalid_client_event(error: impl std::fmt::Display) -> oai_rt_rs::Error {
    oai_rt_rs::Error::InvalidClientEvent(error.to_string())
}

trait FromSample<T> {
    fn from_sample(sample: T) -> Self;
}

impl FromSample<i16> for i16 {
    fn from_sample(sample: i16) -> Self {
        sample
    }
}

impl FromSample<u16> for i16 {
    fn from_sample(sample: u16) -> Self {
        let centered = i32::from(sample) - 32_768;
        centered.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    }
}

impl FromSample<f32> for i16 {
    fn from_sample(sample: f32) -> Self {
        let scaled = sample.clamp(-1.0, 1.0) * f32::from(i16::MAX);
        scaled.round() as i16
    }
}

struct TranscriptLine {
    label: &'static str,
    open: bool,
    text: String,
}

impl TranscriptLine {
    const fn new(label: &'static str) -> Self {
        Self {
            label,
            open: false,
            text: String::new(),
        }
    }

    fn push(&mut self, delta: &str) {
        if !self.open {
            print!("{}> ", self.label);
            self.open = true;
        }
        self.text.push_str(delta);
        print!("{delta}");
    }

    fn break_line(&mut self) {
        if self.open {
            println!();
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FromSample, TranscriptLine, input_to_mono_pcm16, resample_linear};

    #[test]
    fn sample_conversion_is_pcm16_little_domain() {
        assert_eq!(i16::from_sample(0i16), 0);
        assert_eq!(i16::from_sample(32_768u16), 0);
        assert_eq!(i16::from_sample(1.0f32), i16::MAX);
        assert_eq!(i16::from_sample(-1.0f32), i16::MIN + 1);
    }

    #[test]
    fn transcript_line_accumulates_deltas() {
        let mut line = TranscriptLine::new("translation");
        line.push("bo");
        line.push("njour");
        assert_eq!(line.text, "bonjour");
        line.break_line();
        assert!(!line.open);
    }

    #[test]
    fn microphone_conversion_downmixes_and_resamples() {
        let mono = input_to_mono_pcm16(&[100i16, 300, -100, 100], 2);
        assert_eq!(mono, vec![200, 0]);
        let doubled = resample_linear(&[0, 24_000], 24_000, 48_000);
        assert!(doubled.len() >= 4);
    }
}
