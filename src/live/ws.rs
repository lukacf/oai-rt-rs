use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt, stream::SplitSink};
use std::{collections::VecDeque, future::Future, pin::Pin, time::Duration};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async_with_config,
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message, WebSocketConfig},
    },
};

use super::{
    AudioFormat, ClientEvent, Codec, Command, DelegationConfig, DelegationTarget, DelegationUpdate,
    Error, Field, ForkSessionConfig, ForkStartEvent, HttpBodyIssue, LiveClient, Result,
    ServerEvent, ServerFrame, SessionConfig, decode_audio, validate_audio_bytes,
};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

enum Startup {
    New(Box<SessionConfig>),
    Fork(AudioFormat),
    Attached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionRole {
    Primary,
    Sideband,
}

/// Additional query configuration documented by the official sideband SDK/schema.
#[derive(Clone, Copy, Debug, Default)]
pub struct SidebandOptions {
    /// Opt into the WebSocket closing handshake. This is not a final-usage receipt.
    pub graceful_close: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionPhase {
    Starting,
    Active,
    Closing,
    Closed,
    Disconnected,
}

struct WriteRequest {
    event: ClientEvent,
    completion: oneshot::Sender<Result<()>>,
}

/// A bounded full-duplex connection. The driver keeps receiving while commands
/// are sent. All events, including startup, remain visible to the receiver.
pub struct LiveConnection {
    sender: LiveSender,
    receiver: LiveReceiver,
}

/// Cloneable bounded command sender. Send completion means the frame was written,
/// not that the model accepted, consumed, spoke, or played it.
///
/// Cancelling `send` before queueing does not send. Once queueing succeeds,
/// cancellation can race with a write; never automatically retry that command.
#[derive(Clone)]
pub struct LiveSender {
    commands: mpsc::Sender<WriteRequest>,
    phase: watch::Receiver<SessionPhase>,
    role: ConnectionRole,
    format: AudioFormat,
}

/// Sole event receiver. Dropping it aborts the driver and releases the socket.
/// Drop is not a graceful close and cannot confirm final usage.
pub struct LiveReceiver {
    events: mpsc::Receiver<Result<ServerFrame>>,
    buffered: VecDeque<ServerFrame>,
    driver: JoinHandle<()>,
    phase: watch::Sender<SessionPhase>,
}

impl Drop for LiveReceiver {
    fn drop(&mut self) {
        self.driver.abort();
        if *self.phase.borrow() != SessionPhase::Closed {
            self.phase.send_replace(SessionPhase::Disconnected);
        }
    }
}

impl LiveClient {
    /// Connect, send `session.start`, and wait for `session.started`. No model
    /// query, private bootstrap, or Realtime endpoint is used.
    ///
    /// # Errors
    /// Returns startup/provider/handshake errors or a bounded startup timeout.
    pub async fn connect(&self, session: SessionConfig) -> Result<LiveConnection> {
        session.validate()?;
        let socket = self.open_socket(&["live", "sessions"]).await?;
        let event = ClientEvent::new(Command::Start {
            session: session.clone(),
        });
        let connection = self.spawn(socket, Startup::New(Box::new(session)));
        connection.sender.send(event).await?;
        self.wait_started(connection).await
    }

    /// Fork a completed stored session onto a new primary WebSocket.
    /// The model/history/voice are inherited; audio defaults to PCM16/24k again.
    ///
    /// # Errors
    /// Returns invalid override, handshake, startup, or ambiguous-write errors.
    pub async fn fork(
        &self,
        source_session_id: &str,
        session: ForkSessionConfig,
    ) -> Result<LiveConnection> {
        let event = ForkStartEvent::new(session);
        let text = self.options.codec.encode_fork_start(&event)?;
        let format = event.session.audio_format();
        let mut socket = self
            .open_socket(&["live", "sessions", source_session_id, "fork"])
            .await?;
        if !matches!(
            timeout(
                self.options.request_timeout,
                socket.send(Message::Text(text.into()))
            )
            .await,
            Ok(Ok(()))
        ) {
            return Err(Error::AmbiguousWrite);
        }
        let connection = self
            .wait_started(self.spawn(socket, Startup::Fork(format)))
            .await?;
        if connection.receiver.buffered.iter().any(|frame| {
            matches!(&frame.event,ServerEvent::Started {session,..} if session.id == source_session_id)
        }) {
            return Err(Error::Invalid("fork must return a new session identifier".into()));
        }
        Ok(connection)
    }

