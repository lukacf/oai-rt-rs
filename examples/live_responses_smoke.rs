//! Bounded, billable managed function/result/continuation probe against the public API.
use oai_rt_rs::live::{
    ClientEvent, Command, DelegationConfig, DelegationUpdate, Error, Field, FunctionCall,
    LiveClient, LiveConnection, NamedToolChoice, ResponseInputItem, ResponsesConfig,
    ResponsesOptions, ResponsesUpdate, Result, ServerEvent, SessionConfig, SessionUpdate, Tool,
    ToolChoice, ToolChoiceMode,
};
use serde_json::json;
use std::{collections::HashMap,time::Duration};
use tokio::time::{Instant, timeout};

fn config(update_during_handoff: bool) -> SessionConfig {
    SessionConfig {
        instructions:Field::Value("This is a synthetic integration test. Wait for the backend result NATIVE_BLUE_SEVEN, then say exactly: The blue test passed. Do not report success before the backend finishes.".into()),
        delegation:Field::Value(DelegationConfig::Responses {responses:ResponsesConfig {
            model:"gpt-5.5".into(),
            options:ResponsesOptions {
                instructions:Field::Value("Call probe_echo with code NATIVE_BLUE_SEVEN. After receiving the function result, respond with exactly NATIVE_BLUE_SEVEN and no other text.".into()),
                max_output_tokens:Field::Value(256),
                parallel_tool_calls:Field::Value(true),
                tool_choice:Some(if update_during_handoff {
                    ToolChoice::Named(NamedToolChoice::Function {name:"probe_echo".into()})
                } else { ToolChoice::Mode(ToolChoiceMode::Auto) }),
                tools:Some(vec![Tool::Function {
                    name:"probe_echo".into(),description:Field::Value("Synthetic deterministic echo.".into()),
                    parameters:Field::Value(json!({
                        "type":"object","properties":{"code":{"type":"string"}},
                        "required":["code"],"additionalProperties":false
                    }).as_object().unwrap().clone()),
                    strict:Field::Value(true),
                }]),
                ..ResponsesOptions::default()
            },
        }}),
        ..SessionConfig::default()
    }
}

async fn submit_results(connection: &LiveConnection, calls: &[FunctionCall], update_during_handoff: bool) -> Result<()> {
    if calls.len() != 1 {
        return Err(Error::Invalid(
            "probe expected one complete function call".into(),
        ));
    }
    for call in calls {
        if call.name != "probe_echo"
            || serde_json::from_str::<serde_json::Value>(&call.args)?
                != json!({"code":"NATIVE_BLUE_SEVEN"})
        {
            return Err(Error::Invalid(
                "unexpected synthetic function or arguments".into(),
            ));
        }
        let item: ResponseInputItem = serde_json::from_value(json!({
            "type":"function_call_output","call_id":call.call_id,
            "output":"{\"code\":\"NATIVE_BLUE_SEVEN\"}"
        }))?;
        connection
            .send(ClientEvent {
                event_id:Field::Value("tool-output".into()),
                command:Command::ResponseItemCreate {item},
            })
            .await?;
    }
    if !update_during_handoff {
        return connection.send(ClientEvent {
            event_id:Field::Value("backend-continue".into()),command:Command::ResponseCreate,
        }).await;
    }
    connection
        .send(ClientEvent {
            event_id: Field::Value("backend-update".into()),
            command: Command::Update {
                session: SessionUpdate {
                    delegation: Field::Value(DelegationUpdate::Responses {
                        responses: Some(ResponsesUpdate {
                            options: ResponsesOptions {
                                tool_choice: Some(ToolChoice::Mode(ToolChoiceMode::None)),
                                ..ResponsesOptions::default()
                            },
                            ..ResponsesUpdate::default()
                        }),
                    }),
                },
            },
        })
        .await
}

