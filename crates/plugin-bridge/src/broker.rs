use std::collections::HashMap;
use std::io;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::protocol::{
    BgraImage, Message, ProtocolError, RemoteErrorCode, read_message, write_message,
};

pub type RequestId = u64;

const MAX_TRACKED_REQUESTS: usize = 64;
const READER_QUEUE_CAPACITY: usize = 8;
const MIN_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const DISCONNECT_EXIT_GRACE: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub struct BrokerOptions {
    request_timeout: Duration,
    initial_generation: u64,
    max_pending_requests: usize,
}

impl BrokerOptions {
    #[must_use]
    pub fn new(request_timeout: Duration) -> Self {
        Self {
            request_timeout,
            initial_generation: 0,
            max_pending_requests: MAX_TRACKED_REQUESTS,
        }
    }

    #[must_use]
    pub const fn with_initial_generation(mut self, generation: u64) -> Self {
        self.initial_generation = generation;
        self
    }

    #[must_use]
    pub fn with_max_pending_requests(mut self, maximum: usize) -> Self {
        self.max_pending_requests = maximum;
        self
    }
}

#[derive(Debug, Error)]
pub enum HostError {
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error("plugin child process operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("plugin request {request_id} exceeded its {timeout:?} timeout")]
    Timeout {
        request_id: RequestId,
        timeout: Duration,
    },
    #[error("plugin request {request_id} was cancelled")]
    Cancelled { request_id: RequestId },
    #[error("stale document generation {actual}; broker is at generation {expected}")]
    StaleGeneration { expected: u64, actual: u64 },
    #[error("stale response for request {request_id}: generation {actual}, expected {expected}")]
    StaleResponse {
        request_id: RequestId,
        expected: u64,
        actual: u64,
    },
    #[error("document generation must increase: current {current}, requested {requested}")]
    NonMonotonicGeneration { current: u64, requested: u64 },
    #[error("plugin host returned {code:?} for request {request_id}: {message}")]
    Remote {
        request_id: RequestId,
        code: RemoteErrorCode,
        message: String,
    },
    #[error("plugin child exited unexpectedly with code {code:?}")]
    ChildExited { code: Option<i32> },
    #[error("plugin child closed its protocol stream unexpectedly")]
    TransportDisconnected,
    #[error("plugin host sent an unexpected message: {0}")]
    UnexpectedMessage(String),
    #[error("plugin response has unknown or already completed request ID {0}")]
    UnexpectedRequestId(RequestId),
    #[error("too many outstanding plugin requests; the configured bound is {0}")]
    TooManyPending(usize),
    #[error("plugin request ID space was exhausted")]
    RequestIdExhausted,
    #[error("plugin broker timeout must be nonzero")]
    InvalidTimeout,
    #[error("plugin broker pending-request bound must be in 1..={MAX_TRACKED_REQUESTS}")]
    InvalidPendingLimit,
    #[error(
        "plugin response dimensions {actual_width}x{actual_height} do not match request dimensions {expected_width}x{expected_height}"
    )]
    DimensionMismatch {
        expected_width: u32,
        expected_height: u32,
        actual_width: u32,
        actual_height: u32,
    },
}

#[derive(Debug)]
struct PendingRequest {
    generation: u64,
    width: u32,
    height: u32,
}

struct WriterCommand {
    message: Message,
    result: SyncSender<Result<(), ProtocolError>>,
}

enum ReaderEvent {
    Message(Message),
    Error(ProtocolError),
}

/// A supervised, framed connection to one isolated plugin child process.
///
/// Calls are serialized through `&mut self`, while reader and writer threads keep
/// blocked pipes from defeating request deadlines. A timeout is fail-stop: the
/// child is killed and reaped so a late result can never be delivered to a later
/// request. Explicit cancellation is cooperative and permits later requests.
#[derive(Debug)]
pub struct PluginHost {
    child: Child,
    writer: Option<SyncSender<WriterCommand>>,
    writer_thread: Option<JoinHandle<()>>,
    events: Option<Receiver<ReaderEvent>>,
    reader_thread: Option<JoinHandle<()>>,
    request_timeout: Duration,
    max_pending_requests: usize,
    next_request_id: RequestId,
    generation: u64,
    effects: Vec<String>,
    pending: HashMap<RequestId, PendingRequest>,
    cancelled: HashMap<RequestId, u64>,
    inbox: HashMap<RequestId, Message>,
    rejected_stale_responses: u64,
    terminated: bool,
    exit_code: Option<i32>,
}

