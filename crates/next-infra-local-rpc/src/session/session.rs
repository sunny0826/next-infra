use std::fmt;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::protocol::{
    ClientHello, HandshakeResponse, HostHello, RequestEnvelope, ResponseEnvelope, RpcError,
    handshake_response, negotiate, validate_in_flight,
};
use crate::transport::{
    FramedError, SecureUnixListener, TransportError, TransportPaths, connect_secure,
    read_json_frame, verify_peer_uid, write_json_frame,
};

use super::QueryHandler;

#[derive(Debug)]
pub enum SessionError {
    Transport(TransportError),
    Frame(FramedError),
    Rejected(RpcError),
    Protocol(RpcError),
    WorkerPanicked,
    WriterPoisoned,
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => error.fmt(formatter),
            Self::Frame(error) => error.fmt(formatter),
            Self::Rejected(error) => write!(formatter, "handshake rejected: {error}"),
            Self::Protocol(error) => write!(formatter, "protocol error: {error}"),
            Self::WorkerPanicked => formatter.write_str("RPC query worker panicked"),
            Self::WriterPoisoned => formatter.write_str("RPC response writer is unavailable"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<TransportError> for SessionError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

impl From<FramedError> for SessionError {
    fn from(error: FramedError) -> Self {
        Self::Frame(error)
    }
}

pub struct RpcServer<H> {
    host: HostHello,
    handler: Arc<H>,
}

impl<H> RpcServer<H>
where
    H: QueryHandler,
{
    pub fn new(host: HostHello, handler: H) -> Self {
        Self {
            host,
            handler: Arc::new(handler),
        }
    }

    pub fn serve_once(&self, listener: &SecureUnixListener) -> Result<(), SessionError> {
        let (stream, _) = listener
            .accept()
            .map_err(TransportError::Socket)
            .map_err(SessionError::Transport)?;
        self.serve_stream(stream)
    }

    pub fn serve_stream(&self, stream: UnixStream) -> Result<(), SessionError> {
        verify_peer_uid(&stream).map_err(TransportError::Peer)?;
        let mut reader = stream.try_clone().map_err(FramedError::Io)?;
        let writer = Arc::new(Mutex::new(stream));

        let client = match read_json_frame::<_, ClientHello>(&mut reader) {
            Ok(client) => client,
            Err(error) => {
                let rejected = HandshakeResponse::rejected(frame_rpc_error(&error));
                write_locked(&writer, &rejected)?;
                return Err(SessionError::Frame(error));
            }
        };

        let host = selected_host(&self.host, &client);
        let response = handshake_response(&client, &host);
        write_locked(&writer, &response)?;
        let supported = match response {
            HandshakeResponse::Accepted { ref host, .. } => host.supported_capabilities.clone(),
            HandshakeResponse::Rejected { error } => return Err(SessionError::Rejected(error)),
        };

        let in_flight = Arc::new(AtomicUsize::new(0));
        let mut workers: Vec<JoinHandle<Result<(), SessionError>>> = Vec::new();

        loop {
            reap_finished(&mut workers)?;
            let request = match read_json_frame::<_, RequestEnvelope>(&mut reader) {
                Ok(request) => request,
                Err(FramedError::Eof {
                    expected: 4,
                    actual: 0,
                }) => break,
                Err(error) => return Err(SessionError::Frame(error)),
            };

            if !supported.contains(request.query.capability()) {
                let error = RpcError::capability_mismatch(
                    "the negotiated Host does not support this query capability",
                );
                let response = ResponseEnvelope::error(request.request_id, error)
                    .map_err(SessionError::Protocol)?;
                write_locked(&writer, &response)?;
                continue;
            }

            if let Err(error) = validate_in_flight(in_flight.load(Ordering::Acquire)) {
                let response = ResponseEnvelope::error(request.request_id, error)
                    .map_err(SessionError::Protocol)?;
                write_locked(&writer, &response)?;
                continue;
            }

            in_flight.fetch_add(1, Ordering::AcqRel);
            let handler = Arc::clone(&self.handler);
            let writer = Arc::clone(&writer);
            let in_flight = Arc::clone(&in_flight);
            workers.push(thread::spawn(move || {
                let _permit = InFlightPermit(in_flight);
                handle_request(handler.as_ref(), request, &writer)
            }));
        }

        join_all(workers)
    }
}

struct InFlightPermit(Arc<AtomicUsize>);

impl Drop for InFlightPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct RpcClient {
    stream: UnixStream,
    host: HostHello,
    upgrade_recommended: bool,
}

impl RpcClient {
    pub fn connect(paths: &TransportPaths, client: &ClientHello) -> Result<Self, SessionError> {
        let stream = connect_secure(paths)?;
        Self::handshake(stream, client)
    }

    pub fn handshake(mut stream: UnixStream, client: &ClientHello) -> Result<Self, SessionError> {
        verify_peer_uid(&stream).map_err(TransportError::Peer)?;
        write_json_frame(&mut stream, client)?;
        let response: HandshakeResponse = read_json_frame(&mut stream)?;
        match response {
            HandshakeResponse::Accepted {
                host,
                upgrade_recommended,
            } => {
                let result = negotiate(client, &host).map_err(SessionError::Protocol)?;
                if result.upgrade_recommended != upgrade_recommended {
                    return Err(SessionError::Protocol(RpcError::protocol_mismatch(
                        "Host upgrade recommendation does not match negotiated releases",
                    )));
                }
                Ok(Self {
                    stream,
                    host,
                    upgrade_recommended,
                })
            }
            HandshakeResponse::Rejected { error } => Err(SessionError::Rejected(error)),
        }
    }

    pub fn query(&mut self, request: &RequestEnvelope) -> Result<ResponseEnvelope, SessionError> {
        write_json_frame(&mut self.stream, request)?;
        let response: ResponseEnvelope = read_json_frame(&mut self.stream)?;
        if response.request_id != request.request_id {
            return Err(SessionError::Protocol(RpcError::invalid_frame(
                "response request_id does not match the request",
            )));
        }
        Ok(response)
    }

    pub fn host(&self) -> &HostHello {
        &self.host
    }

    pub fn upgrade_recommended(&self) -> bool {
        self.upgrade_recommended
    }
}

fn selected_host(template: &HostHello, client: &ClientHello) -> HostHello {
    let mut host = template.clone();
    host.selected_protocol_minor = client.protocol_minor.min(host.protocol_minor);
    host
}

fn handle_request<H: QueryHandler>(
    handler: &H,
    request: RequestEnvelope,
    writer: &Arc<Mutex<UnixStream>>,
) -> Result<(), SessionError> {
    let request_id = request.request_id;
    let response = match handler.handle(request.query) {
        Ok(response) => ResponseEnvelope::success(request_id, response),
        Err(error) => ResponseEnvelope::error(request_id, error),
    }
    .map_err(SessionError::Protocol)?;
    write_locked(writer, &response)
}

fn write_locked<T: serde::Serialize>(
    writer: &Arc<Mutex<UnixStream>>,
    value: &T,
) -> Result<(), SessionError> {
    let mut writer = writer.lock().map_err(|_| SessionError::WriterPoisoned)?;
    write_json_frame(&mut *writer, value).map_err(SessionError::Frame)
}

fn reap_finished(
    workers: &mut Vec<JoinHandle<Result<(), SessionError>>>,
) -> Result<(), SessionError> {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.swap_remove(index);
            worker.join().map_err(|_| SessionError::WorkerPanicked)??;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn join_all(workers: Vec<JoinHandle<Result<(), SessionError>>>) -> Result<(), SessionError> {
    for worker in workers {
        worker.join().map_err(|_| SessionError::WorkerPanicked)??;
    }
    Ok(())
}

fn frame_rpc_error(error: &FramedError) -> RpcError {
    if error.is_oversized() {
        RpcError::oversized_frame("handshake frame exceeds the 1 MiB maximum")
    } else {
        RpcError::invalid_frame("a valid ClientHello frame is required before queries")
    }
}
