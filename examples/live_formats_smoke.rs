//! Explicit, bounded, billable qualification of every non-default primary audio format.
mod support;
use oai_rt_rs::live::{
    AudioConfig, AudioFormat, ClientEvent, Command, ConnectionRole, Error, Field, LiveClient,
    Nullable, Result, ServerEvent, SessionConfig,
};
use std::time::Duration;
use tokio::time::{Instant, timeout};

async fn probe(client: &LiveClient, format: AudioFormat, silence_byte: u8) -> Result<()> {
    let mut connection = client
        .connect(SessionConfig {
            audio: Some(AudioConfig {
                format: Some(format),
                output: None,
            }),
            instructions: Field::Value(
                "This is a synthetic codec test. When given commentary, say exactly: The codec test is ready.".into(),
            ),
            ..SessionConfig::default()
        })
        .await?;
    let sender = connection.sender();
    let byte_count = usize::try_from(format.sample_rate())
        .map_err(|_| Error::Invalid("unsupported platform sample rate".into()))?
        * format.bytes_per_sample()
        * 6;
    let samples_per_chunk = usize::try_from(format.sample_rate() / 10)
        .map_err(|_| Error::Invalid("unsupported platform sample rate".into()))?;
    let audio = tokio::spawn(async move {
        sender
            .send_audio_paced(&vec![silence_byte; byte_count], samples_per_chunk)
            .await
    });
    let mut acks = support::Acks::new(&[("session.commentary.appended", "codec-context")]);
    let mut negotiated = false;
    let mut output_bytes = 0;
    let mut speech = support::Speech::default();
    let mut transcript = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    let result = async {
        connection
            .send(ClientEvent {
                event_id: Field::Value("codec-context".into()),
                command: Command::CommentaryAppend {
                    content: "The codec test is ready.".into(),
                    delegation_id: Nullable(None),
                },
            })
            .await?;
        loop {
            let frame = timeout(
                deadline.saturating_duration_since(Instant::now()),
                connection.next_event(),
            )
            .await
            .map_err(|_| Error::Timeout)??
            .ok_or(Error::UnconfirmedClose)?;
            support::check_frame(&frame)?;
            acks.observe(&frame);
            if let Some(chunk) = frame.audio(ConnectionRole::Primary, format)? {
                if chunk.format != format {
                    return Err(Error::Invalid("unexpected output format".into()));
                }
                output_bytes += chunk.bytes.len();
                speech.add(&chunk.bytes, format)?;
            }
            match frame.event {
                ServerEvent::Started { session, .. } => {
                    negotiated = session.audio.and_then(|audio| audio.format) == Some(format);
                }
                ServerEvent::Error { error, .. } => return Err(Error::Provider(error)),
                ServerEvent::OutputTranscriptDelta { delta, .. } => {
                    support::append_text(&mut transcript, &delta)?;
                }
                _ => {}
            }
            if acks.complete()
                && negotiated
                && speech.qualified()
                && transcript.contains("The codec test is ready")
            {
                return Ok(());
            }
        }
    }
    .await;
    audio.abort();
    let audio_result = audio.await;
    let closed = support::close(&mut connection).await?;
    match audio_result {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(_) => return Err(Error::Transport("audio producer failed".into())),
    }
    result?;
    let ServerEvent::Closed { usage, .. } = closed.event else {
        return Err(Error::UnconfirmedClose);
    };
    println!(
        "{}",
        serde_json::json!({
            "probe":"public-live-primary-format","format":format,"format_echoed":negotiated,
            "context_ack":acks.complete(),"output_bytes":output_bytes,"final_seconds":usage.seconds,
            "decoded_speech":speech.report(),"expected_transcript":true,
        })
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        Error::Invalid("OPENAI_API_KEY is required; this probe does not skip".into())
    })?;
    let client = LiveClient::new(&key)?;
    for (format, silence_byte) in [
        (AudioFormat::Pcm { rate: 16000 }, 0),
        (AudioFormat::Pcmu { rate: 8000 }, 0xff),
        (AudioFormat::Pcma { rate: 8000 }, 0xd5),
    ] {
        probe(&client, format, silence_byte).await?;
    }
    Ok(())
}
