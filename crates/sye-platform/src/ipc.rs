//! Single-instance local IPC server and client using `interprocess` local sockets.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use interprocess::local_socket::{
    GenericFilePath, Listener, ListenerNonblockingMode, ListenerOptions, Name, Stream, ToFsName,
    traits::{Listener as _, Stream as _},
};
#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use sye_core::PlatformError;
use sye_core::ipc::{
    IpcCodecError, IpcCommand, IpcErrorCode, IpcResponse, MAX_IPC_LINE_BYTES, decode_command,
    decode_response, encode_line,
};

/// Default read/write timeout applied to IPC client and server connections.
pub const DEFAULT_IPC_TIMEOUT: Duration = Duration::from_secs(2);

/// Error returned by local IPC server or client operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcError {
    AlreadyRunning,
    NotRunning,
    Codec(IpcCodecError),
    Io(String),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "an instance is already listening on the IPC socket"),
            Self::NotRunning => write!(f, "no daemon is listening on the IPC socket"),
            Self::Codec(err) => write!(f, "IPC codec error: {err}"),
            Self::Io(msg) => write!(f, "IPC I/O error: {msg}"),
        }
    }
}

impl std::error::Error for IpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(err) => Some(err),
            Self::AlreadyRunning | Self::NotRunning | Self::Io(_) => None,
        }
    }
}

impl From<IpcCodecError> for IpcError {
    fn from(err: IpcCodecError) -> Self {
        Self::Codec(err)
    }
}

impl From<IpcError> for PlatformError {
    fn from(err: IpcError) -> Self {
        match err {
            IpcError::AlreadyRunning | IpcError::NotRunning => Self::Unavailable(err.to_string()),
            IpcError::Codec(codec) => Self::Io(codec.to_string()),
            IpcError::Io(msg) => Self::Io(msg),
        }
    }
}

/// Single-instance local IPC server bound to a filesystem socket (Unix) or named pipe (Windows).
pub struct IpcServer {
    endpoint: PathBuf,
    listener: Listener,
}

impl fmt::Debug for IpcServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IpcServer")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl IpcServer {
    /// Binds a single-instance IPC listener at `endpoint`.
    ///
    /// On Unix, probes any pre-existing socket file: returns `Err(IpcError::AlreadyRunning)` if
    /// another daemon responds, or removes a stale socket file before binding.
    pub fn bind(endpoint: &Path) -> Result<Self, IpcError> {
        #[cfg(unix)]
        {
            prepare_unix_socket_path(endpoint)?;
        }

        let name = to_local_socket_name(endpoint)?;
        let listener = ListenerOptions::new()
            .name(name)
            .reclaim_name(true)
            .create_sync()
            .map_err(|err| {
                if err.kind() == io::ErrorKind::AddrInUse
                    || (cfg!(windows) && err.kind() == io::ErrorKind::PermissionDenied)
                {
                    IpcError::AlreadyRunning
                } else {
                    IpcError::Io(err.to_string())
                }
            })?;

        Ok(Self {
            endpoint: endpoint.to_path_buf(),
            listener,
        })
    }

    /// Returns the socket path or pipe endpoint bound by this server.
    #[must_use]
    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }

    /// Configures non-blocking mode for accepting connections.
    pub fn set_nonblocking(&self, mode: ListenerNonblockingMode) -> Result<(), IpcError> {
        self.listener
            .set_nonblocking(mode)
            .map_err(|err| IpcError::Io(err.to_string()))
    }

    /// Configures the listener so `accept_once` returns `Ok(false)` immediately when no client
    /// is queued, while accepted client streams remain blocking with timeouts.
    pub fn set_accept_nonblocking(&self) -> Result<(), IpcError> {
        self.set_nonblocking(ListenerNonblockingMode::Accept)
    }

    /// Accepts a single client connection, decodes one bounded NDJSON command, invokes `handler`,
    /// and writes the NDJSON response.
    ///
    /// Returns `Ok(false)` when the listener is non-blocking and no client is queued.
    pub fn accept_once<F>(&self, timeout: Duration, handler: F) -> Result<bool, IpcError>
    where
        F: FnOnce(IpcCommand) -> IpcResponse,
    {
        let mut stream = match self.listener.accept() {
            Ok(stream) => stream,
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(err) => return Err(IpcError::Io(err.to_string())),
        };

        configure_stream_timeouts(&stream, timeout);

        let response = match read_bounded_line(&mut stream) {
            Ok(BoundedLine::Valid(line)) => match decode_command(&line) {
                Ok(cmd) => handler(cmd),
                Err(err_resp) => err_resp,
            },
            Ok(BoundedLine::TooLong(len)) => IpcResponse::Error {
                code: IpcErrorCode::InvalidRequest,
                message: format!("IPC line exceeds {MAX_IPC_LINE_BYTES} bytes ({len} bytes read)"),
            },
            Ok(BoundedLine::InvalidUtf8) => IpcResponse::Error {
                code: IpcErrorCode::InvalidRequest,
                message: "IPC request is not valid UTF-8".to_owned(),
            },
            Err(err) => IpcResponse::Error {
                code: IpcErrorCode::InvalidRequest,
                message: format!("failed to read IPC request: {err}"),
            },
        };

        let encoded = encode_line(&response)?;
        stream
            .write_all(encoded.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|err| IpcError::Io(err.to_string()))?;

        Ok(true)
    }
}

