use crate::policy::Policy;
use crate::{Client, Error};
use nddev_clipboard_api::wire::Role;
use socket2::{Domain, SockAddr, Socket, Type};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// One finite local exchange, including connection and protocol negotiation.
///
/// The deadline remains absolute across all requests and streamed chunks. Use a
/// new operation for later work. A cancellation handle cannot open a connection.
pub struct LocalOperation {
    policy: Arc<Policy>,
}

/// Cancels one operation and shuts down its own connected socket.
///
/// Connect itself is bounded by the deadline; cancellation is checked before
/// and after it. Cancellation never proves that a sent mutation was undone.
/// Keeping this handle does not keep a dropped connection open.
#[derive(Clone)]
pub struct Cancellation {
    policy: Arc<Policy>,
}

/// An owned pathname AF_UNIX stream; it exposes no raw handles or archive access.
pub struct LocalTransport {
    socket: Arc<Socket>,
    policy: Arc<Policy>,
}

impl LocalOperation {
    pub fn new(deadline: Instant) -> Self {
        Self {
            policy: Arc::new(Policy::new(deadline)),
        }
    }

    pub fn cancellation(&self) -> Cancellation {
        Cancellation {
            policy: self.policy.clone(),
        }
    }

    pub fn connect(self, path: &Path, role: Role) -> Result<Client<LocalTransport>, Error> {
        self.policy.remaining().map_err(transport_error)?;
        let address = SockAddr::unix(path).map_err(transport_error)?;
        let socket =
            Arc::new(Socket::new(Domain::UNIX, Type::STREAM, None).map_err(transport_error)?);
        self.policy.register(&socket).map_err(transport_error)?;
        let stream = LocalTransport {
            socket,
            policy: self.policy,
        };
        let result = stream.socket.connect_timeout(
            &address,
            stream.policy.remaining().map_err(transport_error)?,
        );
        stream.finish(result).map_err(transport_error)?;
        let policy = stream.policy.clone();
        Client::with_policy(stream, role, Some(policy))
    }
}

impl Cancellation {
    pub fn cancel(&self) {
        self.policy.cancel();
    }
}

impl LocalTransport {
    fn finish<T>(&self, result: io::Result<T>) -> io::Result<T> {
        self.policy.remaining()?;
        result.map_err(|error| match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => io::ErrorKind::TimedOut.into(),
            _ => error,
        })
    }
}

impl Read for LocalTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket
            .set_read_timeout(Some(self.policy.remaining()?))?;
        self.finish((&*self.socket).read(buffer))
    }
}

impl Write for LocalTransport {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.socket
            .set_write_timeout(Some(self.policy.remaining()?))?;
        self.finish((&*self.socket).write(buffer))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.policy.remaining().map(|_| ())
    }
}

fn transport_error(error: io::Error) -> Error {
    Error::Transport(error.kind())
}
