use std::{
    env, fmt, fs,
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    time::Duration,
};

pub const CHANNEL_PREFIX_ENV: &str = "STUI_IPC_CHANNEL_PREFIX";
pub const DEFAULT_CHANNEL_PREFIX: &str = "stui.local";
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 1500;
pub const DEFAULT_POLL_IDLE_SLEEP_MS: u64 = 15;
pub const DEFAULT_EVENT_QUEUE_CAPACITY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpcPolicy {
    pub request_timeout_ms: u64,
    pub poll_idle_sleep_ms: u64,
    pub event_queue_capacity: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventQueueDropPolicy {
    DropOldest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelNamespace {
    prefix: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelName(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcEventCatalog<'a> {
    pub runtime: &'a str,
    pub instance: &'a str,
    pub transport: &'a str,
    pub events_channel: &'a str,
    pub control_channel: &'a str,
    pub mode: &'a str,
    pub emits: &'a [&'a str],
}

#[derive(Debug)]
pub enum IpcError {
    ChannelNotPublished { channel: String },
    ChannelOccupied { channel: String },
    RequestTimedOut { channel: String },
    Io(io::Error),
    InvalidPayload(String),
}

pub type IpcResult<T> = Result<T, IpcError>;

#[derive(Debug)]
pub struct LocalIpcServer {
    channel: ChannelName,
    listener: TcpListener,
    registry_path: PathBuf,
}

#[derive(Debug)]
pub struct LocalIpcConnection {
    stream: TcpStream,
}

impl Default for IpcPolicy {
    fn default() -> Self {
        Self {
            request_timeout_ms: DEFAULT_REQUEST_TIMEOUT_MS,
            poll_idle_sleep_ms: DEFAULT_POLL_IDLE_SLEEP_MS,
            event_queue_capacity: DEFAULT_EVENT_QUEUE_CAPACITY,
        }
    }
}

impl IpcPolicy {
    pub const fn event_queue_drop_policy(self) -> EventQueueDropPolicy {
        EventQueueDropPolicy::DropOldest
    }
}

impl EventQueueDropPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DropOldest => "drop-oldest",
        }
    }
}

impl Default for ChannelNamespace {
    fn default() -> Self {
        Self {
            prefix: DEFAULT_CHANNEL_PREFIX.to_string(),
        }
    }
}

impl ChannelNamespace {
    pub fn from_prefix(prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();

        if prefix.trim().is_empty() {
            Self::default()
        } else {
            Self {
                prefix: prefix.trim().to_string(),
            }
        }
    }

    pub fn from_env() -> Self {
        match env::var(CHANNEL_PREFIX_ENV) {
            Ok(prefix) if !prefix.trim().is_empty() => Self::from_prefix(prefix),
            _ => Self::default(),
        }
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn channel(&self, runtime: &str, instance: &str, capability: &str) -> ChannelName {
        ChannelName(format!(
            "{}.{}.{}.{}",
            self.prefix,
            sanitize_segment(runtime),
            sanitize_segment(instance),
            sanitize_segment(capability),
        ))
    }
}

impl ChannelName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'a> IpcEventCatalog<'a> {
    pub fn to_json(&self) -> String {
        let emits = self
            .emits
            .iter()
            .map(|event| format!("\"{}\"", escape_json_string(event)))
            .collect::<Vec<_>>()
            .join(",");

        format!(
            concat!(
                "{{",
                "\"runtime\":\"{}\"",
                ",\"instance\":\"{}\"",
                ",\"transport\":\"{}\"",
                ",\"events_channel\":\"{}\"",
                ",\"control_channel\":\"{}\"",
                ",\"mode\":\"{}\"",
                ",\"emits\":[{}]",
                "}}"
            ),
            escape_json_string(self.runtime),
            escape_json_string(self.instance),
            escape_json_string(self.transport),
            escape_json_string(self.events_channel),
            escape_json_string(self.control_channel),
            escape_json_string(self.mode),
            emits,
        )
    }
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChannelNotPublished { channel } => {
                write!(f, "channel not published: {channel}")
            }
            Self::ChannelOccupied { channel } => write!(f, "channel already occupied: {channel}"),
            Self::RequestTimedOut { channel } => write!(f, "request timed out: {channel}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::InvalidPayload(detail) => write!(f, "invalid payload: {detail}"),
        }
    }
}

impl std::error::Error for IpcError {}

impl From<io::Error> for IpcError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn ok_response(kind: &str, data_json: &str) -> String {
    format!(
        "{{\"ok\":true,\"kind\":\"{}\",\"data\":{}}}",
        escape_json_string(kind),
        data_json
    )
}

pub fn error_response(code: &str, detail: &str) -> String {
    format!(
        "{{\"ok\":false,\"error\":{{\"code\":\"{}\",\"detail\":\"{}\"}}}}",
        escape_json_string(code),
        escape_json_string(detail)
    )
}

