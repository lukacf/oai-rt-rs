#![allow(dead_code)] // Each independently compiled probe uses a different subset.

use oai_rt_rs::live::{AudioFormat, Error, LiveConnection, Result, ServerEvent, ServerFrame};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};

pub fn check_frame(frame: &ServerFrame) -> Result<()> {
    check_frame_for(frame, false)
}

pub fn check_frame_for(frame: &ServerFrame, browser_denial_expected: bool) -> Result<()> {
    if let ServerEvent::Error { error, .. } = &frame.event {
        if browser_denial_expected
            && error.error_type == "invalid_request_error"
            && error.code.as_deref() == Some("event_not_allowed")
            && error.client_event_id.as_deref() == Some("browser-restricted")
        {
            return Ok(());
        }
        return Err(Error::Provider(error.clone()));
    }
    frame.response_event()?;
    Ok(())
}

pub struct Finalization {
    pub closed: Result<ServerFrame>,
    pub unexpected: Option<Error>,
}

impl Finalization {
    pub fn finish(self) -> Result<ServerFrame> {
        if let Ok(frame) = &self.closed {
            if let ServerEvent::Closed { usage, .. } = &frame.event {
                eprintln!(
                    "{}",
                    json!({"final_usage_confirmed":true,"final_seconds":usage.seconds})
                );
            }
        }
        if let Some(error) = self.unexpected {
            return Err(error);
        }
        self.closed
    }
}

pub async fn finalize(connection: &mut LiveConnection) -> Finalization {
    finalize_for(connection, false).await
}

pub async fn finalize_for(
    connection: &mut LiveConnection,
    browser_denial_expected: bool,
) -> Finalization {
    let mut unexpected = None;
    let closed = connection
        .close_with_events(Duration::from_secs(10), |result| {
            let result = result.and_then(|frame| check_frame_for(&frame, browser_denial_expected));
            if let Err(error) = result {
                if unexpected.is_none() {
                    unexpected = Some(error);
                }
            }
            Ok(())
        })
        .await;
    Finalization { closed, unexpected }
}

pub async fn close(connection: &mut LiveConnection) -> Result<ServerFrame> {
    finalize(connection).await.finish()
}

pub async fn close_for(
    connection: &mut LiveConnection,
    browser_denial_expected: bool,
) -> Result<ServerFrame> {
    finalize_for(connection, browser_denial_expected)
        .await
        .finish()
}

pub async fn started(connection: &mut LiveConnection) -> Result<oai_rt_rs::live::SessionSnapshot> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = connection
                .next_event()
                .await?
                .ok_or(Error::UnconfirmedClose)?;
            check_frame(&frame)?;
            if let ServerEvent::Started { session, .. } = frame.event {
                return Ok(session);
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

pub fn append_text(target: &mut String, text: &str) -> Result<()> {
    if target.len().saturating_add(text.len()) > 65_536 {
        return Err(Error::Invalid("probe transcript capacity exceeded".into()));
    }
    target.push_str(text);
    Ok(())
}

pub struct Acks {
    expected: &'static [(&'static str, &'static str)],
    seen: BTreeSet<usize>,
}

impl Acks {
    pub const fn new(expected: &'static [(&'static str, &'static str)]) -> Self {
        Self {
            expected,
            seen: BTreeSet::new(),
        }
    }

    pub fn observe(&mut self, frame: &ServerFrame) {
        let kind = match frame.event {
            ServerEvent::ThinkingAppended { .. } => "session.thinking.appended",
            ServerEvent::CommentaryAppended { .. } => "session.commentary.appended",
            ServerEvent::InstructionsAppended { .. } => "session.instructions.appended",
            _ => return,
        };
        if let Some(index) = self.expected.iter().position(|(expected_kind, id)| {
            *expected_kind == kind && frame.client_event_id.as_deref() == Some(*id)
        }) {
            self.seen.insert(index);
        }
    }

    pub fn complete(&self) -> bool {
        self.seen.len() == self.expected.len()
    }
    pub fn count(&self) -> usize {
        self.seen.len()
    }
}

pub fn verify_identity(source: Option<&str>, created: &str, attached: &str) -> Result<()> {
    if created != attached || source == Some(created) {
        return Err(Error::Invalid(
            "created, source and attached session identities do not match the requested operation"
                .into(),
        ));
    }
    Ok(())
}

/// 20 ms windows require RMS >= 300 and >= 10% of samples above amplitude 500.
/// Speech qualification needs 200 ms total and a contiguous 100 ms run.
#[derive(Default)]
pub struct Speech {
    rate: u32,
    window_samples: u32,
    window_active: u32,
    window_energy: u64,
    voiced_windows: u32,
    run_windows: u32,
    longest_run: u32,
    pub active_samples: u64,
}

impl Speech {
    pub fn add(&mut self, bytes: &[u8], format: AudioFormat) -> Result<()> {
        if self.rate != 0 && self.rate != format.sample_rate() {
            return Err(Error::Invalid("probe audio rate changed".into()));
        }
        self.rate = format.sample_rate();
        match format {
            AudioFormat::Pcm { .. } => {
                if bytes.len() % 2 != 0 {
                    return Err(Error::Invalid("partial PCM sample".into()));
                }
                for sample in bytes.chunks_exact(2) {
                    self.sample(i16::from_le_bytes([sample[0], sample[1]]));
                }
            }
            AudioFormat::Pcmu { .. } => {
                for byte in bytes {
                    self.sample(decode_mulaw(*byte));
                }
            }
            AudioFormat::Pcma { .. } => {
                for byte in bytes {
                    self.sample(decode_alaw(*byte));
                }
            }
        }
        Ok(())
    }

    fn sample(&mut self, sample: i16) {
        let magnitude = u64::from(sample.unsigned_abs());
        self.window_samples += 1;
        self.window_energy += magnitude * magnitude;
        if magnitude >= 500 {
            self.window_active += 1;
            self.active_samples += 1;
        }
        if self.window_samples == self.rate / 50 {
            if self.window_active * 10 >= self.window_samples
                && self.window_energy >= u64::from(self.window_samples) * 300 * 300
            {
                self.voiced_windows += 1;
                self.run_windows += 1;
                self.longest_run = self.longest_run.max(self.run_windows);
            } else {
                self.run_windows = 0;
            }
            self.window_samples = 0;
            self.window_active = 0;
            self.window_energy = 0;
        }
    }

    pub const fn qualified(&self) -> bool {
        self.voiced_windows >= 10 && self.longest_run >= 5
    }
    pub fn report(&self) -> Value {
        json!({"voiced_ms":self.voiced_windows * 20,"continuous_voiced_ms":self.longest_run * 20,
            "active_samples":self.active_samples,"sample_rate":self.rate})
    }
}

pub fn decode_mulaw(byte: u8) -> i16 {
    let value = !byte;
    let magnitude = ((i16::from(value & 15) << 3) + 132) << ((value >> 4) & 7);
    if value & 128 != 0 {
        132 - magnitude
    } else {
        magnitude - 132
    }
}

pub fn decode_alaw(byte: u8) -> i16 {
    let value = byte ^ 0x55;
    let exponent = (value >> 4) & 7;
    let magnitude = (i16::from(value & 15) << 4) + if exponent == 0 { 8 } else { 264 };
    let magnitude = if exponent > 1 {
        magnitude << (exponent - 1)
    } else {
        magnitude
    };
    if value & 128 != 0 {
        magnitude
    } else {
        -magnitude
    }
}
