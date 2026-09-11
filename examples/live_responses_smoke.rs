//! Bounded, billable managed function/result/continuation probe against the public API.
mod support;
use oai_rt_rs::live::{
    AudioFormat, ClientEvent, Command, ConnectionRole, DelegationConfig, DelegationUpdate, Error,
    Field, FunctionCall, FunctionCallTracker, LiveClient, LiveConnection, NamedToolChoice,
    ResponseAttribution, ResponseEvent, ResponseInputItem, ResponseKey, ResponseLifecycleKind,
    ResponsesConfig, ResponsesOptions, ResponsesUpdate, Result, ServerEvent, ServerFrame,
    SessionConfig, SessionUpdate, Tool, ToolChoice, ToolChoiceMode,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::{Instant, timeout};

fn config(diagnostic: bool) -> SessionConfig {
    SessionConfig {
        instructions: Field::Value("This is a synthetic integration test. Wait for the backend result NATIVE_BLUE_SEVEN, then say exactly: The blue test passed. Do not report success before the backend finishes.".into()),
        delegation: Field::Value(DelegationConfig::Responses {
            responses: ResponsesConfig {
                model: "gpt-5.5".into(),
                options: ResponsesOptions {
                    instructions: Field::Value("Call probe_echo with code NATIVE_BLUE_SEVEN. After receiving the function result, respond with exactly NATIVE_BLUE_SEVEN and no other text.".into()),
                    max_output_tokens: Field::Value(256),
                    parallel_tool_calls: Field::Value(diagnostic),
                    tool_choice: Some(if diagnostic {
                        ToolChoice::Named(NamedToolChoice::Function { name: "probe_echo".into() })
                    } else {
                        ToolChoice::Mode(ToolChoiceMode::Auto)
                    }),
                    tools: Some(vec![Tool::Function {
                        name: "probe_echo".into(),
                        description: Field::Value("Synthetic deterministic echo.".into()),
                        parameters: Field::Value(json!({
                            "type":"object","properties":{"code":{"type":"string"}},
                            "required":["code"],"additionalProperties":false
                        }).as_object().unwrap().clone()),
                        strict: Field::Value(true),
                    }]),
                    ..ResponsesOptions::default()
                },
            },
        }),
        ..SessionConfig::default()
    }
}

async fn send(connection: &LiveConnection, id: &str, command: Command) -> Result<()> {
    connection
        .send(ClientEvent {
            event_id: Field::Value(id.into()),
            command,
        })
        .await
}

fn update(choice: ToolChoiceMode) -> Command {
    Command::Update {
        session: SessionUpdate {
            delegation: Field::Value(DelegationUpdate::Responses {
                responses: Some(ResponsesUpdate {
                    options: ResponsesOptions {
                        tool_choice: Some(ToolChoice::Mode(choice)),
                        ..ResponsesOptions::default()
                    },
                    ..ResponsesUpdate::default()
                }),
            }),
        },
    }
}

async fn submit_results(
    connection: &LiveConnection,
    calls: &[FunctionCall],
    diagnostic: bool,
) -> Result<()> {
    let [call] = calls else {
        return Err(Error::Invalid(
            "probe expected one complete function call".into(),
        ));
    };
    if call.name != "probe_echo"
        || serde_json::from_str::<Value>(&call.args)? != json!({"code":"NATIVE_BLUE_SEVEN"})
    {
        return Err(Error::Invalid(
            "unexpected synthetic function or arguments".into(),
        ));
    }
    send(
        connection,
        "tool-output",
        Command::ResponseItemCreate {
            item: ResponseInputItem::function_call_output(
                &call.call_id,
                "{\"code\":\"NATIVE_BLUE_SEVEN\"}",
            ),
        },
    )
    .await?;
    if diagnostic {
        send(connection, "backend-update", update(ToolChoiceMode::None)).await
    } else {
        send(connection, "backend-continue", Command::ResponseCreate).await
    }
}

async fn update_before_work(connection: &mut LiveConnection) -> Result<()> {
    send(connection, "backend-update", update(ToolChoiceMode::Auto)).await?;
    timeout(Duration::from_secs(10), async {
        loop {
            let frame = connection
                .next_event()
                .await?
                .ok_or(Error::UnconfirmedClose)?;
            support::check_frame(&frame)?;
            match frame.event {
                ServerEvent::Updated { .. }
                    if frame.client_event_id.as_deref() == Some("backend-update") =>
                {
                    return Ok(());
                }
                ServerEvent::Error { error, .. } => return Err(Error::Provider(error)),
                _ => {}
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

#[derive(Default)]
struct Probe {
    tracker: FunctionCallTracker,
    first: Option<ResponseKey>,
    continuation: Option<ResponseKey>,
    results_submitted: bool,
    continuation_sent: bool,
    update_acked: bool,
    backend_text: String,
    voice_text: String,
    speech: support::Speech,
    timeline: Vec<Value>,
}

impl Probe {
    fn record(&mut self, fact: Value) -> Result<()> {
        if self.timeline.len() >= 128 {
            return Err(Error::Invalid("probe timeline capacity exceeded".into()));
        }
        self.timeline.push(fact);
        Ok(())
    }

    fn created(&mut self, key: &ResponseKey) -> Result<()> {
        if key.delegation_id.is_none() {
            return Err(Error::Invalid("unattributed backend lifecycle".into()));
        }
        if self.first.as_ref() == Some(key) || self.continuation.as_ref() == Some(key) {
            return Ok(());
        }
        if self.first.is_none() && !self.continuation_sent {
            self.first = Some(key.clone());
        } else if self.continuation_sent
            && self.continuation.is_none()
            && self
                .first
                .as_ref()
                .is_some_and(|first| first.delegation_id == key.delegation_id)
        {
            self.continuation = Some(key.clone());
        } else {
            return Err(Error::Invalid(
                "unexpected overlapping or independent backend response".into(),
            ));
        }
        Ok(())
    }

    fn observe_response(
        &mut self,
        scope: Option<&str>,
        event: &ResponseEvent,
    ) -> Result<Option<Vec<FunctionCall>>> {
        let attribution = self.tracker.observe(scope, event).map_err(Error::Invalid)?;
        let key = match attribution {
            ResponseAttribution::Owned(key) => key,
            ResponseAttribution::Unowned if matches!(event, ResponseEvent::Unknown { .. }) => {
                return Ok(None);
            }
            ResponseAttribution::Unowned => {
                return Err(Error::Invalid("unowned backend event".into()));
            }
            ResponseAttribution::Ambiguous(_) => {
                return Err(Error::Invalid("ambiguous backend event ownership".into()));
            }
        };
        if let ResponseEvent::Lifecycle { kind, response, .. } = event {
            if *kind == ResponseLifecycleKind::Created {
                self.created(&key)?;
            }
            self.record(json!({"lifecycle":format!("{kind:?}"),
                "response":if self.first.as_ref() == Some(&key) { 1 } else { 2 }}))?;
            if !response.output.is_empty() {
                return Err(Error::Invalid(
                    "expected cleared Live backend lifecycle output".into(),
                ));
            }
            if matches!(
                kind,
                ResponseLifecycleKind::Failed | ResponseLifecycleKind::Incomplete
            ) {
                return Err(Error::Invalid(
                    "backend did not complete successfully".into(),
                ));
            }
            if *kind == ResponseLifecycleKind::Completed
                && self.first.as_ref() == Some(&key)
                && !self.results_submitted
            {
                let calls = self.tracker.ready_calls(&key).ok_or_else(|| {
                    Error::Invalid("function response ended with an uncertain call set".into())
                })?;
                return Ok(Some(calls.to_vec()));
            }
        }
        if event.completed_function_call().is_some() {
            if self.first.as_ref() != Some(&key) {
                return Err(Error::Invalid(
                    "unexpected function in continuation response".into(),
                ));
            }
            self.record(json!({"finished_function_item":true,"owning_response_complete":false}))?;
        }
        if self.continuation.as_ref() == Some(&key) {
            match event {
                ResponseEvent::OutputTextDelta { delta, .. } => {
                    append_bounded(&mut self.backend_text, delta)?;
                }
                ResponseEvent::OutputTextDone { text, .. } => {
                    self.backend_text.clear();
                    append_bounded(&mut self.backend_text, text)?;
                }
                _ => {}
            }
        }
        Ok(None)
    }

    fn successful(&self) -> bool {
        self.results_submitted
            && self.update_acked
            && self.continuation.as_ref().is_some_and(|key| {
                self.tracker
                    .ready_calls(key)
                    .is_some_and(<[FunctionCall]>::is_empty)
            })
            && self.backend_text.contains("NATIVE_BLUE_SEVEN")
            && self.voice_text.contains("The blue test passed")
            && self.speech.qualified()
    }

    fn report(&self, diagnostic: bool) -> Value {
        json!({
            "probe":"public-live-managed-responses",
            "complete_function_calls":self.first.as_ref().and_then(|key| self.tracker.calls(key)).map_or(0, <[FunctionCall]>::len),
            "first_response_complete":self.first.as_ref().is_some_and(|key| self.tracker.ready_calls(key).is_some()),
            "continuation_sent":self.continuation_sent,
            "continuation_created":self.continuation.is_some(),
            "continuation_terminal":self.continuation.as_ref().and_then(|key| self.tracker.terminal(key)).map(|kind| format!("{kind:?}")),
            "backend_result_matched":self.backend_text.contains("NATIVE_BLUE_SEVEN"),
            "voice_result_matched":self.voice_text.contains("The blue test passed"),
            "speech":self.speech.report(),"sparse_update_acked":self.update_acked,
            "update_during_handoff":diagnostic,"call_response_identity_verified":true,
            "cleared_snapshots_verified":true,"timeline":self.timeline,
        })
    }
}

fn append_bounded(target: &mut String, text: &str) -> Result<()> {
    if target.len().saturating_add(text.len()) > 65_536 {
        return Err(Error::Invalid("probe text capacity exceeded".into()));
    }
    target.push_str(text);
    Ok(())
}

async fn observe_frame(
    connection: &LiveConnection,
    probe: &mut Probe,
    frame: &ServerFrame,
    diagnostic: bool,
) -> Result<()> {
    if let Some(event) = frame.response_event()? {
        let ServerEvent::Response { delegation_id, .. } = &frame.event else {
            return Err(Error::Invalid("missing response envelope".into()));
        };
        if let Some(calls) =
            probe.observe_response(delegation_id.value().map(String::as_str), &event)?
        {
            submit_results(connection, &calls, diagnostic).await?;
            probe.results_submitted = true;
            probe.continuation_sent = !diagnostic;
            probe.record(json!({"results_submitted_after_same_response_completion":true}))?;
        }
    }
    if let Some(chunk) = frame.audio(ConnectionRole::Primary, AudioFormat::default())? {
        probe.speech.add(&chunk.bytes, chunk.format)?;
    }
    match &frame.event {
        ServerEvent::Updated { .. }
            if diagnostic && frame.client_event_id.as_deref() == Some("backend-update") =>
        {
            if !probe.results_submitted || probe.continuation_sent {
                return Err(Error::Invalid(
                    "unexpected handoff update acknowledgment".into(),
                ));
            }
            probe.update_acked = true;
            send(connection, "backend-continue", Command::ResponseCreate).await?;
            probe.continuation_sent = true;
            probe.record(json!({"update_acked_then_continue":true}))?;
        }
        ServerEvent::OutputTranscriptDelta { delta, .. } => {
            append_bounded(&mut probe.voice_text, delta)?;
        }
        _ => {}
    }
    Ok(())
}

fn latch_failure(probe: &mut Probe, first: &mut Option<Error>, error: Error) -> Result<()> {
    if let Error::Provider(provider) = &error {
        probe.record(
            json!({"provider_error_code":provider.code,"correlation":provider.client_event_id}),
        )?;
    } else {
        probe.record(json!({"unexpected_error":error.to_string()}))?;
    }
    if first.is_none() {
        *first = Some(error);
    }
    Ok(())
}

async fn exercise(connection: &mut LiveConnection, diagnostic: bool) -> Result<Value> {
    if !diagnostic {
        update_before_work(connection).await?;
    }
    let user = serde_json::from_value(json!({"type":"message","role":"user",
        "content":[{"type":"input_text","text":"Run the synthetic echo test now."}]}))?;
    send(
        connection,
        "backend-user",
        Command::ResponseItemCreate { item: user },
    )
    .await?;
    send(connection, "backend-start", Command::ResponseCreate).await?;
    let mut probe = Probe {
        update_acked: !diagnostic,
        ..Probe::default()
    };
    let mut unexpected = None;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let Ok(result) = timeout(
            deadline.saturating_duration_since(Instant::now()),
            connection.next_event(),
        )
        .await
        else {
            eprintln!("{}", probe.report(diagnostic));
            return Err(unexpected.unwrap_or(Error::Timeout));
        };
        let frame = match result {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                eprintln!("{}", probe.report(diagnostic));
                return Err(unexpected.unwrap_or(Error::UnconfirmedClose));
            }
            Err(error) => {
                latch_failure(&mut probe, &mut unexpected, error)?;
                continue;
            }
        };
        if let Err(error) = support::check_frame(&frame) {
            latch_failure(&mut probe, &mut unexpected, error)?;
            continue;
        }
        if let Err(error) = observe_frame(connection, &mut probe, &frame, diagnostic).await {
            eprintln!("{}", probe.report(diagnostic));
            return Err(unexpected.unwrap_or(error));
        }
        if probe.successful() {
            let report = probe.report(diagnostic);
            if let Some(error) = unexpected {
                eprintln!("{report}");
                return Err(error);
            }
            return Ok(report);
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        Error::Invalid("OPENAI_API_KEY is required; this probe does not skip".into())
    })?;
    let client = LiveClient::new(&key)?;
    let diagnostic = std::env::args().any(|arg| arg == "--update-during-handoff");
    let mut connection = client.connect(config(diagnostic)).await?;
    let sender = connection.sender();
    let chunk_samples = if std::env::args().any(|arg| arg == "--100ms-audio") {
        2400
    } else {
        480
    };
    let audio = tokio::spawn(async move {
        sender
            .send_audio_paced(&vec![0; 24000 * 2 * 25], chunk_samples)
            .await
    });
    let result = exercise(&mut connection, diagnostic).await;
    if let Err(error) = &result {
        eprintln!("probe stage failed: {error}");
    }
    audio.abort();
    let audio_result = audio.await;
    let close_result = support::close(&mut connection).await;
    if let Err(error) = &close_result {
        eprintln!("probe finalization failed: {error}");
    }
    match audio_result {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(_) => return Err(Error::Transport("audio producer failed".into())),
    }
    let mut report = result?;
    let ServerEvent::Closed { usage, .. } = close_result?.event else {
        return Err(Error::UnconfirmedClose);
    };
    report["final_seconds"] = json!(usage.seconds);
    println!("{report}");
    Ok(())
}

#[cfg(test)]
mod exercise_tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use futures::{SinkExt, StreamExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};

    type Peer = WebSocketStream<TcpStream>;

    async fn emit(peer: &mut Peer, frame: Value) {
        peer.send(Message::Text(frame.to_string().into()))
            .await
            .unwrap();
    }

    async fn receive(peer: &mut Peer) {
        timeout(Duration::from_secs(2), peer.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    fn lifecycle(kind: &str, id: &str) -> Value {
        json!({"type":format!("response.{kind}"),"sequence_number":0,"response":{
            "id":id,"created_at":1,"status":if kind=="completed" {"completed"} else {"in_progress"},"output":[]
        }})
    }

    async fn response(peer: &mut Peer, event: Value) {
        emit(
            peer,
            json!({"type":"response.event","event_id":"e","delegation_id":"d","event":event}),
        )
        .await;
    }

    async fn mock_backend(mut peer: Peer, injected: u8) {
        let session = json!({"id":"s","model":"gpt-live-1","status":"active","expires_at":1});
        receive(&mut peer).await;
        emit(
            &mut peer,
            json!({"type":"session.started","event_id":"start","session":session}),
        )
        .await;
        receive(&mut peer).await;
        emit(
            &mut peer,
            json!({"type":"session.updated","event_id":"update",
            "client_event_id":"backend-update","session":session}),
        )
        .await;
        receive(&mut peer).await;
        receive(&mut peer).await;
        response(&mut peer, lifecycle("created", "r1")).await;
        response(
            &mut peer,
            json!({"type":"response.output_item.done","sequence_number":1,"output_index":0,
            "item":{"type":"function_call","id":"fc","call_id":"call","name":"probe_echo",
                "arguments":"{\"code\":\"NATIVE_BLUE_SEVEN\"}","status":"completed"}}),
        )
        .await;
        response(&mut peer, lifecycle("completed", "r1")).await;
        receive(&mut peer).await;
        receive(&mut peer).await;
        let error = match injected {
            1 => Some(
                json!({"type":"transport.failed","event_id":"failed","session_id":"s",
                "error":{"type":"call_error","code":"failed","message":"synthetic"}}),
            ),
            2 => Some(json!({"type":"error","event_id":"failed",
                "error":{"type":"invalid_request_error","code":null,"message":"synthetic"}})),
            3 => Some(json!({"type":"session.output_audio.delta","delta":123})),
            4 => Some(
                json!({"type":"response.event","event_id":"bad","event":{"type":"response.completed"}}),
            ),
            _ => None,
        };
        if let Some(error) = error {
            emit(&mut peer, error).await;
        }
        response(&mut peer, lifecycle("created", "r2")).await;
        response(&mut peer, json!({"type":"response.output_text.delta","sequence_number":1,
            "item_id":"message","output_index":0,"content_index":0,"logprobs":[],"delta":"NATIVE_BLUE_SEVEN"})).await;
        response(&mut peer, json!({"type":"response.output_item.done","sequence_number":2,"output_index":0,
            "item":{"type":"message","id":"message","role":"assistant","content":[],"status":"completed"}})).await;
        response(&mut peer, lifecycle("completed", "r2")).await;
        emit(
            &mut peer,
            json!({"type":"session.output_transcript.delta","event_id":"voice",
            "delta":"The blue test passed.","start_ms":0,"end_ms":200}),
        )
        .await;
        let pcm: Vec<_> = (0..4800).flat_map(|_| 1000_i16.to_le_bytes()).collect();
        emit(
            &mut peer,
            json!({"type":"session.output_audio.delta","delta":STANDARD.encode(pcm)}),
        )
        .await;
        emit(
            &mut peer,
            json!({"type":"info","event_id":"after","code":"after-evidence","message":"synthetic"}),
        )
        .await;
        receive(&mut peer).await;
        emit(
            &mut peer,
            json!({"type":"session.closed","event_id":"closed","session":session,
            "reason":"close_requested","usage":{"seconds":1}}),
        )
        .await;
    }

    #[tokio::test]
    async fn exercise_latches_failures_but_consumes_later_success_and_final_usage() {
        for injected in 0..5 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = LiveClient::with_options(
                "synthetic",
                oai_rt_rs::live::ClientOptions {
                    base_url: format!("http://{}/v1/", listener.local_addr().unwrap())
                        .parse()
                        .unwrap(),
                    request_timeout: Duration::from_secs(2),
                    ..oai_rt_rs::live::ClientOptions::default()
                },
            )
            .unwrap();
            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                mock_backend(accept_async(tcp).await.unwrap(), injected).await;
            });
            let mut connection = client.connect(config(false)).await.unwrap();
            let result = timeout(Duration::from_secs(2), exercise(&mut connection, false))
                .await
                .unwrap();
            match injected {
                0 => {
                    let report = result.unwrap();
                    assert_eq!(report["continuation_terminal"], "Completed");
                    assert_eq!(report["voice_result_matched"], true);
                }
                1 => assert!(matches!(result, Err(Error::Transport(_)))),
                2 => assert!(matches!(result, Err(Error::Provider(_)))),
                _ => assert!(matches!(result, Err(Error::MalformedEvent { .. }))),
            }
            let marker = connection.next_event().await.unwrap().unwrap();
            assert!(
                matches!(marker.event, ServerEvent::Info {code,..} if code=="after-evidence"),
                "exercise must keep observing after failure, not return before later lifecycle/audio evidence"
            );
            support::close(&mut connection).await.unwrap();
            server.await.unwrap();
        }
    }
}