    async fn wait_started(&self, mut connection: LiveConnection) -> Result<LiveConnection> {
        let ready = timeout(self.options.request_timeout, async {
            loop {
                let frame = connection
                    .receiver
                    .events
                    .recv()
                    .await
                    .ok_or(Error::UnconfirmedClose)??;
                match &frame.event {
                    ServerEvent::Error { error, .. } => return Err(Error::Provider(error.clone())),
                    ServerEvent::Closed { .. } => return Err(Error::UnconfirmedClose),
                    _ => {}
                }
                let started = matches!(frame.event, ServerEvent::Started { .. });
                connection.receiver.buffered.push_back(frame);
                if started {
                    return Ok(());
                }
                if connection.receiver.buffered.len() >= self.options.event_capacity {
                    return Err(Error::Invalid(
                        "startup events exceed configured queue capacity".into(),
                    ));
                }
            }
        })
        .await
        .map_err(|_| Error::Timeout)?;
        ready?;
        Ok(connection)
    }

    /// Attach a trusted control/observation sideband. Attaching does not start a
    /// new session or replay history. Audio input belongs on the primary media
    /// transport, never this connection.
    ///
    /// # Errors
    /// Returns an error if the session identifier or authenticated handshake fails.
    pub async fn attach(&self, session_id: &str) -> Result<LiveConnection> {
        self.attach_with_options(session_id, SidebandOptions::default())
            .await
    }

    /// Attach with explicit sideband query options; omitted values remain omitted.
    ///
    /// # Errors
    /// Returns identifier or authenticated handshake failures.
    pub async fn attach_with_options(
        &self,
        session_id: &str,
        options: SidebandOptions,
    ) -> Result<LiveConnection> {
        let socket = self
            .open_socket_with_query(
                &["live", "sessions", session_id, "attach"],
                options.graceful_close,
            )
            .await?;
        Ok(self.spawn(socket, Startup::Attached))
    }

    async fn open_socket(&self, segments: &[&str]) -> Result<Socket> {
        self.open_socket_with_query(segments, None).await
    }