/// Stateless local IPC client for sending a single command to the running daemon.
#[derive(Debug, Default, Clone, Copy)]
pub struct IpcClient;

impl IpcClient {
    /// Sends `command` to `endpoint` with [`DEFAULT_IPC_TIMEOUT`] and returns the response.
    pub fn send_command(endpoint: &Path, command: &IpcCommand) -> Result<IpcResponse, IpcError> {
        Self::send_command_with_timeout(endpoint, command, DEFAULT_IPC_TIMEOUT)
    }

    /// Sends `command` to `endpoint` with an explicit read/write timeout.
    pub fn send_command_with_timeout(
        endpoint: &Path,
        command: &IpcCommand,
        timeout: Duration,
    ) -> Result<IpcResponse, IpcError> {
        let name = to_local_socket_name(endpoint)?;
        let mut stream = Stream::connect(name).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => IpcError::NotRunning,
            _ => IpcError::Io(err.to_string()),
        })?;

        configure_stream_timeouts(&stream, timeout);

        let line = encode_line(command)?;
        stream
            .write_all(line.as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|err| IpcError::Io(err.to_string()))?;

        match read_bounded_line(&mut stream).map_err(|err| IpcError::Io(err.to_string()))? {
            BoundedLine::Valid(resp_line) => Ok(decode_response(&resp_line)?),
            BoundedLine::TooLong(len) => Err(IpcError::Codec(IpcCodecError::LineTooLong {
                len,
                max: MAX_IPC_LINE_BYTES,
            })),
            BoundedLine::InvalidUtf8 => Err(IpcError::Codec(IpcCodecError::Deserialize(
                "IPC response is not valid UTF-8".to_owned(),
            ))),
        }
    }
}

enum BoundedLine {
    Valid(String),
    TooLong(usize),
    InvalidUtf8,
}

fn read_bounded_line(stream: &mut Stream) -> io::Result<BoundedLine> {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::with_capacity(256);
    let max_read = u64::try_from(MAX_IPC_LINE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    reader.by_ref().take(max_read).read_until(b'\n', &mut buf)?;

    if buf.len() > MAX_IPC_LINE_BYTES {
        return Ok(BoundedLine::TooLong(buf.len()));
    }
    match String::from_utf8(buf) {
        Ok(line) => Ok(BoundedLine::Valid(line)),
        Err(_) => Ok(BoundedLine::InvalidUtf8),
    }
}

fn configure_stream_timeouts(stream: &Stream, timeout: Duration) {
    // Windows named pipes return ErrorKind::Unsupported for stream timeouts.
    let _ = stream.set_recv_timeout(Some(timeout));
    let _ = stream.set_send_timeout(Some(timeout));
}

#[cfg(unix)]
fn prepare_unix_socket_path(endpoint: &Path) -> Result<(), IpcError> {
    if let Some(parent) = endpoint.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|err| IpcError::Io(err.to_string()))?;
    }

    if endpoint.symlink_metadata().is_ok() {
        let name = to_local_socket_name(endpoint)?;
        match Stream::connect(name) {
            Ok(_live) => return Err(IpcError::AlreadyRunning),
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
                ) =>
            {
                match std::fs::remove_file(endpoint) {
                    Ok(()) => {}
                    Err(rm_err) if rm_err.kind() == io::ErrorKind::NotFound => {}
                    Err(rm_err) => return Err(IpcError::Io(rm_err.to_string())),
                }
            }
            Err(err) => return Err(IpcError::Io(err.to_string())),
        }
    }
    Ok(())
}