async fn update_before_work(connection: &mut LiveConnection) -> Result<()> {
    connection.send(ClientEvent {
        event_id:Field::Value("backend-update".into()),
        command:Command::Update {session:SessionUpdate {
            delegation:Field::Value(DelegationUpdate::Responses {responses:Some(ResponsesUpdate {
                options:ResponsesOptions {tool_choice:Some(ToolChoice::Mode(ToolChoiceMode::Auto)),..ResponsesOptions::default()},
                ..ResponsesUpdate::default()
            })}),
        }},
    }).await?;
    timeout(Duration::from_secs(10),async {
        loop {
            let frame = connection.next_event().await?.ok_or(Error::UnconfirmedClose)?;
            match frame.event {
                ServerEvent::Updated { .. } if frame.client_event_id.as_deref() == Some("backend-update") => return Ok(()),
                ServerEvent::Error {error,..} => return Err(Error::Provider(error)),
                _ => {}
            }
        }
    }).await.map_err(|_| Error::Timeout)?
}

async fn exercise(connection: &mut LiveConnection, update_during_handoff: bool) -> Result<serde_json::Value> {
    if !update_during_handoff { update_before_work(connection).await?; }
    let user: ResponseInputItem = serde_json::from_value(json!({
        "type":"message","role":"user","content":[{"type":"input_text","text":"Run the synthetic echo test now."}]
    }))?;
    connection
        .send(ClientEvent {event_id:Field::Value("backend-user".into()),command:Command::ResponseItemCreate {item:user}})
        .await?;
    connection
        .send(ClientEvent {event_id:Field::Value("backend-start".into()),command:Command::ResponseCreate})
        .await?;
    let mut calls = Vec::new();
    let mut completed = 0;
    let mut update_acked = !update_during_handoff;
    let mut backend_text = String::new();
    let mut voice_text = String::new();
    let mut voiced_samples = 0;
    let mut response_ids = HashMap::new();
    let mut delegation_ids = HashMap::new();
    let mut active_responses = HashMap::<String,String>::new();
    let mut call_response = None::<(String,String)>;
    let mut timeline = Vec::new();
    let mut provider_error = None;
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let next = timeout(
            deadline.saturating_duration_since(Instant::now()),
            connection.next_event(),
        )
        .await;
        let frame = match next {
            Ok(frame) => frame?.ok_or(Error::UnconfirmedClose)?,
            Err(_) => {
                eprintln!("{}",json!({
                    "probe_timeout":true,"calls":calls.len(),"completed":completed,"update_acked":update_acked,
                    "backend_result_matched":backend_text.contains("NATIVE_BLUE_SEVEN"),
                    "voice_result_matched":voice_text.contains("The blue test passed"),
                    "voice_characters":voice_text.len(),"voiced_samples":voiced_samples,
                    "synthetic_backend_text":backend_text,"timeline":timeline,
                }));
                return Err(provider_error.map_or(Error::Timeout,Error::Provider));
            }
        };
        let decoded = frame.response_event();
        if decoded.is_err() {
            let null_fields: Vec<_> = frame.raw["event"]["response"].as_object()
                .into_iter().flat_map(|object| object.iter())
                .filter(|(_,value)| value.is_null()).map(|(key,_)| key.as_str()).collect();
            eprintln!("{}",json!({
                "nested_schema_event":frame.raw["event"]["type"],
                "null_snapshot_fields":null_fields,
            }));
        }
        if let Some(event) = decoded? {
            let scope_id = frame.raw["delegation_id"].as_str();
            let scope = frame.raw["delegation_id"].as_str().map(|id| {
                let next = delegation_ids.len() + 1;
                *delegation_ids.entry(id.to_owned()).or_insert(next)
            });
            if let oai_rt_rs::live::ResponseEvent::Lifecycle {response,kind,..} = &event {
                if *kind == oai_rt_rs::live::ResponseLifecycleKind::Created {
                    if let Some(scope_id) = scope_id {
                        if active_responses.get(scope_id).is_some_and(|id| id != &response.id) {
                            return Err(Error::Invalid("ambiguous overlapping responses in one delegation".into()));
                        }
                        active_responses.insert(scope_id.to_owned(),response.id.clone());
                    }
                }
                let next = response_ids.len() + 1;
                let label = *response_ids.entry(response.id.clone()).or_insert(next);
                timeline.push(json!({"lifecycle":format!("{kind:?}"),"response":label,"status":format!("{:?}",response.status),
                    "scope":scope,"correlation":frame.client_event_id}));
            }
            if let oai_rt_rs::live::ResponseEvent::OutputItemDone {
                item:oai_rt_rs::live::ResponseEventItem::FunctionCall(call),..
            } = &event {
                timeline.push(json!({"all_function_item_done_status":format!("{:?}",call.status),"args_bytes":call.args.len(),"scope":scope}));
            }
            if let Some(call) = event.completed_function_call() {
                let scope_id = scope_id.ok_or_else(|| Error::Invalid("unattributed function item: no outer delegation".into()))?;
                let response_id = active_responses.get(scope_id)
                    .ok_or_else(|| Error::Invalid("unattributed function item: no matching response.created".into()))?;
                let owner = (scope_id.to_owned(),response_id.clone());
                if call_response.as_ref().is_some_and(|prior| prior != &owner) {
                    return Err(Error::Invalid("function items belong to independent responses".into()));
                }
                call_response = Some(owner);
                calls.push(call.clone());
                timeline.push(json!({"function_item_done":true,"scope":scope}));
            }
            match frame.raw["event"]["type"].as_str() {
                Some("response.output_text.delta") if call_response.as_ref().is_some_and(|(scope,id)| {
                    Some(scope.as_str())==scope_id && active_responses.get(scope).is_some_and(|active| active!=id)
                }) => {
                    backend_text.push_str(
                        frame.raw["event"]["delta"]
                            .as_str()
                            .ok_or_else(|| Error::Invalid("missing backend text delta".into()))?,
                    );
                }
                Some("response.completed") => {
                    if frame.raw["event"]["response"]["output"] != json!([]) {
                        return Err(Error::Invalid(
                            "expected cleared Live backend lifecycle output".into(),
                        ));
                    }
                    let scope_id = scope_id.ok_or_else(|| Error::Invalid("unattributed completion".into()))?;
                    let response_id = frame.raw["event"]["response"]["id"].as_str()
                        .ok_or_else(|| Error::Invalid("completion lacks response identity".into()))?;
                    if active_responses.get(scope_id).map(String::as_str) != Some(response_id) {
                        return Err(Error::Invalid("completion does not match the active scoped response".into()));
                    }
                    active_responses.remove(scope_id);
                    let owner = (scope_id.to_owned(),response_id.to_owned());
                    if call_response.as_ref() == Some(&owner) && completed == 0 {
                        completed = 1;
                        backend_text.clear();
                        timeline.push(json!({"submit_results_after_completion":completed,"calls":calls.len()}));
                        submit_results(connection, &calls, update_during_handoff).await?;
                    } else if completed == 1 && call_response.as_ref().is_some_and(|(scope,id)| scope==scope_id && id!=response_id) {
                        completed = 2;
                    }
                }
                Some("response.output_text.done") if call_response.as_ref().is_some_and(|(scope,id)| {
                    Some(scope.as_str())==scope_id && active_responses.get(scope).is_some_and(|active| active!=id)
                }) => {
                    backend_text = frame.raw["event"]["text"].as_str()
                        .ok_or_else(|| Error::Invalid("missing completed backend text".into()))?.to_owned();
                }
                Some(kind @ ("response.failed" | "response.incomplete")) => {
                    eprintln!("{}",json!({
                        "backend_terminal":kind,
                        "error_code":frame.raw["event"]["response"]["error"]["code"],
                        "incomplete_reason":frame.raw["event"]["response"]["incomplete_details"]["reason"],
                    }));
                    return Err(Error::Invalid("backend did not complete successfully".into()));
                }
                _ => {}
            }
        }
        if let Some(chunk) = frame.audio(
            oai_rt_rs::live::ConnectionRole::Primary,
            oai_rt_rs::live::AudioFormat::default(),
        )? {
            if chunk
                .bytes
                .chunks_exact(2)
                .any(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]).unsigned_abs() >= 500)
            {
                voiced_samples += chunk.bytes.len() / 2;
            }
        }
        match frame.event {
            ServerEvent::Updated { .. }
                if update_during_handoff && frame.client_event_id.as_deref() == Some("backend-update") =>
            {
                update_acked = true;
                timeline.push(json!({"update_acked_then_continue":true}));
                connection
                    .send(ClientEvent {event_id:Field::Value("backend-continue".into()),command:Command::ResponseCreate})
                    .await?;
            }
            ServerEvent::OutputTranscriptDelta { delta, .. } => voice_text.push_str(&delta),
            ServerEvent::Error { error, .. } => {
                timeline.push(json!({"provider_error_code":error.code,"correlation":error.client_event_id}));
                provider_error = Some(error);
            }
            _ => {}
        }
        if completed == 2
            && update_acked
            && backend_text.contains("NATIVE_BLUE_SEVEN")
            && voice_text.contains("The blue test passed")
            && voiced_samples >= 2400
        {
            if let Some(error) = provider_error {
                eprintln!("{}",json!({"provider_error_with_later_success_timeline":timeline}));
                return Err(Error::Provider(error));
            }
            return Ok(json!({
                "probe":"public-live-managed-responses","complete_function_calls":calls.len(),
                "backend_completions":completed,"cleared_snapshots_verified":true,
                "backend_result_matched":true,"voice_result_matched":true,
                "voiced_samples":voiced_samples,"sparse_update_acked":true,
                "update_during_handoff":update_during_handoff,
                "created_response_count":response_ids.len(),
                "call_response_identity_verified":true,
            }));
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let result = run().await;
    if let Err(Error::Provider(error)) = &result {
        eprintln!("{}",json!({
            "probe_error_code":error.code,
            "parameter":error.param,
            "client_event_id":error.client_event_id,
            "message":error.message.split_whitespace().map(|word| {
                if ["resp_","live_","call_","sk-"].iter().any(|prefix| word.contains(prefix)) {
                    "[identifier]"
                } else { word }
            }).collect::<Vec<_>>().join(" "),
        }));
    }
    result
}

