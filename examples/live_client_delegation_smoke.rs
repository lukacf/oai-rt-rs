//! Bounded, billable client-delegation probe using a synthetic raw PCM16/24k file.
use oai_rt_rs::live::{
    ClientEvent, CloseReason, Command, Error, Field, LiveClient, Nullable, Result, ServerEvent,
    SessionConfig,
};
use std::{collections::HashSet, time::Duration};
use tokio::time::{Instant, timeout};

fn synthetic_audio() -> Result<Vec<u8>> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| Error::Invalid("supply a synthetic raw PCM16LE/24k file".into()))?;
    let mut pcm = std::fs::read(path)
        .map_err(|_| Error::Invalid("could not read synthetic sample".into()))?;
    if pcm.len() > 24_000 * 2 * 12 {
        return Err(Error::Invalid(
            "probe input must be at most 12 seconds".into(),
        ));
    }
    pcm.extend(vec![0; 24_000 * 2 * 6]);
    Ok(pcm)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        Error::Invalid("OPENAI_API_KEY is required; this probe does not skip".into())
    })?;
    let pcm = synthetic_audio()?;
    let client = LiveClient::new(&key)?;
    let mut connection = client.connect(SessionConfig {
        instructions: Field::Value(
            "This is a synthetic order lookup test. When asked to look up any order, delegate to the client. Do not invent the result. When the client returns the result, say exactly: The test order is ready for pickup.".into(),
        ),
        ..SessionConfig::default()
    }).await?;
    let sender = connection.sender();
    let audio = tokio::spawn(async move { sender.send_audio_paced(&pcm, 2400).await });
    let mut delegations = 0;
    let mut acked = HashSet::new();
    let mut input_fragments = 0;
    let mut transcript = String::new();
    let mut voiced_samples = 0;
    let deadline = Instant::now() + Duration::from_secs(22);
    let probe = async {
        loop {
            let frame = timeout(deadline.saturating_duration_since(Instant::now()), connection.next_event())
                .await.map_err(|_| Error::Timeout)??.ok_or(Error::UnconfirmedClose)?;
            if let Some(id) = &frame.client_event_id {
                acked.insert(id.clone());
            }
            if let Some(chunk) = frame.audio(oai_rt_rs::live::ConnectionRole::Primary, oai_rt_rs::live::AudioFormat::default())? {
                if chunk.bytes.chunks_exact(2)
                    .any(|b| i16::from_le_bytes([b[0],b[1]]).unsigned_abs() >= 500)
                {
                    voiced_samples += chunk.bytes.len() / 2;
                }
            }
            match frame.event {
                ServerEvent::InputTranscriptDelta { .. } => input_fragments += 1,
                ServerEvent::OutputTranscriptDelta { delta, .. } => transcript.push_str(&delta),
                ServerEvent::DelegationCreated { delegation, .. } => {
                    if delegation.target != oai_rt_rs::live::DelegationTarget::Client
                        || delegation.response_id.is_some()
                    {
                        return Err(Error::Invalid("unexpected client delegation metadata".into()));
                    }
                    delegations += 1;
                    for (event_id, command) in [
                        ("result-thinking", Command::ThinkingAppend {
                            content:"Verified synthetic lookup result: the test order is ready for pickup.".into(),
                            delegation_id:Nullable(Some(delegation.id.clone())),
                        }),
                        ("result-commentary", Command::CommentaryAppend {
                            content:"The test order is ready for pickup.".into(),
                            delegation_id:Nullable(Some(delegation.id.clone())),
                        }),
                    ] {
                        connection.send(ClientEvent {event_id:Field::Value(event_id.into()),command}).await?;
                    }
                }
                ServerEvent::Error { error, .. } => return Err(Error::Provider(error)),
                _ => {}
            }
            if delegations > 0 && input_fragments > 0
                && acked.contains("result-thinking") && acked.contains("result-commentary")
                && transcript.contains("The test order is ready for pickup") && voiced_samples >= 4800
            {
                return Ok(());
            }
        }
    }.await;
    audio.abort();
    let audio_result = audio.await;
    let closed = connection
        .close(Duration::from_secs(10), |_| Ok(()))
        .await?;
    match audio_result {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(_) => return Err(Error::Transport("audio producer failed".into())),
    }
    probe?;
    let ServerEvent::Closed {
        reason: CloseReason::CloseRequested,
        usage,
        ..
    } = closed.event
    else {
        return Err(Error::UnconfirmedClose);
    };
    println!(
        "{{\"probe\":\"public-live-client-delegation\",\"delegations\":{delegations},\"input_fragments\":{input_fragments},\"acks\":{},\"voiced_samples\":{voiced_samples},\"result_transcript_matched\":true,\"final_seconds\":{}}}",
        acked.len(),
        usage.seconds
    );
    Ok(())
}