    async fn open_socket_with_query(
        &self,
        segments: &[&str],
        graceful_close: Option<bool>,
    ) -> Result<Socket> {
        let mut url = self.endpoint(segments)?;
        if let Some(graceful_close) = graceful_close {
            url.query_pairs_mut().append_pair(
                "graceful_close",
                if graceful_close { "true" } else { "false" },
            );
        }
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|()| Error::Invalid("invalid WebSocket URL".into()))?;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|_| Error::Invalid("invalid WebSocket request".into()))?;
        request.headers_mut().extend(self.headers.clone());
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(self.options.codec.max_event_bytes);
        config.max_frame_size = Some(self.options.codec.max_event_bytes);
        let result = timeout(
            self.options.request_timeout,
            connect_async_with_config(request, Some(config), false),
        )
        .await
        .map_err(|_| Error::Timeout)?;
        match result {
            Ok((socket, _)) => Ok(socket),
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                let header = |name: &str| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|h| h.to_str().ok())
                        .map(str::to_owned)
                };
                let mut body = response.body().clone().unwrap_or_default();
                let body_issue = upgrade_body_issue(
                    response.headers(),
                    body.len(),
                    self.options.codec.max_event_bytes,
                );
                body.truncate(self.options.codec.max_event_bytes);
                Err(Error::Http {
                    status: response.status().as_u16(),
                    headers: Box::new(response.headers().clone()),
                    request_id: header("x-request-id"),
                    content_type: header("content-type"),
                    retry_after: header("retry-after"),
                    body,
                    body_issue,
                })
            }
            Err(_) => Err(Error::Transport("WebSocket handshake".into())),
        }
    }

    fn spawn(&self, socket: Socket, startup: Startup) -> LiveConnection {
        let (role, config, format, started_sent) = match startup {
            Startup::New(config) => {
                let format = config.audio_format();
                (ConnectionRole::Primary, Some(*config), format, false)
            }
            Startup::Fork(format) => (ConnectionRole::Primary, None, format, true),
            Startup::Attached => (ConnectionRole::Sideband, None, AudioFormat::default(), true),
        };
        let (commands, requests) = mpsc::channel(self.options.command_capacity);
        let (events, incoming) = mpsc::channel(self.options.event_capacity);
        let initial = if role == ConnectionRole::Primary {
            SessionPhase::Starting
        } else {
            SessionPhase::Active
        };
        let (phase_tx, phase) = watch::channel(initial);
        let state = DriverState {
            role,
            config,
            format,
            phase: phase_tx.clone(),
            started_sent,
            delegation_target: None,
        };
        let driver = tokio::spawn(run_driver(
            socket,
            requests,
            events,
            state,
            self.options.codec,
            self.options.request_timeout,
        ));
        LiveConnection {
            sender: LiveSender {
                commands,
                phase,
                role,
                format,
            },
            receiver: LiveReceiver {
                events: incoming,
                buffered: VecDeque::new(),
                driver,
                phase: phase_tx,
            },
        }
    }
}

impl LiveConnection {
    #[must_use]
    pub fn sender(&self) -> LiveSender {
        self.sender.clone()
    }

    #[must_use]
    pub fn split(self) -> (LiveSender, LiveReceiver) {
        (self.sender, self.receiver)
    }

    /// # Errors
    /// Returns validation, lifecycle, or ambiguous delivery errors.
    pub async fn send(&self, event: ClientEvent) -> Result<()> {
        self.sender.send(event).await
    }

    /// # Errors
    /// Returns malformed event/transport errors; unexpected EOF is not success.
    pub async fn next_event(&mut self) -> Result<Option<ServerFrame>> {
        self.receiver.next_event().await
    }

    /// Send close and drain every remaining event through the supplied callback,
    /// returning the terminal snapshot and final usage. No events are discarded.
    ///
    /// # Errors
    /// Returns an error for callback failure, timeout, or unconfirmed EOF. Rejections
    /// of earlier commands are observed without stopping final-usage draining.
    pub async fn close<F>(&mut self, deadline: Duration, mut observe: F) -> Result<ServerFrame>
    where
        F: FnMut(&ServerFrame) -> Result<()>,
    {
        self.close_with_events(deadline, |event| match event {
            Ok(frame) => observe(&frame),
            Err(error) => Err(error),
        })
        .await
    }

    /// Close while explicitly observing both decoded events and decoding failures.
    /// Returning `Ok(())` for an error allows draining a later valid final event;
    /// it does not turn that error into a successful event. Queues remain bounded.
    ///
    /// # Errors
    /// Returns callback failure, deadline expiry, or unconfirmed termination.
    pub async fn close_with_events<F>(
        &mut self,
        deadline: Duration,
        mut observe: F,
    ) -> Result<ServerFrame>
    where
        F: FnMut(Result<ServerFrame>) -> Result<()>,
    {
        let sender = self.sender.clone();
        let mut sent = sender.phase() != SessionPhase::Active;
        let send_close = sender.send(ClientEvent::new(Command::Close));
        tokio::pin!(send_close);
        let result = timeout(deadline, async {
            loop {
                tokio::select! {
                    result = &mut send_close, if !sent => {
                        sent = true;
                        match result {
                            Ok(()) | Err(Error::Closed | Error::AmbiguousWrite) => {}
                            Err(error) => return Err(error),
                        }
                    }
                    frame = self.next_event() => {
                        match frame {
                            Ok(Some(frame)) => {
                                if matches!(frame.event, ServerEvent::Closed { .. }) {
                                    observe(Ok(frame.clone()))?;
                                    return Ok(frame);
                                }
                                observe(Ok(frame))?;
                            }
                            Ok(None) => return Err(Error::UnconfirmedClose),
                            Err(error) => observe(Err(error))?,
                        }
                    }
                }
            }
        })
        .await;
        let result = result.unwrap_or(Err(Error::Timeout));
        if result.is_err() {
            self.receiver.driver.abort();
            if *self.receiver.phase.borrow() != SessionPhase::Closed {
                self.receiver.phase.send_replace(SessionPhase::Disconnected);
            }
        }
        result
    }
}