fn to_local_socket_name(endpoint: &Path) -> Result<Name<'static>, IpcError> {
    #[cfg(unix)]
    {
        let resolved = if endpoint.as_os_str().len() >= 100
            && let Ok(cwd) = std::env::current_dir()
            && let Ok(rel) = endpoint.strip_prefix(&cwd)
            && !rel.as_os_str().is_empty()
        {
            rel.to_path_buf()
        } else {
            endpoint.to_path_buf()
        };
        resolved
            .to_fs_name::<GenericFilePath>()
            .map_err(|err| IpcError::Io(err.to_string()))
    }
    #[cfg(windows)]
    {
        let raw = endpoint.to_string_lossy();
        if raw.starts_with(r"\\.\pipe\") {
            endpoint
                .to_path_buf()
                .to_fs_name::<GenericFilePath>()
                .map_err(|err| IpcError::Io(err.to_string()))
        } else {
            let sanitized: String = raw
                .chars()
                .map(|c| {
                    if matches!(c, '\\' | '/' | ':' | ' ') {
                        '_'
                    } else {
                        c
                    }
                })
                .collect();
            sanitized
                .to_ns_name::<GenericNamespaced>()
                .map_err(|err| IpcError::Io(err.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use sye_core::{BreakType, SchedulerState, StatusSnapshot};

    use super::*;
    use crate::test_util::TestTempDir;

    #[test]
    fn ipc_round_trip_multiple_commands() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("rt")?;
        let sock = tmp.path().join("s.sock");
        let server = IpcServer::bind(&sock)?;

        let handle = thread::spawn(move || -> Result<(), IpcError> {
            for _ in 0..3 {
                let handled = server.accept_once(DEFAULT_IPC_TIMEOUT, |cmd| match cmd {
                    IpcCommand::Status => IpcResponse::Status(StatusSnapshot {
                        state: SchedulerState::Waiting,
                        enabled: true,
                        next_break_type: Some(BreakType::Short),
                        next_break_name: Some("Blink".to_owned()),
                        seconds_until_next_break: 120,
                        consecutive_skips: 0,
                    }),
                    IpcCommand::Disable { minutes } => IpcResponse::Ok {
                        message: Some(format!("disabled:{minutes:?}")),
                    },
                    IpcCommand::Quit => IpcResponse::Ok {
                        message: Some("bye".to_owned()),
                    },
                    _ => IpcResponse::Ok { message: None },
                })?;
                if !handled {
                    return Err(IpcError::Io("unexpected WouldBlock".to_owned()));
                }
            }
            Ok(())
        });

        let resp1 = IpcClient::send_command(&sock, &IpcCommand::Status)?;
        assert!(matches!(resp1, IpcResponse::Status(ref s) if s.enabled));

        let resp2 = IpcClient::send_command(&sock, &IpcCommand::Disable { minutes: Some(30) })?;
        assert_eq!(
            resp2,
            IpcResponse::Ok {
                message: Some("disabled:Some(30)".to_owned())
            }
        );

        let resp3 = IpcClient::send_command(&sock, &IpcCommand::Quit)?;
        assert_eq!(
            resp3,
            IpcResponse::Ok {
                message: Some("bye".to_owned())
            }
        );

        match handle.join() {
            Ok(res) => res?,
            Err(_) => return Err("server thread panicked".into()),
        }
        Ok(())
    }

    #[test]
    fn ipc_single_instance_rejects_second_bind() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("si")?;
        let sock = tmp.path().join("s.sock");
        let _server = IpcServer::bind(&sock)?;

        let second = IpcServer::bind(&sock);
        assert_eq!(second.err(), Some(IpcError::AlreadyRunning));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn ipc_recovers_from_stale_unix_socket() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("st")?;
        let sock = tmp.path().join("s.sock");

        // Create a real Unix socket and drop the listener without removing the file.
        let raw_listener = std::os::unix::net::UnixListener::bind(&sock)?;
        drop(raw_listener);
        assert!(sock.exists());

        let server = IpcServer::bind(&sock)?;
        assert_eq!(server.endpoint(), sock.as_path());
        Ok(())
    }

    #[test]
    fn ipc_client_reports_not_running_for_missing_socket() -> Result<(), Box<dyn std::error::Error>>
    {
        let tmp = TestTempDir::new("mi")?;
        let sock = tmp.path().join("s.sock");

        let err = IpcClient::send_command(&sock, &IpcCommand::Status).err();
        assert_eq!(err, Some(IpcError::NotRunning));
        Ok(())
    }

    #[test]
    fn ipc_server_rejects_oversized_line_without_crashing() -> Result<(), Box<dyn std::error::Error>>
    {
        let tmp = TestTempDir::new("ov")?;
        let sock = tmp.path().join("s.sock");
        let server = IpcServer::bind(&sock)?;

        let handle = thread::spawn(move || -> Result<(), IpcError> {
            let _ =
                server.accept_once(DEFAULT_IPC_TIMEOUT, |_| IpcResponse::Ok { message: None })?;
            Ok(())
        });

        let name = to_local_socket_name(&sock)?;
        let mut raw_client = Stream::connect(name)?;
        configure_stream_timeouts(&raw_client, DEFAULT_IPC_TIMEOUT);
        let mut payload = vec![b'x'; MAX_IPC_LINE_BYTES + 64];
        payload.push(b'\n');
        let _ = raw_client.write_all(&payload);
        let _ = raw_client.flush();

        let BoundedLine::Valid(resp_line) = read_bounded_line(&mut raw_client)? else {
            return Err("expected valid error response line".into());
        };
        let resp = decode_response(&resp_line)?;
        assert!(matches!(
            resp,
            IpcResponse::Error {
                code: IpcErrorCode::InvalidRequest,
                ..
            }
        ));

        match handle.join() {
            Ok(res) => res?,
            Err(_) => return Err("server thread panicked".into()),
        }
        Ok(())
    }

    #[test]
    fn ipc_nonblocking_accept_returns_false_when_idle() -> Result<(), Box<dyn std::error::Error>> {
        let tmp = TestTempDir::new("nb")?;
        let sock = tmp.path().join("s.sock");
        let server = IpcServer::bind(&sock)?;
        server.set_nonblocking(ListenerNonblockingMode::Accept)?;

        let handled =
            server.accept_once(DEFAULT_IPC_TIMEOUT, |_| IpcResponse::Ok { message: None })?;
        assert!(!handled);
        Ok(())
    }
}