impl LocalIpcServer {
    pub fn bind(channel: ChannelName) -> IpcResult<Self> {
        prepare_registry_for_bind(&channel)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let registry_path = registry_path(&channel);

        if let Some(parent) = registry_path.parent() {
            fs::create_dir_all(parent)?;
        }

        write_registry_file(&registry_path, &listener.local_addr()?.to_string())?;

        Ok(Self {
            channel,
            listener,
            registry_path,
        })
    }

    pub fn channel(&self) -> &ChannelName {
        &self.channel
    }

    pub fn accept(&self) -> io::Result<LocalIpcConnection> {
        let (stream, _) = self.listener.accept()?;
        apply_stream_timeout(&stream, IpcPolicy::default())?;
        Ok(LocalIpcConnection { stream })
    }

    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.listener.set_nonblocking(nonblocking)
    }

    pub fn try_accept(&self) -> io::Result<Option<LocalIpcConnection>> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                apply_stream_timeout(&stream, IpcPolicy::default())?;
                Ok(Some(LocalIpcConnection { stream }))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::ConnectionReset
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}

impl Drop for LocalIpcServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.registry_path);
    }
}

impl LocalIpcConnection {
    pub fn read_request(&mut self) -> IpcResult<String> {
        read_frame(&mut self.stream)
    }

    pub fn write_response(&mut self, response: &str) -> io::Result<()> {
        write_frame(&mut self.stream, response)
    }
}

pub fn request(channel: &ChannelName, request: &str) -> IpcResult<String> {
    let registry_path = registry_path(channel);
    let address = fs::read_to_string(&registry_path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            IpcError::ChannelNotPublished {
                channel: channel.as_str().to_string(),
            }
        } else {
            IpcError::Io(error)
        }
    })?;
    let address = address.trim().to_string();
    let mut stream = match TcpStream::connect(&address) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::NotFound
                    | io::ErrorKind::AddrNotAvailable
            ) =>
        {
            let _ = fs::remove_file(&registry_path);
            return Err(IpcError::ChannelNotPublished {
                channel: channel.as_str().to_string(),
            });
        }
        Err(error) => return Err(IpcError::Io(error)),
    };

    apply_stream_timeout(&stream, IpcPolicy::default())?;

    write_frame(&mut stream, request)?;
    read_frame(&mut stream).map_err(|error| map_timeout_error(error, channel))
}

fn write_frame(stream: &mut TcpStream, payload: &str) -> io::Result<()> {
    let bytes = payload.as_bytes();
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload too large"))?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()
}

fn read_frame(stream: &mut TcpStream) -> IpcResult<String> {
    let mut len_bytes = [0; 4];
    stream.read_exact(&mut len_bytes)?;
    let len = u32::from_le_bytes(len_bytes) as usize;
    let mut payload = vec![0; len];
    stream.read_exact(&mut payload)?;
    String::from_utf8(payload).map_err(|error| IpcError::InvalidPayload(error.to_string()))
}

fn prepare_registry_for_bind(channel: &ChannelName) -> IpcResult<()> {
    let path = registry_path(channel);

    if !path.exists() {
        return Ok(());
    }

    let address = fs::read_to_string(&path)?;

    match TcpStream::connect(address.trim()) {
        Ok(stream) => {
            let _ = apply_stream_timeout(&stream, IpcPolicy::default());
            Err(IpcError::ChannelOccupied {
                channel: channel.as_str().to_string(),
            })
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::TimedOut
                    | io::ErrorKind::NotFound
                    | io::ErrorKind::AddrNotAvailable
            ) =>
        {
            let _ = fs::remove_file(&path);
            Ok(())
        }
        Err(error) => Err(IpcError::Io(error)),
    }
}

fn apply_stream_timeout(stream: &TcpStream, policy: IpcPolicy) -> io::Result<()> {
    let timeout = Some(Duration::from_millis(policy.request_timeout_ms));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)
}

fn map_timeout_error(error: IpcError, channel: &ChannelName) -> IpcError {
    match error {
        IpcError::Io(io_error)
            if matches!(
                io_error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) =>
        {
            IpcError::RequestTimedOut {
                channel: channel.as_str().to_string(),
            }
        }
        other => other,
    }
}

fn registry_path(channel: &ChannelName) -> PathBuf {
    env::temp_dir()
        .join("stui-ipc")
        .join(format!("{}.addr", hex_encode(channel.as_str().as_bytes())))
}

fn write_registry_file(path: &PathBuf, address: &str) -> io::Result<()> {
    let temp_path = path.with_extension("addr.tmp");
    fs::write(&temp_path, address)?;

    if path.exists() {
        fs::remove_file(path)?;
    }

    fs::rename(temp_path, path)
}

fn sanitize_segment(segment: &str) -> String {
    let mut sanitized = String::with_capacity(segment.len());

    for ch in segment.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            sanitized.push(ch);
        } else {
            sanitized.push('_');
        }
    }

    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(nibble_to_hex(byte >> 4));
        output.push(nibble_to_hex(byte & 0x0f));
    }

    output
}

fn nibble_to_hex(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!("nibble out of range"),
    }
}

pub fn escape_json_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