async fn run() -> Result<()> {
    let key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        Error::Invalid("OPENAI_API_KEY is required; this probe does not skip".into())
    })?;
    let client = LiveClient::new(&key)?;
    let update_during_handoff = std::env::args().any(|arg| arg == "--update-during-handoff");
    let mut connection = client.connect(config(update_during_handoff)).await?;
    let sender = connection.sender();
    let chunk_samples = if std::env::args().any(|arg| arg == "--100ms-audio") {2400} else {480};
    let audio = tokio::spawn(async move {
        sender
            .send_audio_paced(&vec![0; 24000 * 2 * 25], chunk_samples)
            .await
    });
    let result = exercise(&mut connection, update_during_handoff).await;
    if let Err(error) = &result {
        match error {
            Error::Json(detail) => eprintln!("{}",json!({"probe_schema_error":detail.to_string()})),
            Error::MalformedEvent {raw,source} => {
                let null_fields: Vec<_> = raw["error"].as_object().into_iter()
                    .flat_map(|object| object.iter()).filter(|(_,value)| value.is_null())
                    .map(|(key,_)| key.as_str()).collect();
                eprintln!("{}",json!({
                    "malformed_live_event":raw["type"],"schema_error":source.to_string(),
                    "provider_error_code":raw["error"]["code"],"null_error_fields":null_fields,
                }));
            }
            Error::Provider(detail) => eprintln!("{}",json!({"probe_error_code":detail.code,"parameter":detail.param})),
            other => eprintln!("probe stage failed: {other}"),
        }
    }
    audio.abort();
    let audio_result = audio.await;
    let close_result = connection
        .close(Duration::from_secs(10), |_| Ok(()))
        .await;
    if let Err(error) = &close_result { eprintln!("probe finalization failed: {error}"); }
    if result.is_err() {
        if let Ok(frame) = &close_result {
            if let ServerEvent::Closed {usage,..} = &frame.event {
                eprintln!("{}",json!({"final_usage_confirmed_after_probe_error":true,"final_seconds":usage.seconds}));
            }
        }
    }
    match audio_result {
        Ok(result) => result?,
        Err(error) if error.is_cancelled() => {}
        Err(_) => return Err(Error::Transport("audio producer failed".into())),
    }
    let mut report = result?;
    let closed = close_result?;
    let ServerEvent::Closed { usage, .. } = closed.event else {
        return Err(Error::UnconfirmedClose);
    };
    report["final_seconds"] = json!(usage.seconds);
    println!("{report}");
    Ok(())
}