impl PluginHost {
    pub fn spawn(mut command: Command, options: BrokerOptions) -> Result<Self, HostError> {
        if options.request_timeout.is_zero() {
            return Err(HostError::InvalidTimeout);
        }
        if options.max_pending_requests == 0 || options.max_pending_requests > MAX_TRACKED_REQUESTS
        {
            return Err(HostError::InvalidPendingLimit);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Never let an inherited or piped stderr become an unbounded protocol
            // side channel. Operators can configure command-level diagnostics.
            .stderr(Stdio::null())
            .spawn()?;
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HostError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "plugin child stdin was not piped",
            )));
        };
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HostError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "plugin child stdout was not piped",
            )));
        };

        let (writer_sender, writer_receiver) = mpsc::sync_channel::<WriterCommand>(1);
        let writer_thread = match thread::Builder::new()
            .name("paint-plugin-writer".to_owned())
            .spawn(move || {
                let mut stdin = stdin;
                while let Ok(command) = writer_receiver.recv() {
                    let result = write_message(&mut stdin, &command.message);
                    let failed = result.is_err();
                    if command.result.send(result).is_err() || failed {
                        break;
                    }
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        };

        let (event_sender, event_receiver) =
            mpsc::sync_channel::<ReaderEvent>(READER_QUEUE_CAPACITY);
        let reader_thread = match thread::Builder::new()
            .name("paint-plugin-reader".to_owned())
            .spawn(move || {
                let mut stdout = stdout;
                loop {
                    match read_message(&mut stdout) {
                        Ok(message) => {
                            if event_sender.send(ReaderEvent::Message(message)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = event_sender.send(ReaderEvent::Error(error));
                            break;
                        }
                    }
                }
            }) {
            Ok(thread) => thread,
            Err(error) => {
                drop(writer_sender);
                let _ = writer_thread.join();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        };

        let mut host = Self {
            child,
            writer: Some(writer_sender),
            writer_thread: Some(writer_thread),
            events: Some(event_receiver),
            reader_thread: Some(reader_thread),
            request_timeout: options.request_timeout,
            max_pending_requests: options.max_pending_requests,
            next_request_id: 1,
            generation: options.initial_generation,
            effects: Vec::new(),
            pending: HashMap::new(),
            cancelled: HashMap::new(),
            inbox: HashMap::new(),
            rejected_stale_responses: 0,
            terminated: false,
            exit_code: None,
        };
        // Starting a runtime and compiling/loading a source-built effect can be
        // slower than an intentionally tiny per-request timeout. Keep a separate
        // bounded startup window without weakening the subsequent request deadline.
        let request_timeout = host.request_timeout;
        host.request_timeout = request_timeout.max(MIN_HANDSHAKE_TIMEOUT);
        let handshake_result = host.handshake();
        host.request_timeout = request_timeout;
        if let Err(error) = handshake_result {
            let _ = host.terminate_internal();
            return Err(error);
        }
        Ok(host)
    }

    fn handshake(&mut self) -> Result<(), HostError> {
        let request_id = self.allocate_request_id()?;
        self.send_message(Message::Hello { request_id })?;
        let deadline = Instant::now() + self.request_timeout;
        let event = self.receive_event(deadline, request_id)?;
        match event {
            ReaderEvent::Message(Message::Ready {
                request_id: received_id,
                effects,
            }) if received_id == request_id => {
                self.effects = effects;
                Ok(())
            }
            ReaderEvent::Message(message) => Err(self.fail_with(HostError::UnexpectedMessage(
                format!("expected Ready for handshake request {request_id}, got {message:?}"),
            ))),
            ReaderEvent::Error(error) => Err(self.handle_reader_error(error)),
        }
    }

    #[must_use]
    pub fn effects(&self) -> &[String] {
        &self.effects
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub const fn rejected_stale_responses(&self) -> u64 {
        self.rejected_stale_responses
    }

    pub fn advance_generation(&mut self, generation: u64) -> Result<(), HostError> {
        if generation <= self.generation {
            return Err(HostError::NonMonotonicGeneration {
                current: self.generation,
                requested: generation,
            });
        }
        self.generation = generation;
        let active: Vec<_> = self.pending.keys().copied().collect();
        for request_id in active {
            self.cancel(request_id)?;
        }
        Ok(())
    }

    pub fn apply(
        &mut self,
        generation: u64,
        effect: &str,
        image: &BgraImage,
    ) -> Result<BgraImage, HostError> {
        let request_id = self.start_apply(generation, effect, image)?;
        self.wait_for_apply(request_id)
    }

    pub fn start_apply(
        &mut self,
        generation: u64,
        effect: &str,
        image: &BgraImage,
    ) -> Result<RequestId, HostError> {
        if generation != self.generation {
            return Err(HostError::StaleGeneration {
                expected: self.generation,
                actual: generation,
            });
        }
        if self.terminated {
            return Err(HostError::ChildExited {
                code: self.exit_code,
            });
        }
        if self.pending.len() + self.cancelled.len() >= self.max_pending_requests {
            return Err(HostError::TooManyPending(self.max_pending_requests));
        }
        let request_id = self.allocate_request_id()?;
        let message = Message::Apply {
            request_id,
            generation,
            effect: effect.to_owned(),
            // This explicit copy is the v1 ownership boundary. The caller's
            // surface is never shared with or mutated by the child.
            image: image.clone(),
        };
        self.send_message(message)?;
        self.pending.insert(
            request_id,
            PendingRequest {
                generation,
                width: image.width(),
                height: image.height(),
            },
        );
        Ok(request_id)
    }

    pub fn cancel(&mut self, request_id: RequestId) -> Result<(), HostError> {
        if self.cancelled.contains_key(&request_id) {
            return Ok(());
        }
        let Some(pending) = self.pending.remove(&request_id) else {
            return Err(HostError::UnexpectedRequestId(request_id));
        };
        if self.cancelled.len() >= self.max_pending_requests {
            self.pending.insert(request_id, pending);
            return Err(HostError::TooManyPending(self.max_pending_requests));
        }
        self.cancelled.insert(request_id, pending.generation);
        if let Err(error) = self.send_message(Message::Cancel { request_id }) {
            let _ = self.terminate_internal();
            return Err(error);
        }
        Ok(())
    }

    pub fn wait_for_apply(&mut self, request_id: RequestId) -> Result<BgraImage, HostError> {
        if self.cancelled.contains_key(&request_id) {
            return Err(HostError::Cancelled { request_id });
        }
        if !self.pending.contains_key(&request_id) {
            return Err(HostError::UnexpectedRequestId(request_id));
        }
        let deadline = Instant::now() + self.request_timeout;
        loop {
            if let Some(message) = self.inbox.remove(&request_id) {
                return self.complete_apply(request_id, message);
            }
            if self.cancelled.contains_key(&request_id) {
                return Err(HostError::Cancelled { request_id });
            }
            match self.receive_event(deadline, request_id)? {
                ReaderEvent::Message(message) => self.route_message(message)?,
                ReaderEvent::Error(error) => return Err(self.handle_reader_error(error)),
            }
        }
    }

    fn route_message(&mut self, message: Message) -> Result<(), HostError> {
        let Some((request_id, generation)) = response_identity(&message) else {
            return Err(self.fail_with(HostError::UnexpectedMessage(format!(
                "only apply responses are valid during a request, got {message:?}"
            ))));
        };
        if let Some(expected_generation) = self.cancelled.get(&request_id).copied() {
            if generation != expected_generation {
                return Err(self.fail_with(HostError::StaleResponse {
                    request_id,
                    expected: expected_generation,
                    actual: generation,
                }));
            }
            // Cancellation and successful completion race by design. Consume
            // exactly one late response, but never deliver it as current output.
            self.cancelled.remove(&request_id);
            self.rejected_stale_responses = self.rejected_stale_responses.saturating_add(1);
            return Ok(());
        }
        if !self.pending.contains_key(&request_id) {
            return Err(self.fail_with(HostError::UnexpectedRequestId(request_id)));
        }
        self.inbox.insert(request_id, message);
        Ok(())
    }

    fn complete_apply(
        &mut self,
        request_id: RequestId,
        message: Message,
    ) -> Result<BgraImage, HostError> {
        let Some(pending) = self.pending.remove(&request_id) else {
            return Err(HostError::UnexpectedRequestId(request_id));
        };
        let generation = response_identity(&message)
            .map(|(_, generation)| generation)
            .ok_or_else(|| HostError::UnexpectedMessage(format!("{message:?}")))?;
        if generation != pending.generation {
            return Err(self.fail_with(HostError::StaleResponse {
                request_id,
                expected: pending.generation,
                actual: generation,
            }));
        }
        match message {
            Message::ApplyResult {
                generation, image, ..
            } => {
                if image.width() != pending.width || image.height() != pending.height {
                    return Err(self.fail_with(HostError::DimensionMismatch {
                        expected_width: pending.width,
                        expected_height: pending.height,
                        actual_width: image.width(),
                        actual_height: image.height(),
                    }));
                }
                if generation != self.generation {
                    return Err(self.fail_with(HostError::StaleResponse {
                        request_id,
                        expected: self.generation,
                        actual: generation,
                    }));
                }
                Ok(image)
            }
            Message::Error {
                generation: _,
                code: RemoteErrorCode::Cancelled,
                ..
            } => Err(HostError::Cancelled { request_id }),
            Message::Error { code, message, .. } => Err(HostError::Remote {
                request_id,
                code,
                message,
            }),
            other => Err(self.fail_with(HostError::UnexpectedMessage(format!(
                "expected ApplyResult or Error, got {other:?}"
            )))),
        }
    }

    fn receive_event(
        &mut self,
        deadline: Instant,
        request_id: RequestId,
    ) -> Result<ReaderEvent, HostError> {
        if self.events.is_none() {
            return Err(self.handle_disconnect());
        }
        let events = self
            .events
            .as_ref()
            .ok_or(HostError::TransportDisconnected)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(remaining) {
            Ok(event) => Ok(event),
            Err(RecvTimeoutError::Timeout) => {
                let error = HostError::Timeout {
                    request_id,
                    timeout: self.request_timeout,
                };
                let _ = self.terminate_internal();
                self.pending.remove(&request_id);
                Err(error)
            }
            Err(RecvTimeoutError::Disconnected) => Err(self.handle_disconnect()),
        }
    }

    fn handle_reader_error(&mut self, error: ProtocolError) -> HostError {
        if matches!(error, ProtocolError::Eof) {
            return self.handle_disconnect();
        }
        self.fail_with(HostError::Protocol(error))
    }

    fn handle_disconnect(&mut self) -> HostError {
        // Pipe closure and waitable child status are not synchronized on every OS.
        // Give an exiting child a bounded grace period, but never wait forever for
        // a process that closed its protocol stream and continued running.
        let deadline = Instant::now() + DISCONNECT_EXIT_GRACE;
        let exited = if self.terminated {
            true
        } else {
            loop {
                match self.child.try_wait() {
                    Ok(Some(status)) => {
                        self.exit_code = status.code();
                        self.terminated = true;
                        break true;
                    }
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Ok(None) => break false,
                    Err(error) => {
                        let _ = self.terminate_internal();
                        return error.into();
                    }
                }
            }
        };
        let _ = self.terminate_internal();
        if exited {
            HostError::ChildExited {
                code: self.exit_code,
            }
        } else {
            HostError::TransportDisconnected
        }
    }

    fn fail_with(&mut self, error: HostError) -> HostError {
        let _ = self.terminate_internal();
        error
    }

    fn send_message(&mut self, message: Message) -> Result<(), HostError> {
        if self.terminated {
            return Err(HostError::ChildExited {
                code: self.exit_code,
            });
        }
        let request_id = message_identity(&message);
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let writer = self
            .writer
            .as_ref()
            .ok_or(HostError::TransportDisconnected)?;
        if let Err(error) = writer.try_send(WriterCommand {
            message,
            result: result_sender,
        }) {
            let message = match error {
                TrySendError::Full(_) => "plugin writer queue is full",
                TrySendError::Disconnected(_) => "plugin writer disconnected",
            };
            return Err(self.fail_with(HostError::Io(io::Error::new(
                io::ErrorKind::BrokenPipe,
                message,
            ))));
        }
        match result_receiver.recv_timeout(self.request_timeout) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(self.fail_with(HostError::Protocol(error))),
            Err(RecvTimeoutError::Timeout) => Err(self.fail_with(HostError::Timeout {
                request_id,
                timeout: self.request_timeout,
            })),
            Err(RecvTimeoutError::Disconnected) => Err(self.handle_disconnect()),
        }
    }

    fn allocate_request_id(&mut self) -> Result<RequestId, HostError> {
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(HostError::RequestIdExhausted)?;
        Ok(request_id)
    }

    pub fn is_running(&mut self) -> Result<bool, HostError> {
        if self.terminated {
            return Ok(false);
        }
        if let Some(status) = self.child.try_wait()? {
            self.terminated = true;
            self.exit_code = status.code();
            return Ok(false);
        }
        Ok(true)
    }

    /// Force-stop and reap the child. This is also used for fail-stop timeout
    /// handling and is public so embedders can respond to their own shutdown.
    pub fn terminate(&mut self) -> Result<(), HostError> {
        self.terminate_internal().map(|_| ())
    }

    fn terminate_internal(&mut self) -> Result<(), HostError> {
        if !self.terminated {
            match self.child.try_wait()? {
                Some(status) => {
                    self.exit_code = status.code();
                }
                None => {
                    match self.child.kill() {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
                        Err(error) => return Err(error.into()),
                    }
                    let status = self.child.wait()?;
                    self.exit_code = status.code();
                }
            }
            self.terminated = true;
        }
        self.reap_threads();
        Ok(())
    }

    fn reap_threads(&mut self) {
        self.writer.take();
        // Dropping the receiver releases a reader blocked on the bounded queue.
        self.events.take();
        if let Some(thread) = self.writer_thread.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.reader_thread.take() {
            let _ = thread.join();
        }
    }

    pub fn close(&mut self) -> Result<(), HostError> {
        if self.terminated {
            self.reap_threads();
            return Ok(());
        }
        let active: Vec<_> = self.pending.keys().copied().collect();
        for request_id in active {
            self.cancel(request_id)?;
        }
        let request_id = self.allocate_request_id()?;
        self.send_message(Message::Shutdown { request_id })?;
        let deadline = Instant::now() + self.request_timeout;
        loop {
            match self.receive_event(deadline, request_id)? {
                ReaderEvent::Message(Message::ShutdownAck {
                    request_id: received,
                }) if received == request_id => break,
                ReaderEvent::Message(message) => self.route_message(message)?,
                ReaderEvent::Error(error) => return Err(self.handle_reader_error(error)),
            }
        }
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                self.exit_code = status.code();
                self.terminated = true;
                self.reap_threads();
                return Ok(());
            }
            thread::sleep(Duration::from_millis(2));
        }
        let _ = self.terminate_internal();
        Err(HostError::Timeout {
            request_id,
            timeout: self.request_timeout,
        })
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        let _ = self.terminate_internal();
    }
}

fn message_identity(message: &Message) -> RequestId {
    match message {
        Message::Apply { request_id, .. }
        | Message::Cancel { request_id }
        | Message::ApplyResult { request_id, .. }
        | Message::Error { request_id, .. }
        | Message::Hello { request_id }
        | Message::Ready { request_id, .. }
        | Message::Shutdown { request_id }
        | Message::ShutdownAck { request_id } => *request_id,
    }
}

fn response_identity(message: &Message) -> Option<(RequestId, u64)> {
    match message {
        Message::ApplyResult {
            request_id,
            generation,
            ..
        }
        | Message::Error {
            request_id,
            generation,
            ..
        } => Some((*request_id, *generation)),
        _ => None,
    }
}
