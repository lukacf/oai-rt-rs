use std::fmt;

use super::{AudioFormat, ConnectionRole, Error, Result, ServerEvent, ServerFrame, decode_audio};

/// An observed session-relative interval, not a turn or playback-completion marker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioInterval {
    pub start_ms: f64,
    pub end_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioSource {
    Output,
    /// Sideband copy before model-input muting.
    ReflectedInput,
}

/// Raw mono audio suitable for an application-owned playback/recording pipeline.
#[derive(Clone, PartialEq)]
pub struct AudioChunk {
    pub bytes: Vec<u8>,
    pub format: AudioFormat,
    pub source: AudioSource,
    pub interval: Option<AudioInterval>,
}

impl fmt::Debug for AudioChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioChunk")
            .field("byte_count", &self.bytes.len())
            .field("format", &self.format)
            .field("source", &self.source)
            .field("interval", &self.interval)
            .finish()
    }
}

impl ServerFrame {
    /// Decode audio without dropping accompanying events from the receiver.
    ///
    /// Primary output uses the session's configured codec. Sideband copies are
    /// always PCM16LE at 24 kHz, irrespective of negotiated media or primary codec.
    ///
    /// # Errors
    /// Rejects invalid audio or an incomplete sideband output interval.
    pub fn audio(
        &self,
        role: ConnectionRole,
        primary_format: AudioFormat,
    ) -> Result<Option<AudioChunk>> {
        let format = if role == ConnectionRole::Sideband {
            AudioFormat::default()
        } else {
            primary_format
        };
        match &self.event {
            ServerEvent::OutputAudioDelta {
                delta,
                start_ms,
                end_ms,
            } => {
                let interval = if role == ConnectionRole::Sideband {
                    match (start_ms, end_ms) {
                        (Some(start_ms), Some(end_ms)) => Some(AudioInterval {
                            start_ms: *start_ms,
                            end_ms: *end_ms,
                        }),
                        _ => {
                            return Err(Error::Invalid(
                                "sideband output requires start_ms and end_ms".into(),
                            ));
                        }
                    }
                } else {
                    None
                };
                Ok(Some(AudioChunk {
                    bytes: decode_audio(delta, format)?,
                    format,
                    source: AudioSource::Output,
                    interval,
                }))
            }
            ServerEvent::InputAudio { audio } if role == ConnectionRole::Sideband => {
                Ok(Some(AudioChunk {
                    bytes: decode_audio(audio, format)?,
                    format,
                    source: AudioSource::ReflectedInput,
                    interval: None,
                }))
            }
            _ => Ok(None),
        }
    }
}
