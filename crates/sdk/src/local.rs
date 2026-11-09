use crate::policy::Policy;
use crate::{Client, Error, Subscription};
use async_io::Async;
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

/// Cancels one operation, wakes its I/O wait and shuts down its own socket.
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
    socket: Arc<Async<Socket>>,
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
        let stream = self.transport(path)?;
        let policy = stream.policy.clone();
        Client::with_policy(stream, role, Some(policy))
    }

    pub fn subscribe(self, path: &Path) -> Result<Subscription<LocalTransport>, Error> {
        let stream = self.transport(path)?;
        let policy = stream.policy.clone();
        Subscription::with_policy(stream, Some(policy))
    }

    fn transport(self, path: &Path) -> Result<LocalTransport, Error> {
        self.policy.remaining().map_err(transport_error)?;
        let address = SockAddr::unix(path).map_err(transport_error)?;
        let socket = Socket::new(Domain::UNIX, Type::STREAM, None).map_err(transport_error)?;
        let result =
            socket.connect_timeout(&address, self.policy.remaining().map_err(transport_error)?);
        self.policy.remaining().map_err(transport_error)?;
        result.map_err(transport_error)?;
        let socket = Arc::new(Async::new(socket).map_err(transport_error)?);
        self.policy.register(&socket).map_err(transport_error)?;
        Ok(LocalTransport {
            socket,
            policy: self.policy,
        })
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
        self.finish(
            self.policy
                .wait(self.socket.read_with(|mut socket| socket.read(buffer))),
        )
    }
}

impl Write for LocalTransport {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.finish(
            self.policy
                .wait(self.socket.write_with(|mut socket| socket.write(buffer))),
        )
    }

    fn flush(&mut self) -> io::Result<()> {
        self.policy.remaining().map(|_| ())
    }
}

fn transport_error(error: io::Error) -> Error {
    Error::Transport(error.kind())
}
