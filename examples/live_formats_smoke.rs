//! Explicit, bounded, billable qualification of every non-default primary audio format.
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
                "This is a synthetic codec test. Speak briefly when given commentary.".into(),
            ),
            ..SessionConfig::default()
        })
        .await?;
    let sender = connection.sender();
    let byte_count = usize::try_from(format.sample_rate())
        .map_err(|_| Error::Invalid("unsupported platform sample rate".into()))?
        * format.bytes_per_sample()
        * 3;
    let samples_per_chunk = usize::try_from(format.sample_rate() / 10)
        .map_err(|_| Error::Invalid("unsupported platform sample rate".into()))?;
    let audio = tokio::spawn(async move {
        sender
            .send_audio_paced(&vec![silence_byte; byte_count], samples_per_chunk)
            .await
    });
    let mut acknowledged = false;
    let mut negotiated = false;
    let mut output_bytes = 0;
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
            acknowledged |= frame.client_event_id.as_deref() == Some("codec-context");
            if let Some(chunk) = frame.audio(ConnectionRole::Primary, format)? {
                if chunk.format != format {
                    return Err(Error::Invalid("unexpected output format".into()));
                }
                output_bytes += chunk.bytes.len();
            }
            match frame.event {
                ServerEvent::Started { session, .. } => {
                    negotiated = session.audio.and_then(|audio| audio.format) == Some(format);
                }
                ServerEvent::Error { error, .. } => return Err(Error::Provider(error)),
                _ => {}
            }
            if acknowledged && negotiated && output_bytes > 0 {
                return Ok(());
            }
        }
    }
    .await;
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
    result?;
    let ServerEvent::Closed { usage, .. } = closed.event else {
        return Err(Error::UnconfirmedClose);
    };
    println!(
        "{}",
        serde_json::json!({
            "probe":"public-live-primary-format","format":format,"format_echoed":negotiated,
            "context_ack":acknowledged,"output_bytes":output_bytes,"final_seconds":usage.seconds,
            "acoustic_fidelity_asserted":false,
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