impl LiveSender {
    #[must_use]
    pub fn phase(&self) -> SessionPhase {
        *self.phase.borrow()
    }

    #[must_use]
    pub const fn role(&self) -> ConnectionRole {
        self.role
    }

    /// Enqueue and write one command without waiting for a provider ACK.
    ///
    /// # Errors
    /// Returns validation, lifecycle, or ambiguous write errors. No retry occurs.
    pub async fn send(&self, event: ClientEvent) -> Result<()> {
        event.validate()?;
        if matches!(
            self.phase(),
            SessionPhase::Closing | SessionPhase::Closed | SessionPhase::Disconnected
        ) {
            return Err(Error::Closed);
        }
        let (completion, result) = oneshot::channel();
        self.commands
            .send(WriteRequest { event, completion })
            .await
            .map_err(|_| Error::Closed)?;
        result.await.unwrap_or(Err(Error::AmbiguousWrite))
    }

    /// Encode one raw audio chunk. Caller controls ordering, pacing and silence.
    ///
    /// # Errors
    /// Rejects sideband audio, invalid samples, or a closing session.
    pub async fn send_audio(&self, bytes: &[u8]) -> Result<()> {
        if self.role != ConnectionRole::Primary {
            return Err(Error::Invalid("sideband cannot send input audio".into()));
        }
        validate_audio_bytes(bytes, self.format)?;
        self.send(ClientEvent::new(Command::InputAudioAppend {
            audio: STANDARD.encode(bytes),
        }))
        .await
    }

    /// Pace a finite synthetic/raw audio sample at the configured sample rate.
    /// This sends only the provided audio; it does not synthesize trailing silence.
    ///
    /// # Errors
    /// Rejects empty/invalid chunks and returns any send failure immediately.
    pub async fn send_audio_paced(&self, bytes: &[u8], chunk_samples: usize) -> Result<()> {
        validate_audio_bytes(bytes, self.format)?;
        let chunk_bytes = chunk_samples
            .checked_mul(self.format.bytes_per_sample())
            .filter(|n| *n > 0)
            .ok_or_else(|| Error::Invalid("invalid audio chunk size".into()))?;
        let start = Instant::now();
        let mut offset = Duration::ZERO;
        for chunk in bytes.chunks(chunk_bytes) {
            tokio::time::sleep_until(start + offset).await;
            self.send_audio(chunk).await?;
            let samples = u32::try_from(chunk.len() / self.format.bytes_per_sample())
                .map_err(|_| Error::Invalid("audio chunk too large".into()))?;
            offset +=
                Duration::from_secs_f64(f64::from(samples) / f64::from(self.format.sample_rate()));
        }
        tokio::time::sleep_until(start + offset).await;
        Ok(())
    }
}

impl LiveReceiver {
    /// # Errors
    /// Returns protocol/transport errors; only confirmed termination yields EOF.
    pub async fn next_event(&mut self) -> Result<Option<ServerFrame>> {
        if let Some(frame) = self.buffered.pop_front() {
            return Ok(Some(frame));
        }
        self.events.recv().await.transpose()
    }
}

struct DriverState {
    role: ConnectionRole,
    config: Option<SessionConfig>,
    format: AudioFormat,
    phase: watch::Sender<SessionPhase>,
    started_sent: bool,
    delegation_target: Option<DelegationTarget>,
}

