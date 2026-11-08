use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(unix, windows))]
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

pub(crate) struct Policy {
    deadline: Instant,
    cancelled: AtomicBool,
    #[cfg(any(unix, windows))]
    socket: OnceLock<Weak<socket2::Socket>>,
}

impl Policy {
    #[cfg(any(unix, windows))]
    pub(crate) fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            cancelled: AtomicBool::new(false),
            socket: OnceLock::new(),
        }
    }

    #[cfg(any(unix, windows))]
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.shutdown();
    }

    #[cfg(any(unix, windows))]
    pub(crate) fn register(&self, socket: &Arc<socket2::Socket>) -> io::Result<()> {
        self.socket
            .set(Arc::downgrade(socket))
            .map_err(|_| io::ErrorKind::AlreadyExists.into())
    }

    pub(crate) fn shutdown(&self) {
        #[cfg(any(unix, windows))]
        if let Some(socket) = self.socket.get().and_then(Weak::upgrade) {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
    }

    fn stopped(&self, kind: io::ErrorKind) -> io::Error {
        self.shutdown();
        kind.into()
    }

    pub(crate) fn remaining(&self) -> io::Result<Duration> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(self.stopped(io::ErrorKind::ConnectionAborted));
        }
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| self.stopped(io::ErrorKind::TimedOut))
    }
}

pub(crate) fn copy(
    input: &mut impl Read,
    output: &mut impl Write,
    bytes: u64,
    policy: Option<&Policy>,
) -> io::Result<u64> {
    let mut buffer = [0; 8192];
    let mut copied = 0;
    while copied < bytes {
        if let Some(policy) = policy {
            policy.remaining()?;
        }
        let length = usize::try_from((bytes - copied).min(buffer.len() as u64))
            .expect("bounded by buffer length");
        let count = match input.read(&mut buffer[..length]) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if let Some(policy) = policy {
            policy.remaining()?;
        }
        output.write_all(&buffer[..count])?;
        copied += count as u64;
    }
    if let Some(policy) = policy {
        policy.remaining()?;
    }
    Ok(copied)
}
