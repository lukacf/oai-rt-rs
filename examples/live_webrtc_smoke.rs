//! Run through `scripts/live_webrtc_smoke.py`: Rust owns public signaling and sideband.
mod support;
use oai_rt_rs::live::{
    ClientConfig, ClientEvent, CloseReason, Command, ConnectionRole, CreateRequest,
    DataChannelConfig, Error, EventPermissions, Field, ForkRequest, ForkSessionConfig, LiveClient,
    LiveConnection, Nullable, Result, ServerEvent, SessionConfig, WebRtcTransport,
};
use serde_json::{Value, json};
use std::{io::BufRead, time::Duration};
use tokio::{
    sync::mpsc,
    time::{Instant, timeout},
};

fn input_lines() -> mpsc::Receiver<Result<Value>> {
    let (sender, receiver) = mpsc::channel(4);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let parsed = line
                .map_err(|_| Error::Transport("peer pipe".into()))
                .and_then(|line| serde_json::from_str(&line).map_err(Error::from));
            if sender.blocking_send(parsed).is_err() {
                break;
            }
        }
    });
    receiver
}

async fn peer_message(receiver: &mut mpsc::Receiver<Result<Value>>) -> Result<Value> {
    timeout(Duration::from_secs(35), receiver.recv())
        .await
        .map_err(|_| Error::Timeout)?
        .ok_or_else(|| Error::Transport("peer pipe closed".into()))?
}

async fn create_peer(
    client: &LiveClient,
    config: SessionConfig,
    sdp: String,
    fork: bool,
) -> Result<(oai_rt_rs::live::CreateResponse, Option<String>)> {
    let transport = WebRtcTransport::WebRtc { sdp };
    if !fork {
        return client
            .create_webrtc(&CreateRequest {
                session: config,
                transport,
            })
            .await
            .map(|created| (created, None));
    }
    let mut source = client
        .connect(SessionConfig {
            store: Some(true),
            client: None,
            ..config.clone()
        })
        .await?;
    let recorded = async {
        let session = support::started(&mut source).await?;
        source
            .sender()
            .send_audio_paced(&vec![0; 48000], 2400)
            .await?;
        Ok::<_, Error>(session)
    }
    .await;
    support::close(&mut source).await?;
    let session = recorded?;
    let created = client
        .fork_webrtc(
            &session.id,
            &ForkRequest {
                transport,
                session: Some(ForkSessionConfig {
                    store: Some(false),
                    client: config.client,
                    ..ForkSessionConfig::default()
                }),
            },
        )
        .await?;
    support::verify_identity(Some(&session.id), &created.session.id, &created.session.id)?;
    Ok((created, Some(session.id)))
}

async fn negotiate(
    client: &LiveClient,
    restricted: bool,
    fork: bool,
    input: &mut mpsc::Receiver<Result<Value>>,
) -> Result<(LiveConnection, bool)> {
    let offer = peer_message(input).await?;
    let sdp = offer
        .get("sdp")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Invalid("peer must supply an SDP offer".into()))?;
    let config = SessionConfig {
        instructions:Field::Value("This is a synthetic connection test. When commentary arrives, say exactly: The connection test is ready.".into()),
        client:restricted.then(|| ClientConfig {
            data_channel:DataChannelConfig {
                allowed_client_events:Some(EventPermissions::Selected(Vec::new())),
                allowed_server_events:None,
            },
        }),
        ..SessionConfig::default()
    };
    let (created, source) = create_peer(client, config, sdp.to_owned(), fork).await?;
    let mut connection = client.attach(&created.session.id).await?;
    let result = async {
        timeout(Duration::from_secs(10), async {
            loop {
                let frame = connection
                    .next_event()
                    .await?
                    .ok_or(Error::UnconfirmedClose)?;
                support::check_frame_for(&frame, restricted)?;
                if let ServerEvent::Started { session, .. } = frame.event {
                    return support::verify_identity(
                        source.as_deref(),
                        &created.session.id,
                        &session.id,
                    );
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)??;
        println!("{}", json!({"sdp":created.transport.sdp()}));
        let ready = peer_message(input).await?;
        if ready.get("ready") != Some(&Value::Bool(true)) {
            return Err(Error::Invalid("peer media was not made ready".into()));
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        support::close_for(&mut connection, restricted).await?;
        return Err(error);
    }
    let verified_new_id = source
        .as_deref()
        .is_some_and(|source| source != created.session.id);
    Ok((connection, verified_new_id))
}

async fn seed(connection: &LiveConnection) -> Result<()> {
    for (id, command) in [
        (
            "peer-thinking",
            Command::ThinkingAppend {
                content: "The synthetic connection test is ready.".into(),
                delegation_id: Nullable(None),
            },
        ),
        (
            "peer-commentary",
            Command::CommentaryAppend {
                content: "The connection test is ready.".into(),
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

async fn run(client: &LiveClient, restricted: bool, fork: bool) -> Result<()> {
    let mut input = input_lines();
    let (mut connection, verified_new_id) = negotiate(client, restricted, fork, &mut input).await?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut acks = support::Acks::new(&[
        ("session.thinking.appended", "peer-thinking"),
        ("session.commentary.appended", "peer-commentary"),
    ]);
    let mut media_verified = false;
    let mut reflected_input_bytes = 0;
    let mut reflected_output_deltas = 0;
    let result = async {
        seed(&connection).await?;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => return Err(Error::Timeout),
                message = input.recv(), if !media_verified => {
                    let message = message.ok_or_else(|| Error::Transport("peer pipe closed".into()))??;
                    media_verified = message.get("media_verified") == Some(&Value::Bool(true));
                    if !media_verified { return Err(Error::Invalid("peer did not verify media".into())); }
                }
                frame = connection.next_event() => {
                    let frame = frame?.ok_or(Error::UnconfirmedClose)?;
                    support::check_frame_for(&frame, restricted)?;
                    acks.observe(&frame);
                    if let Some(chunk) = frame.audio(ConnectionRole::Sideband,oai_rt_rs::live::AudioFormat::default())? {
                        match chunk.source {
                            oai_rt_rs::live::AudioSource::ReflectedInput => reflected_input_bytes += chunk.bytes.len(),
                            oai_rt_rs::live::AudioSource::Output => reflected_output_deltas += 1,
                        }
                    }
                }
            }
            if media_verified && acks.complete()
                && reflected_input_bytes > 0 && reflected_output_deltas > 0
            { return Ok(()); }
        }
    }.await;
    let closed = support::close_for(&mut connection, restricted).await?;
    result?;
    let ServerEvent::Closed {
        reason: CloseReason::CloseRequested,
        usage,
        ..
    } = closed.event
    else {
        return Err(Error::UnconfirmedClose);
    };
    println!(
        "{}",
        json!({
            "probe":"public-live-rust-webrtc","sideband_acks":acks.count(),"closed":true,
            "final_seconds":usage.seconds,"reflected_input_bytes":reflected_input_bytes,
            "reflected_output_deltas":reflected_output_deltas,"restricted_browser":restricted,
            "http_fork":fork,"fork_new_id_verified":verified_new_id,"created_matches_attached":true,
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
    run(
        &client,
        std::env::args().any(|arg| arg == "--restrict-browser"),
        std::env::args().any(|arg| arg == "--fork"),
    )
    .await
}
