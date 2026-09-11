use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::{SinkExt, StreamExt, stream::SplitSink};
use std::{collections::VecDeque, future::Future, pin::Pin, time::Duration};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{
        handshake::{client::generate_key, derive_accept_key},
        protocol::{Message, Role, WebSocketConfig},
    },
};

use super::{
    AudioFormat, ClientEvent, Codec, Command, DelegationConfig, DelegationTarget, DelegationUpdate,
    Error, Field, ForkSessionConfig, ForkStartEvent, HttpBodyIssue, LiveClient, Result,
    ServerEvent, ServerFrame, SessionConfig, decode_audio, validate_audio_bytes,
};

type Socket = WebSocketStream<reqwest::Upgraded>;

#[derive(Clone, Copy)]
enum Startup {
    New {
        format: AudioFormat,
        responses_mode: bool,
    },
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
    text: String,
    guard: CommandGuard,
    completion: oneshot::Sender<Result<()>>,
}

enum CommandGuard {
    Start,
    Audio,
    Responses,
    Update(Option<bool>),
    Context { attributed: bool },
    Close,
    Other,
}

impl CommandGuard {
    const fn from_command(command: &Command) -> Self {
        match command {
            Command::Start { .. } => Self::Start,
            Command::InputAudioAppend { .. } => Self::Audio,
            Command::ResponseItemCreate { .. } | Command::ResponseCreate => Self::Responses,
            Command::Update { session } => Self::Update(match &session.delegation {
                Field::Absent => None,
                Field::Value(DelegationUpdate::Responses { .. }) => Some(true),
                Field::Value(DelegationUpdate::Client) | Field::Null => Some(false),
            }),
            Command::InstructionsAppend { delegation_id, .. }
            | Command::ThinkingAppend { delegation_id, .. }
            | Command::CommentaryAppend { delegation_id, .. } => Self::Context {
                attributed: delegation_id.0.is_some(),
            },
            Command::Close => Self::Close,
            _ => Self::Other,
        }
    }
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
    codec: Codec,
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
        let startup = Startup::New {
            format: session.audio_format(),
            responses_mode: matches!(
                session.delegation,
                Field::Value(DelegationConfig::Responses { .. })
            ),
        };
        let text = self
            .options
            .codec
            .encode(&ClientEvent::new(Command::Start { session }))?;
        let socket = self.open_socket(&["live", "sessions"]).await?;
        let connection = self.spawn(socket, startup);
        connection
            .sender
            .send_prepared(text, CommandGuard::Start)
            .await?;
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
        let key = generate_key();
        let deadline = Instant::now() + self.options.request_timeout;
        // tungstenite's authenticated client handshake logs raw headers at TRACE.
        // reqwest retains sensitive-header redaction; only upgraded IO reaches it.
        let response = self
            .websocket_http
            .get(url)
            .header("connection", "Upgrade")
            .header("upgrade", "websocket")
            .header("sec-websocket-version", "13")
            .header("sec-websocket-key", &key)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    Error::Timeout
                } else {
                    Error::Transport("WebSocket HTTP handshake".into())
                }
            })?;
        if response.status() != reqwest::StatusCode::SWITCHING_PROTOCOLS {
            return Err(self.http_error_until(response, deadline).await);
        }
        let rejected = rejected_upgrade(&response);
        if !valid_upgrade(&response, &key) {
            return Err(rejected);
        }
        let Ok(Ok(upgraded)) = tokio::time::timeout_at(deadline, response.upgrade()).await else {
            return Err(rejected);
        };
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(self.options.codec.max_event_bytes);
        config.max_frame_size = Some(self.options.codec.max_event_bytes);
        Ok(WebSocketStream::from_raw_socket(upgraded, Role::Client, Some(config)).await)
    }

    fn spawn(&self, socket: Socket, startup: Startup) -> LiveConnection {
        let (role, responses_mode, format, started_sent) = match startup {
            Startup::New {
                format,
                responses_mode,
            } => (ConnectionRole::Primary, Some(responses_mode), format, false),
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
            responses_mode,
            phase: phase_tx.clone(),
            started_sent,
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
                codec: self.options.codec,
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
        let text = self.codec.encode(&event)?;
        if let Command::InputAudioAppend { audio } = &event.command {
            decode_audio(audio, self.format)?;
        }
        let guard = CommandGuard::from_command(&event.command);
        drop(event);
        self.send_prepared(text, guard).await
    }

    async fn send_prepared(&self, text: String, guard: CommandGuard) -> Result<()> {
        if matches!(
            self.phase(),
            SessionPhase::Closing | SessionPhase::Closed | SessionPhase::Disconnected
        ) {
            return Err(Error::Closed);
        }
        let (completion, result) = oneshot::channel();
        self.commands
            .send(WriteRequest {
                text,
                guard,
                completion,
            })
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
    responses_mode: Option<bool>,
    phase: watch::Sender<SessionPhase>,
    started_sent: bool,
}

impl DriverState {
    fn validate(&self, command: &CommandGuard) -> Result<()> {
        let phase = *self.phase.borrow();
        if matches!(
            phase,
            SessionPhase::Closing | SessionPhase::Closed | SessionPhase::Disconnected
        ) {
            return Err(Error::Closed);
        }
        if matches!(command, CommandGuard::Start) {
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
        if matches!(command, CommandGuard::Audio) && self.role != ConnectionRole::Primary {
            return Err(Error::Invalid("sideband cannot send input audio".into()));
        }
        if matches!(command, CommandGuard::Responses) && self.responses_mode == Some(false) {
            return Err(Error::Invalid(
                "Responses commands require Responses delegation".into(),
            ));
        }
        if let CommandGuard::Update(requested_mode) = command {
            if let Some(responses_mode) = self.responses_mode {
                if requested_mode.is_some_and(|mode| mode != responses_mode) {
                    return Err(Error::Invalid(
                        "delegation mode is immutable; null selects client".into(),
                    ));
                }
            }
        }
        if matches!(command, CommandGuard::Context { attributed: true })
            && self.responses_mode == Some(true)
        {
            return Err(Error::Invalid(
                "context requires a client delegation, not a Responses delegation".into(),
            ));
        }
        Ok(())
    }

    fn prepare(&mut self, guard: &CommandGuard) -> Result<()> {
        self.validate(guard)?;
        if matches!(guard, CommandGuard::Start) {
            self.started_sent = true;
        }
        if matches!(guard, CommandGuard::Close) {
            self.phase.send_replace(SessionPhase::Closing);
        }
        Ok(())
    }

    fn observe(&mut self, frame: &ServerFrame) -> bool {
        let startup_error = *self.phase.borrow() == SessionPhase::Starting
            && matches!(frame.event, ServerEvent::Error { .. });
        if let ServerEvent::Started { session, .. } = &frame.event {
            if self.responses_mode.is_none() && !session.delegation.is_absent() {
                self.responses_mode = Some(matches!(
                    session.delegation,
                    Field::Value(DelegationConfig::Responses { .. })
                ));
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
                self.responses_mode
                    .get_or_insert(delegation.target == DelegationTarget::Responses);
            }
            _ => {}
        }
        if startup_error {
            self.phase.send_replace(SessionPhase::Disconnected);
        }
        matches!(frame.event, ServerEvent::Closed { .. }) || startup_error
    }
}

fn valid_upgrade(response: &reqwest::Response, key: &str) -> bool {
    let headers = response.headers();
    let single = |name| {
        let mut values = headers.get_all(name).iter();
        let value = values.next()?.to_str().ok()?;
        values.next().is_none().then_some(value)
    };
    let connection = headers
        .get_all("connection")
        .iter()
        .map(|value| value.to_str().ok())
        .collect::<Option<Vec<_>>>();
    let connection = connection.is_some_and(|values| {
        let tokens: Vec<_> = values
            .iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .collect();
        tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("upgrade"))
            && tokens.iter().all(|token| {
                !token.is_empty()
                    && token.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
                    })
            })
    });
    response.version() == reqwest::Version::HTTP_11
        && single("upgrade").is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && connection
        && single("sec-websocket-accept") == Some(derive_accept_key(key.as_bytes()).as_str())
        && !headers.contains_key("sec-websocket-protocol")
        && !headers.contains_key("sec-websocket-extensions")
}

