use oai_rt_rs::protocol::models::{
    AudioConfig, AudioFormat, GPT_LIVE_TRANSCRIBE, GPT_REALTIME_2_1, GPT_REALTIME_TRANSLATE,
    GPT_REALTIME_WHISPER, InputAudioConfig, InputAudioTranscription, Nullable, OutputAudioConfig,
    OutputModalities, SessionConfig, SessionKind,
};
use oai_rt_rs::transport::rest::{ExpiresAfter, RealtimeRestAdapter};
use oai_rt_rs::{Error, Realtime, RealtimeClient, Result, SdkEvent, ServerEvent};

fn api_key() -> std::io::Result<String> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| std::io::Error::other("OPENAI_API_KEY is required for opted-in live tests"))
}

fn short_expiry() -> ExpiresAfter {
    ExpiresAfter {
        anchor: "created_at".to_string(),
        seconds: 600,
    }
}

fn realtime_config() -> SessionConfig {
    let mut config = SessionConfig::new(
        SessionKind::Realtime,
        GPT_REALTIME_2_1,
        OutputModalities::Audio,
    );
    config.audio = Some(AudioConfig {
        input: Some(InputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            turn_detection: None,
            transcription: None,
            noise_reduction: None,
        }),
        output: Some(OutputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            voice: None,
            speed: None,
            language: None,
        }),
    });
    config
}

fn translation_config() -> SessionConfig {
    let mut config = SessionConfig::new(
        SessionKind::Translation,
        GPT_REALTIME_TRANSLATE,
        OutputModalities::Audio,
    );
    config.audio = Some(AudioConfig {
        input: Some(InputAudioConfig {
            format: None,
            turn_detection: None,
            transcription: Some(Nullable::Value(InputAudioTranscription {
                model: Some(GPT_REALTIME_WHISPER.to_string()),
                ..InputAudioTranscription::default()
            })),
            noise_reduction: None,
        }),
        output: Some(OutputAudioConfig {
            format: None,
            voice: None,
            speed: None,
            language: Some("es".to_string()),
        }),
    });
    config
}

fn transcription_config() -> SessionConfig {
    let mut config = SessionConfig::new(
        SessionKind::Transcription,
        GPT_LIVE_TRANSCRIBE,
        OutputModalities::Audio,
    );
    config.audio = Some(AudioConfig {
        input: Some(InputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            turn_detection: Some(Nullable::Null),
            transcription: Some(Nullable::Value(InputAudioTranscription {
                model: Some(GPT_LIVE_TRANSCRIBE.to_string()),
                ..InputAudioTranscription::default()
            })),
            noise_reduction: None,
        }),
        output: None,
    });
    config
}

#[tokio::test]
#[ignore = "realtime_live"]
#[allow(clippy::result_large_err)]
async fn realtime_live_rest_endpoints_accept_current_session_shapes() -> Result<()> {
    let api_key = api_key()?;
    let rest = RealtimeRestAdapter::new(&api_key)?;

    rest.create_client_secret_with_expiry(realtime_config(), Some(short_expiry()))
        .await?;
    let transcription = rest
        .create_transcription_client_secret_with_expiry_and_safety_identifier(
            transcription_config(),
            Some(short_expiry()),
            None,
        )
        .await?;
    assert!(
        !transcription.session.id.is_empty(),
        "missing transcription session ID"
    );
    rest.create_translation_client_secret_with_expiry_and_safety_identifier(
        translation_config(),
        Some(short_expiry()),
        None,
    )
    .await?;

    Ok(())
}

#[tokio::test]
#[ignore = "realtime_live"]
#[allow(clippy::result_large_err)]
async fn realtime_live_websocket_handshakes_accept_current_targets() -> Result<()> {
    let api_key = api_key()?;

    let _voice = RealtimeClient::connect(&api_key, Some(GPT_REALTIME_2_1), None).await?;
    let _translation =
        RealtimeClient::connect_translation(&api_key, Some(GPT_REALTIME_TRANSLATE), None).await?;

    Ok(())
}

#[tokio::test]
#[ignore = "realtime_live"]
#[allow(clippy::result_large_err)]
async fn realtime_live_transcription_builder_receives_ga_update() -> Result<()> {
    let api_key = api_key()?;
    let mut session = Realtime::transcription_builder()
        .api_key(api_key)
        .transcription_model(GPT_LIVE_TRANSCRIBE)
        .manual_turn_detection()
        .connect_ws()
        .await?;
    let update = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = session.next_event().await? {
            match event {
                SdkEvent::Raw(raw) => {
                    if let ServerEvent::TranscriptionSessionUpdated { session, .. } = *raw {
                        return Ok(session);
                    }
                }
                SdkEvent::Error { error, .. } => return Err(Error::Api(error)),
                _ => {}
            }
        }
        Err(Error::ConnectionClosed)
    })
    .await
    .expect("timed out waiting for the GA transcription update")?;
    assert_eq!(
        update
            .audio
            .and_then(|audio| audio.input)
            .and_then(|input| input.transcription)
            .and_then(|settings| settings.as_ref()?.model.clone())
            .as_deref(),
        Some(GPT_LIVE_TRANSCRIBE)
    );
    Ok(())
}
