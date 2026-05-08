#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::result_large_err,
    clippy::significant_drop_tightening,
    clippy::suboptimal_flops,
    clippy::too_many_lines,
    clippy::use_self
)]

use base64::Engine as _;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use futures::StreamExt;
use oai_rt_rs::{
    ClientEvent, GPT_REALTIME_2, GPT_REALTIME_WHISPER, Realtime, ReasoningEffort, Result, SdkEvent,
    ServerError, ServerEvent, SessionHandle, ToolChoice, ToolChoiceMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{self, Write as _};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const SAMPLE_RATE: u32 = 24_000;
const CHUNK_SAMPLES: usize = 2_400;

#[derive(Debug, Deserialize, JsonSchema)]
struct SumArgs {
    a: i64,
    b: i64,
}

#[derive(Debug, Serialize)]
struct SumResult {
    result: i64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        oai_rt_rs::Error::InvalidClientEvent("OPENAI_API_KEY must be set".to_string())
    })?;

    println!("connecting to {GPT_REALTIME_2} voice session...");
    println!(
        "audio note: this WebSocket demo is headset-first. It does not provide acoustic echo cancellation; use headphones, or use the browser WebRTC example for speakerphone-style full duplex."
    );
    let mut session = Realtime::builder()
        .api_key(api_key)
        .model(GPT_REALTIME_2)
        .tool_choice(ToolChoice::Mode(ToolChoiceMode::Auto))
        .voice_session()
        .voice("marin")
        .vad_server_default()
        .transcription(GPT_REALTIME_WHISPER)
        .reasoning_effort(ReasoningEffort::Low)
        .auto_barge_in(true)
        .tool_desc(
            "sum",
            "Add two signed integers.",
            |args: SumArgs| async move {
                Ok(SumResult {
                    result: args.a + args.b,
                })
            },
        )
        .connect_ws()
        .await?;

    let playback = AudioPlayback::start()?;
    let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<i16>>(16);
    let input_stream = start_microphone(audio_tx)?;
    input_stream.play().map_err(invalid_client_event)?;

    let handle = session.handle();
    let mut events = session.events();
    let mut user = LiveLine::new("you");
    let mut assistant = LiveLine::new("assistant");

    println!("listening. Ask something aloud, e.g. 'what is 12 plus 30?'. Press Ctrl-C to stop.");

    loop {
        tokio::select! {
            chunk = audio_rx.recv() => {
                let Some(chunk) = chunk else {
                    break;
                };
                append_input_audio(&handle, &chunk).await?;
            }
            event = events.next() => {
                let Some(event) = event else {
                    break;
                };
                match event {
                    SdkEvent::InputTranscriptionDelta { delta, .. } => user.push(&delta),
                    SdkEvent::InputTranscriptionCompleted { transcript, .. } => {
                        user.finish_with(&transcript);
                    }
                    SdkEvent::TranscriptDelta { delta, .. } | SdkEvent::TextDelta { delta, .. } => {
                        assistant.push(&delta);
                    }
                    SdkEvent::TranscriptDone { transcript, .. } => {
                        assistant.finish_with(&transcript);
                    }
                    SdkEvent::TextDone { text, .. } => {
                        assistant.finish_with(&text);
                    }
                    SdkEvent::AudioDelta { delta, .. } => {
                        playback.push_base64_pcm16(&delta)?;
                    }
                    SdkEvent::ToolCallDelta { call_id, delta, .. } => {
                        user.break_line();
                        assistant.break_line();
                        println!("tool:{call_id}> arguments delta: {delta}");
                    }
                    SdkEvent::ToolCall { name, arguments, .. } => {
                        user.break_line();
                        assistant.break_line();
                        println!("tool:{name}> arguments: {arguments}");
                    }
                    SdkEvent::Raw(raw) => match *raw {
                        ServerEvent::InputAudioBufferSpeechStarted { .. } => {
                            user.break_line();
                            assistant.break_line();
                            playback.clear();
                            println!("barge-in: speech started, clearing local playback");
                        }
                        ServerEvent::ResponseDone { response, .. } => {
                            assistant.break_line();
                            println!("response done: {}", response.id);
                        }
                        _ => {}
                    },
                    SdkEvent::Error { error, .. } if is_benign_cancel_race(&error) => {
                        println!("barge-in: response already finished before cancel landed");
                    }
                    SdkEvent::Error { error, .. } => {
                        user.break_line();
                        assistant.break_line();
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

fn is_benign_cancel_race(error: &ServerError) -> bool {
    error.is_response_cancel_not_active()
}

async fn append_input_audio(handle: &SessionHandle, samples: &[i16]) -> Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let audio = base64::engine::general_purpose::STANDARD.encode(bytes);
    handle
        .send_raw(ClientEvent::InputAudioBufferAppend {
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

struct AudioPlayback {
    _stream: cpal::Stream,
    samples: Arc<Mutex<VecDeque<i16>>>,
    sample_rate: u32,
    channels: u16,
}

impl AudioPlayback {
    fn start() -> Result<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            oai_rt_rs::Error::InvalidClientEvent("no default output device found".to_string())
        })?;
        let supported = device
            .default_output_config()
            .map_err(invalid_client_event)?;
        let config: StreamConfig = supported.clone().into();
        let sample_format = supported.sample_format();
        let sample_rate = config.sample_rate;
        let channels = config.channels;
        println!(
            "speaker: {sample_rate} Hz, {channels} channel(s); converting model audio from 24 kHz mono"
        );
        let samples = Arc::new(Mutex::new(VecDeque::<i16>::new()));
        let err_fn = |err| eprintln!("speaker stream error: {err}");
        let stream = match sample_format {
            SampleFormat::I16 => build_output_stream::<i16>(&device, &config, &samples, err_fn),
            SampleFormat::U16 => build_output_stream::<u16>(&device, &config, &samples, err_fn),
            SampleFormat::F32 => build_output_stream::<f32>(&device, &config, &samples, err_fn),
            _ => Err(oai_rt_rs::Error::InvalidClientEvent(format!(
                "unsupported speaker sample format: {sample_format:?}"
            ))),
        }?;
        stream.play().map_err(invalid_client_event)?;
        Ok(Self {
            _stream: stream,
            samples,
            sample_rate,
            channels,
        })
    }

    fn push_base64_pcm16(&self, delta: &str) -> Result<()> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(delta.as_bytes())
            .map_err(invalid_client_event)?;
        let mut mono = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.chunks_exact(2) {
            mono.push(i16::from_le_bytes([pair[0], pair[1]]));
        }
        let output = convert_mono_pcm16(&mono, SAMPLE_RATE, self.sample_rate, self.channels);
        let mut samples = self.samples.lock().map_err(|_| {
            oai_rt_rs::Error::InvalidClientEvent("speaker queue lock poisoned".to_string())
        })?;
        samples.extend(output);
        Ok(())
    }

    fn clear(&self) {
        if let Ok(mut samples) = self.samples.lock() {
            samples.clear();
        }
    }
}

fn build_output_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    samples: &Arc<Mutex<VecDeque<i16>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + FromI16Sample + Send + 'static,
{
    let samples = Arc::clone(samples);
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                if let Ok(mut pending) = samples.lock() {
                    for sample in data {
                        let next = pending.pop_front().unwrap_or_default();
                        *sample = T::from_i16_sample(next);
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(invalid_client_event)?;
    Ok(stream)
}

fn convert_mono_pcm16(
    samples: &[i16],
    source_rate: u32,
    target_rate: u32,
    channels: u16,
) -> Vec<i16> {
    if samples.is_empty() || channels == 0 {
        return Vec::new();
    }

    let resampled = if source_rate == target_rate {
        samples.to_vec()
    } else {
        resample_linear(samples, source_rate, target_rate)
    };
    let mut output = Vec::with_capacity(resampled.len() * usize::from(channels));
    for sample in resampled {
        output.extend(std::iter::repeat_n(sample, usize::from(channels)));
    }
    output
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

trait FromI16Sample {
    fn from_i16_sample(sample: i16) -> Self;
}

impl FromI16Sample for i16 {
    fn from_i16_sample(sample: i16) -> Self {
        sample
    }
}

impl FromI16Sample for u16 {
    fn from_i16_sample(sample: i16) -> Self {
        let shifted = i32::from(sample) + 32_768;
        shifted.clamp(i32::from(u16::MIN), i32::from(u16::MAX)) as u16
    }
}

impl FromI16Sample for f32 {
    fn from_i16_sample(sample: i16) -> Self {
        f32::from(sample) / f32::from(i16::MAX)
    }
}

fn invalid_client_event(error: impl std::fmt::Display) -> oai_rt_rs::Error {
    oai_rt_rs::Error::InvalidClientEvent(error.to_string())
}

struct LiveLine {
    label: &'static str,
    buffer: String,
    open: bool,
}

impl LiveLine {
    const fn new(label: &'static str) -> Self {
        Self {
            label,
            buffer: String::new(),
            open: false,
        }
    }

    fn push(&mut self, delta: &str) {
        if !self.open {
            print!("{}> ", self.label);
            self.open = true;
        }
        self.buffer.push_str(delta);
        print!("{delta}");
        let _ = io::stdout().flush();
    }

    fn finish_with(&mut self, text: &str) {
        if self.open {
            println!();
        }
        self.buffer.clear();
        self.buffer.push_str(text);
        println!("{} final> {}", self.label, self.buffer);
        self.open = false;
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
    use super::{
        FromI16Sample, FromSample, LiveLine, SAMPLE_RATE, convert_mono_pcm16, input_to_mono_pcm16,
        is_benign_cancel_race,
    };
    use oai_rt_rs::{ApiErrorType, ServerError};

    #[test]
    fn sample_conversion_is_pcm16_little_domain() {
        assert_eq!(i16::from_sample(0i16), 0);
        assert_eq!(i16::from_sample(32_768u16), 0);
        assert_eq!(i16::from_sample(1.0f32), i16::MAX);
        assert_eq!(i16::from_sample(-1.0f32), i16::MIN + 1);
        assert_eq!(i16::from_i16_sample(123), 123);
        assert_eq!(u16::from_i16_sample(0), 32_768);
        assert_eq!(f32::from_i16_sample(i16::MAX), 1.0);
    }

    #[test]
    fn live_line_accumulates_and_resets() {
        let mut line = LiveLine::new("assistant");
        line.push("hel");
        line.push("lo");
        assert_eq!(line.buffer, "hello");
        line.finish_with("hello!");
        assert_eq!(line.buffer, "hello!");
        assert!(!line.open);
    }

    #[test]
    fn playback_conversion_duplicates_channels_and_resamples() {
        let stereo = convert_mono_pcm16(&[100, 200], SAMPLE_RATE, SAMPLE_RATE, 2);
        assert_eq!(stereo, vec![100, 100, 200, 200]);

        let doubled = convert_mono_pcm16(&[0, 24_000], 24_000, 48_000, 1);
        assert!(doubled.len() >= 4);
        assert_eq!(doubled[0], 0);
    }

    #[test]
    fn input_conversion_downmixes_interleaved_stereo() {
        let mono = input_to_mono_pcm16(&[100i16, 300, -100, 100], 2);
        assert_eq!(mono, vec![200, 0]);
    }

    #[test]
    fn response_cancel_not_active_is_benign() {
        let error = ServerError {
            error_type: ApiErrorType::InvalidRequestError,
            code: Some("response_cancel_not_active".to_string()),
            message: "Cancellation failed: no active response found".to_string(),
            param: None,
            event_id: None,
        };
        assert!(is_benign_cancel_race(&error));
    }
}
