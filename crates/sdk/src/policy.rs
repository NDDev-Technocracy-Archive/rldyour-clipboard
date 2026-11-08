use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(unix, windows))]
use std::sync::{Arc, Mutex, OnceLock, Weak};
#[cfg(any(unix, windows))]
use std::task::{Poll, Waker};
use std::time::{Duration, Instant};

pub(crate) struct Policy {
    deadline: Instant,
    cancelled: AtomicBool,
    #[cfg(any(unix, windows))]
    socket: OnceLock<Weak<async_io::Async<socket2::Socket>>>,
    #[cfg(any(unix, windows))]
    waiter: Mutex<Option<Waker>>,
}

impl Policy {
    #[cfg(any(unix, windows))]
    pub(crate) fn new(deadline: Instant) -> Self {
        Self {
            deadline,
            cancelled: AtomicBool::new(false),
            socket: OnceLock::new(),
            waiter: Mutex::new(None),
        }
    }

    #[cfg(any(unix, windows))]
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let waiter = self
            .waiter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(waiter) = waiter {
            waiter.wake();
        }
        self.shutdown();
    }

    #[cfg(any(unix, windows))]
    pub(crate) fn register(
        &self,
        socket: &Arc<async_io::Async<socket2::Socket>>,
    ) -> io::Result<()> {
        self.socket
            .set(Arc::downgrade(socket))
            .map_err(|_| io::ErrorKind::AlreadyExists.into())
    }

    pub(crate) fn shutdown(&self) {
        #[cfg(any(unix, windows))]
        if let Some(socket) = self.socket.get().and_then(Weak::upgrade) {
            let _ = socket.get_ref().shutdown(std::net::Shutdown::Both);
        }
    }

    #[cfg(any(unix, windows))]
    pub(crate) fn wait<T>(
        &self,
        future: impl std::future::Future<Output = io::Result<T>>,
    ) -> io::Result<T> {
        self.remaining()?;
        let _waiter = Waiter(self);
        let mut future = std::pin::pin!(future);
        let mut deadline = std::pin::pin!(async_io::Timer::at(self.deadline));
        async_io::block_on(std::future::poll_fn(|context| {
            {
                let mut waiter = self
                    .waiter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if waiter
                    .as_ref()
                    .is_none_or(|waker| !waker.will_wake(context.waker()))
                {
                    *waiter = Some(context.waker().clone());
                }
            }
            // Check after registering the waker so cancellation cannot be lost
            // between the check and an I/O future becoming pending.
            if let Err(error) = self.remaining() {
                return Poll::Ready(Err(error));
            }
            match future.as_mut().poll(context) {
                Poll::Ready(result) => Poll::Ready(result),
                Poll::Pending if deadline.as_mut().poll(context).is_ready() => {
                    Poll::Ready(Err(self.stopped(io::ErrorKind::TimedOut)))
                }
                Poll::Pending => Poll::Pending,
            }
        }))
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

#[cfg(any(unix, windows))]
struct Waiter<'a>(&'a Policy);

#[cfg(any(unix, windows))]
impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        self.0
            .waiter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
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
