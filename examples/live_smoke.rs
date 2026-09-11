//! Explicit, bounded, billable public Live smoke probe. Run with `OPENAI_API_KEY`.
use oai_rt_rs::live::{
    AudioFormat, ClientEvent, Command, Error, Field, LiveClient, LiveConnection, Nullable, Result,
    ServerEvent, SessionConfig, decode_audio,
};
use std::{collections::HashSet, time::Duration};
use tokio::time::{Instant, timeout};

async fn seed(connection: &LiveConnection) -> Result<()> {
    for (id, command) in [
        (
            "thinking-a",
            Command::ThinkingAppend {
                content: "The synthetic test color is blue.".into(),
                delegation_id: Nullable(None),
            },
        ),
        (
            "thinking-b",
            Command::ThinkingAppend {
                content: "The synthetic test number is seven.".into(),
                delegation_id: Nullable(None),
            },
        ),
        (
            "commentary",
            Command::CommentaryAppend {
                content: "Please say: The test is ready.".into(),
                delegation_id: Nullable(None),
            },
        ),
    ] {
        connection
            .send(ClientEvent {
                event_id: Field::Value(id.into()),
                command,
            })
            .await?;
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        Error::Invalid("OPENAI_API_KEY is required; this probe does not skip".into())
    })?;
    let client = LiveClient::new(&key)?;
    let config = SessionConfig {
        instructions: Field::Value("This is a synthetic integration test. When asked to speak, say exactly: The test is ready.".into()),
        ..SessionConfig::default()
    };
    let mut connection = client.connect(config).await?;
    seed(&connection).await?;
    let sender = connection.sender();
    let audio = tokio::spawn(async move {
        sender
            .send_audio_paced(&vec![0; 24_000 * 2 * 6], 2400)
            .await
    });
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut acks = HashSet::new();
    let mut audio_chunks = 0;
    let mut voiced_samples = 0;
    let mut transcript = String::new();
    let probe = async {
        while Instant::now() < deadline {
            let frame = timeout(
                deadline.saturating_duration_since(Instant::now()),
                connection.next_event(),
            )
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or(Error::UnconfirmedClose)?;
            if let Some(id) = frame.client_event_id {
                acks.insert(id);
            }
            match frame.event {
                ServerEvent::OutputAudioDelta { delta, .. } => {
                    let bytes = decode_audio(&delta, AudioFormat::default())?;
                    audio_chunks += 1;
                    let peak = bytes
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]]).unsigned_abs())
                        .max()
                        .unwrap_or(0);
                    if peak >= 500 {
                        voiced_samples += bytes.len() / 2;
                    }
                }
                ServerEvent::OutputTranscriptDelta { delta, .. } => transcript.push_str(&delta),
                ServerEvent::Error { error, .. } => return Err(Error::Provider(error)),
                _ => {}
            }
            if acks.len() == 3 && voiced_samples >= 2400 && transcript.contains("The test is ready")
            {
                break;
            }
        }
        Ok(())
    }
    .await;
    audio.abort();
    let audio_result = audio.await;
    let final_frame = connection
        .close(Duration::from_secs(10), |_| Ok(()))
        .await?;
    match audio_result {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(_) => return Err(Error::Transport("audio producer failed".into())),
    }
    probe?;
    if acks.len() != 3 || voiced_samples < 2400 || !transcript.contains("The test is ready") {
        return Err(Error::Invalid(
            "smoke requires all context ACKs, voiced audio and the expected transcript".into(),
        ));
    }
    let ServerEvent::Closed { usage, reason, .. } = final_frame.event else {
        return Err(Error::UnconfirmedClose);
    };
    println!(
        "{{\"probe\":\"public-live-primary\",\"acks\":{},\"audio_chunks\":{audio_chunks},\"voiced_samples\":{voiced_samples},\"transcript_matched\":true,\"final_seconds\":{},\"close_reason\":\"{reason:?}\"}}",
        acks.len(),
        usage.seconds
    );
    Ok(())
}
