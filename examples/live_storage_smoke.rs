//! Bounded, billable stored-session/content probe; recording bytes stay in memory.
mod support;
use oai_rt_rs::live::{
    AudioFormat, ClientEvent, Command, ConnectionRole, Error, Field, ForkSessionConfig, LiveClient,
    LiveConnection, Nullable, Result, ServerEvent, SessionConfig,
};
use std::time::Duration;
use tokio::time::{Instant, timeout};

fn verify_stereo_wav(bytes: &[u8]) -> Result<usize> {
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(Error::Invalid("recording is not RIFF/WAVE".into()));
    }
    let mut position = 12;
    let mut format_valid = false;
    let mut frames = 0;
    let mut left_peak = 0;
    while let Some(header) = bytes.get(position..position + 8) {
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let payload = bytes
            .get(position + 8..position + 8 + size)
            .ok_or_else(|| Error::Invalid("truncated WAV chunk".into()))?;
        match &header[..4] {
            b"fmt " if size >= 16 => {
                format_valid = payload[..4] == [1, 0, 2, 0]
                    && payload[4..8] == 24000_u32.to_le_bytes()
                    && payload[12..16] == [4, 0, 16, 0];
            }
            b"data" => {
                if size % 4 != 0 {
                    return Err(Error::Invalid("partial stereo WAV frame".into()));
                }
                frames += size / 4;
                left_peak = payload
                    .chunks_exact(4)
                    .map(|frame| i16::from_le_bytes([frame[0], frame[1]]).unsigned_abs())
                    .max()
                    .unwrap_or(0)
                    .max(left_peak);
            }
            _ => {}
        }
        position += 8 + size + size % 2;
    }
    if !format_valid || frames == 0 || left_peak < 500 {
        return Err(Error::Invalid(
            "recording must be stereo PCM16/24k with synthetic input on left".into(),
        ));
    }
    Ok(frames)
}

async fn fork_voice(fork: &mut LiveConnection) -> Result<serde_json::Value> {
    let sender = fork.sender();
    let producer =
        tokio::spawn(async move { sender.send_audio_paced(&vec![0; 48_000 * 12], 480).await });
    let mut acks = support::Acks::new(&[("session.instructions.appended", "fork-context")]);
    let mut speech = support::Speech::default();
    let mut transcript = String::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    let result = async {
        fork.send(ClientEvent {
            event_id:Field::Value("fork-context".into()),
            command:Command::InstructionsAppend {
                content:"The recording phase is complete. Say exactly: The fork test is ready.".into(),
                delegation_id:Nullable(None),
            },
        }).await?;
        loop {
            let frame = timeout(deadline.saturating_duration_since(Instant::now()), fork.next_event())
                .await.map_err(|_| Error::Timeout)??.ok_or(Error::UnconfirmedClose)?;
            support::check_frame(&frame)?;
            acks.observe(&frame);
            if let Some(chunk) = frame.audio(ConnectionRole::Primary, AudioFormat::default())? {
                speech.add(&chunk.bytes, chunk.format)?;
            }
            if let ServerEvent::OutputTranscriptDelta { delta, .. } = frame.event {
                support::append_text(&mut transcript, &delta)?;
            }
            if acks.complete() && speech.qualified() && transcript.contains("The fork test is ready") {
                return Ok(serde_json::json!({"context_ack":true,"expected_transcript":true,"speech":speech.report()}));
            }
        }
    }.await;
    producer.abort();
    match producer.await {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(_) => return Err(Error::Transport("fork audio producer failed".into())),
    }
    result
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        Error::Invalid("OPENAI_API_KEY is required; this probe does not skip".into())
    })?;
    let client = LiveClient::new(&key)?;
    let mut connection = client
        .connect(SessionConfig {
            store: Some(true),
            instructions: Field::Value("This is a synthetic recording/fork test. Remain silent until explicitly asked to speak; when instructed, say exactly: The fork test is ready.".into()),
            ..SessionConfig::default()
        })
        .await?;
    let recorded = async {
        let session = support::started(&mut connection).await?;
        let pcm: Vec<u8> = (0..24000)
            .flat_map(|sample| [0_i16, 1000, 0, -1000][sample % 4].to_le_bytes())
            .collect();
        connection.sender().send_audio_paced(&pcm, 2400).await?;
        Ok::<_, Error>(session)
    }
    .await;
    let closed = support::close(&mut connection).await?;
    let session = recorded?;
    let ServerEvent::Closed { usage, .. } = closed.event else {
        return Err(Error::UnconfirmedClose);
    };
    let content = client.download_content(&session.id).await?;
    let wav = content.read_all(4 * 1024 * 1024).await?;
    let frames = verify_stereo_wav(&wav)?;
    let mut fork = client
        .fork(
            &session.id,
            ForkSessionConfig {
                store: Some(false),
                ..ForkSessionConfig::default()
            },
        )
        .await?;
    let result = async {
        let fork_session = support::started(&mut fork).await?;
        support::verify_identity(Some(&session.id), &fork_session.id, &fork_session.id)?;
        if fork_session.model != session.model
            || fork_session.audio.and_then(|audio| audio.format) != Some(AudioFormat::default())
        {
            return Err(Error::Invalid(
                "fork did not inherit model and default PCM24k".into(),
            ));
        }
        fork_voice(&mut fork).await
    }
    .await;
    let fork_closed = support::close(&mut fork).await?;
    let fork_media = result?;
    let ServerEvent::Closed {
        usage: fork_usage, ..
    } = fork_closed.event
    else {
        return Err(Error::UnconfirmedClose);
    };
    println!(
        "{}",
        serde_json::json!({
            "probe":"public-live-storage","wav_bytes":wav.len(),"stereo_frames":frames,
            "rate":24000,"bits":16,"input_left_verified":true,"final_seconds":usage.seconds,
            "ws_fork_new_id":true,"ws_fork_inherited_model":true,"ws_fork_pcm24k":true,
            "fork_final_seconds":fork_usage.seconds,
            "fork_media":fork_media,
        })
    );
    Ok(())
}
