use oai_rt_rs::protocol::models::{
    AudioConfig, AudioFormat, GPT_REALTIME_2, GPT_REALTIME_TRANSLATE, GPT_REALTIME_WHISPER,
    InputAudioConfig, InputAudioTranscription, Nullable, OutputAudioConfig, OutputModalities,
    SessionConfig, SessionKind,
};
use oai_rt_rs::transport::rest::{ExpiresAfter, RealtimeRestAdapter};
use oai_rt_rs::{RealtimeClient, Result};

fn api_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
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
        GPT_REALTIME_2,
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
                language: None,
                prompt: None,
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
        GPT_REALTIME_WHISPER,
        OutputModalities::Audio,
    );
    config.audio = Some(AudioConfig {
        input: Some(InputAudioConfig {
            format: Some(AudioFormat::pcm_24khz()),
            turn_detection: None,
            transcription: Some(Nullable::Value(InputAudioTranscription {
                model: Some("gpt-4o-transcribe".to_string()),
                language: Some("en".to_string()),
                prompt: None,
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
    let Some(api_key) = api_key() else {
        eprintln!("skipping live test: OPENAI_API_KEY is not set");
        return Ok(());
    };
    let rest = RealtimeRestAdapter::new(&api_key)?;

    rest.create_client_secret_with_expiry(realtime_config(), Some(short_expiry()))
        .await?;
    rest.create_session(SessionConfig::new(
        SessionKind::Realtime,
        "gpt-realtime",
        OutputModalities::AudioText,
    ))
    .await?;
    rest.create_transcription_session(transcription_config())
        .await?;
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
    let Some(api_key) = api_key() else {
        eprintln!("skipping live test: OPENAI_API_KEY is not set");
        return Ok(());
    };

    let _voice = RealtimeClient::connect(&api_key, Some(GPT_REALTIME_2), None).await?;
    let _transcription = RealtimeClient::connect_transcription(&api_key, None).await?;
    let _translation =
        RealtimeClient::connect_translation(&api_key, Some(GPT_REALTIME_TRANSLATE), None).await?;

    Ok(())
}