fn rejected_upgrade(response: &reqwest::Response) -> Error {
    let header = |name| {
        response
            .headers()
            .get(name)
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned)
    };
    Error::Http {
        status: response.status().as_u16(),
        headers: Box::new(response.headers().clone()),
        request_id: header("x-request-id"),
        content_type: header("content-type"),
        retry_after: header("retry-after"),
        body: Vec::new(),
        body_issue: Some(HttpBodyIssue::Unconfirmed),
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
                if let Err(error) = state.prepare(&request.guard) {
                    let _ = request.completion.send(Err(error));
                    continue;
                }
                writing = Some(begin_write(writer.take().expect("idle writer"), Some(request.text), write_timeout, Some(request.completion)));
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

    #[tokio::test]
    async fn oversized_send_rejects_before_waiting_on_a_full_command_queue() {
        let (commands, mut requests) = mpsc::channel(1);
        let (_phase_tx, phase) = watch::channel(SessionPhase::Active);
        let sender = LiveSender {
            commands,
            phase,
            role: ConnectionRole::Primary,
            format: AudioFormat::default(),
            codec: Codec {
                max_event_bytes: 512,
            },
        };
        let (completion, _receipt) = oneshot::channel();
        sender
            .commands
            .send(WriteRequest {
                text: "{}".into(),
                guard: CommandGuard::Other,
                completion,
            })
            .await
            .unwrap();
        assert_eq!(sender.commands.capacity(), 0);
        let mut oversized = Box::pin(sender.send(ClientEvent::new(Command::ThinkingAppend {
            content: "x".repeat(128 * 1024),
            delegation_id: super::super::Nullable(None),
        })));
        assert!(matches!(
            futures::poll!(&mut oversized),
            std::task::Poll::Ready(Err(Error::Invalid(_)))
        ));
        assert_eq!(
            sender.commands.capacity(),
            0,
            "oversized command must not enter the queue"
        );
        let mut valid = Box::pin(sender.send(ClientEvent::new(Command::ThinkingAppend {
            content: "valid".into(),
            delegation_id: super::super::Nullable(None),
        })));
        assert!(futures::poll!(&mut valid).is_pending());
        requests.recv().await.unwrap();
        assert!(futures::poll!(&mut valid).is_pending());
        let queued = requests.recv().await.unwrap();
        assert!(queued.text.len() <= 512);
        assert!(queued.text.capacity() <= 512);
        assert!(matches!(
            queued.guard,
            CommandGuard::Context { attributed: false }
        ));
        queued.completion.send(Ok(())).unwrap();
        valid.await.unwrap();
    }

    #[test]
    fn queued_guards_use_latest_mode_and_phase_at_dequeue() {
        let (phase, _receiver) = watch::channel(SessionPhase::Active);
        let mut state = DriverState {
            role: ConnectionRole::Sideband,
            responses_mode: None,
            phase,
            started_sent: true,
        };
        let guard = CommandGuard::Responses;
        state.validate(&guard).unwrap();
        let frame = Codec::default()
            .decode_server(
                r#"{
            "type":"session.started","event_id":"e","session":{
                "id":"s","model":"gpt-live-1","status":"active","expires_at":1,"delegation":null
            }
        }"#,
            )
            .unwrap();
        state.observe(&frame);
        assert!(matches!(state.prepare(&guard), Err(Error::Invalid(_))));
        state.phase.send_replace(SessionPhase::Closing);
        assert!(matches!(
            state.prepare(&CommandGuard::Other),
            Err(Error::Closed)
        ));
    }
}