impl DriverState {
    fn validate(&self, command: &Command) -> Result<()> {
        let phase = *self.phase.borrow();
        if matches!(
            phase,
            SessionPhase::Closing | SessionPhase::Closed | SessionPhase::Disconnected
        ) {
            return Err(Error::Closed);
        }
        if matches!(command, Command::Start { .. }) {
            if self.role != ConnectionRole::Primary || self.started_sent {
                return Err(Error::Invalid(
                    "session.start is primary-only and once-only".into(),
                ));
            }
        } else if phase == SessionPhase::Starting {
            return Err(Error::Invalid(
                "wait for session.started before application commands".into(),
            ));
        }
        if let Command::InputAudioAppend { audio } = command {
            if self.role != ConnectionRole::Primary {
                return Err(Error::Invalid("sideband cannot send input audio".into()));
            }
            decode_audio(audio, self.format)?;
        }
        if matches!(
            command,
            Command::ResponseItemCreate { .. } | Command::ResponseCreate
        ) && self.config.as_ref().is_some_and(|config| {
            !matches!(
                config.delegation,
                Field::Value(DelegationConfig::Responses { .. })
            )
        }) {
            return Err(Error::Invalid(
                "Responses commands require Responses delegation".into(),
            ));
        }
        if let Command::Update { session } = command {
            if let Some(config) = &self.config {
                let responses_mode = matches!(
                    config.delegation,
                    Field::Value(DelegationConfig::Responses { .. })
                );
                let requested_mode = match session.delegation {
                    Field::Absent => None,
                    Field::Value(DelegationUpdate::Responses { .. }) => Some(true),
                    Field::Value(DelegationUpdate::Client) | Field::Null => Some(false),
                };
                if requested_mode.is_some_and(|mode| mode != responses_mode) {
                    return Err(Error::Invalid(
                        "delegation mode is immutable; null selects client".into(),
                    ));
                }
            }
        }
        match command {
            Command::InstructionsAppend { delegation_id, .. }
            | Command::ThinkingAppend { delegation_id, .. }
            | Command::CommentaryAppend { delegation_id, .. }
                if delegation_id.0.is_some()
                    && (self.delegation_target == Some(DelegationTarget::Responses)
                        || self.config.as_ref().is_some_and(|config| {
                            matches!(
                                config.delegation,
                                Field::Value(DelegationConfig::Responses { .. })
                            )
                        })) =>
            {
                return Err(Error::Invalid(
                    "context requires a client delegation, not a Responses delegation".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    fn prepare(&mut self, event: &ClientEvent, codec: Codec) -> Result<String> {
        self.validate(&event.command)?;
        let text = codec.encode(event)?;
        if matches!(event.command, Command::Start { .. }) {
            self.started_sent = true;
        }
        if matches!(event.command, Command::Close) {
            self.phase.send_replace(SessionPhase::Closing);
        }
        Ok(text)
    }

    fn observe(&mut self, frame: &ServerFrame) -> bool {
        let startup_error = *self.phase.borrow() == SessionPhase::Starting
            && matches!(frame.event, ServerEvent::Error { .. });
        if let ServerEvent::Started { session, .. } = &frame.event {
            if self.config.is_none() && !session.delegation.is_absent() {
                self.config = Some(SessionConfig {
                    model: session.model.clone(),
                    delegation: session.delegation.clone(),
                    ..SessionConfig::default()
                });
            }
        }
        match &frame.event {
            ServerEvent::Started { .. } if *self.phase.borrow() == SessionPhase::Starting => {
                self.phase.send_replace(SessionPhase::Active);
            }
            ServerEvent::Closed { .. } => {
                self.phase.send_replace(SessionPhase::Closed);
            }
            ServerEvent::DelegationCreated { delegation, .. } => {
                // Delegation mode is immutable; no per-ID history is needed.
                self.delegation_target.get_or_insert(delegation.target);
            }
            _ => {}
        }
        if startup_error {
            self.phase.send_replace(SessionPhase::Disconnected);
        }
        matches!(frame.event, ServerEvent::Closed { .. }) || startup_error
    }
}

fn upgrade_body_issue(
    headers: &reqwest::header::HeaderMap,
    buffered: usize,
    limit: usize,
) -> Option<HttpBodyIssue> {
    if buffered > limit {
        return Some(HttpBodyIssue::Truncated);
    }
    if headers.contains_key("transfer-encoding") {
        return Some(HttpBodyIssue::Unconfirmed);
    }
    let mut lengths = headers.get_all("content-length").iter();
    let length = lengths
        .next()
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<usize>().ok());
    if lengths.next().is_none() && length == Some(buffered) {
        None
    } else {
        Some(HttpBodyIssue::Unconfirmed)
    }
}

type Writer = SplitSink<Socket, Message>;
type WriteFuture = Pin<Box<dyn Future<Output = (Writer, bool)> + Send>>;

struct InFlightWrite {
    future: WriteFuture,
    completion: Option<oneshot::Sender<Result<()>>>,
}

fn begin_write(
    mut writer: Writer,
    text: Option<String>,
    deadline: Duration,
    completion: Option<oneshot::Sender<Result<()>>>,
) -> InFlightWrite {
    let future = Box::pin(async move {
        let success = timeout(deadline, async {
            if let Some(text) = text {
                writer.send(Message::Text(text.into())).await
            } else {
                writer.flush().await
            }
        })
        .await
        .is_ok_and(|result| result.is_ok());
        (writer, success)
    });
    InFlightWrite { future, completion }
}

fn reject_queued(requests: &mut mpsc::Receiver<WriteRequest>) {
    requests.close();
    while let Ok(request) = requests.try_recv() {
        let _ = request.completion.send(Err(Error::Closed));
    }
}

async fn run_driver(
    socket: Socket,
    mut requests: mpsc::Receiver<WriteRequest>,
    events: mpsc::Sender<Result<ServerFrame>>,
    mut state: DriverState,
    codec: Codec,
    write_timeout: Duration,
) {
    let (writer, mut reader) = socket.split();
    let mut writer = Some(writer);
    let mut writing: Option<InFlightWrite> = None;
    let mut pending = VecDeque::new();
    let mut commands_open = true;
    let mut terminal = false;
    let mut flush_pending = false;
    let mut receive_deadline = None;
    loop {
        if terminal {
            reject_queued(&mut requests);
            if let Some(completion) = writing.as_mut().and_then(|write| write.completion.take()) {
                let _ = completion.send(Err(Error::AmbiguousWrite));
            }
        }
        if flush_pending && writing.is_none() && !terminal && receive_deadline.is_none() {
            writing = Some(begin_write(
                writer.take().expect("idle writer"),
                None,
                write_timeout,
                None,
            ));
            flush_pending = false;
        }
        tokio::select! {
            () = events.closed() => break,
            permit = events.reserve(), if !pending.is_empty() => {
                let Ok(permit) = permit else { break };
                if let Some(frame) = pending.pop_front() {
                    permit.send(frame);
                }
                if terminal && pending.is_empty() { break; }
            }
            (returned_writer, success) = async { writing.as_mut().expect("in-flight writer").future.as_mut().await }, if writing.is_some() => {
                if let Some(completion) = writing.take().and_then(|write| write.completion) {
                    let _ = completion.send(if success { Ok(()) } else { Err(Error::AmbiguousWrite) });
                }
                writer = Some(returned_writer);
                if !success && !terminal {
                    state.phase.send_replace(SessionPhase::Disconnected);
                    reject_queued(&mut requests);
                    commands_open = false;
                    // A broken sink does not prove the reader has lost final usage.
                    receive_deadline = Some(Instant::now() + write_timeout);
                }
            }
            () = async { tokio::time::sleep_until(receive_deadline.expect("receive deadline")).await }, if receive_deadline.is_some() && !terminal => {
                pending.push_back(Err(Error::UnconfirmedClose));
                terminal = true;
            }
            request = requests.recv(), if commands_open && !terminal && writing.is_none() => {
                let Some(request) = request else { commands_open = false; continue };
                if request.completion.is_closed() { continue; }
                let text = match state.prepare(&request.event, codec) {
                    Ok(text) => text,
                    Err(error) => { let _ = request.completion.send(Err(error)); continue; }
                };
                writing = Some(begin_write(writer.take().expect("idle writer"), Some(text), write_timeout, Some(request.completion)));
            }
            message = reader.next(), if pending.is_empty() && !terminal => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match codec.decode_server(&text) {
                            Ok(frame) => {
                                terminal = state.observe(&frame);
                                pending.push_back(Ok(frame));
                            }
                            Err(error) => { pending.push_back(Err(error)); }
                        }
                    }
                    Some(Ok(Message::Ping(_))) => {
                        flush_pending = true;
                    }
                    Some(Ok(Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Ok(Message::Binary(_))) => {
                        pending.push_back(Err(Error::Invalid("Live events must be JSON text frames".into())));
                        terminal = true;
                    }
                    Some(Err(tokio_tungstenite::tungstenite::Error::Capacity(_))) => {
                        pending.push_back(Err(Error::ContinuityLost));
                        pending.push_back(Err(Error::UnconfirmedClose));
                        terminal = true;
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => {
                        pending.push_back(Err(Error::UnconfirmedClose));
                        terminal = true;
                    }
                }
            }
        }
    }
    reject_queued(&mut requests);
    drop(writing);
    if *state.phase.borrow() != SessionPhase::Closed {
        state.phase.send_replace(SessionPhase::Disconnected);
    }
    // Releasing the socket is bounded even if the peer does not finish its close handshake.
    if let Some(mut writer) = writer {
        let _ = timeout(Duration::from_secs(1), writer.close()).await;
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use reqwest::header::{CONTENT_LENGTH, HeaderMap, HeaderValue, TRANSFER_ENCODING};

    #[test]
    fn error_body_completeness_requires_unambiguous_ascii_content_length() {
        for (lengths, buffered, expected) in [
            (vec!["0"], 0, None),
            (vec!["7"], 7, None),
            (vec!["007"], 7, None),
            (vec!["+0"], 0, Some(HttpBodyIssue::Unconfirmed)),
            (vec!["+7"], 7, Some(HttpBodyIssue::Unconfirmed)),
            (vec![""], 0, Some(HttpBodyIssue::Unconfirmed)),
            (vec!["-0"], 0, Some(HttpBodyIssue::Unconfirmed)),
            (vec!["7 "], 7, Some(HttpBodyIssue::Unconfirmed)),
            (vec!["0x7"], 7, Some(HttpBodyIssue::Unconfirmed)),
            (
                vec!["184467440737095516160"],
                0,
                Some(HttpBodyIssue::Unconfirmed),
            ),
            (vec!["0", "0"], 0, Some(HttpBodyIssue::Unconfirmed)),
            (vec!["0, 0"], 0, Some(HttpBodyIssue::Unconfirmed)),
            (vec!["7"], 0, Some(HttpBodyIssue::Unconfirmed)),
            (vec![], 0, Some(HttpBodyIssue::Unconfirmed)),
        ] {
            let mut headers = HeaderMap::new();
            for length in lengths {
                headers.append(CONTENT_LENGTH, HeaderValue::from_str(length).unwrap());
            }
            assert_eq!(upgrade_body_issue(&headers, buffered, 10), expected);
            headers.insert(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));
            assert_eq!(
                upgrade_body_issue(&headers, buffered, 10),
                Some(HttpBodyIssue::Unconfirmed)
            );
        }
        assert_eq!(
            upgrade_body_issue(&HeaderMap::new(), 11, 10),
            Some(HttpBodyIssue::Truncated)
        );
    }
}
