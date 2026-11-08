//! Real kernel sockets exercise stalled and interrupted local peers.
//! Successful archive operations are qualified against the actual daemon.
#![cfg(any(unix, windows))]

use nddev_clipboard_api::wire::{Request, Role};
use nddev_clipboard_sdk::{Error, LocalOperation};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
#[cfg(windows)]
use uds_windows::{UnixListener as Listener, UnixStream as Stream};

struct Endpoint {
    _directory: TempDir,
    path: PathBuf,
    listener: Listener,
}

impl Endpoint {
    fn new() -> Self {
        #[cfg(unix)]
        let parent = PathBuf::from("/tmp");
        #[cfg(windows)]
        let parent = std::env::temp_dir();
        let directory = tempfile::Builder::new()
            .prefix("cb-sdk-")
            .tempdir_in(parent)
            .unwrap();
        let path = directory.path().join("local.sock");
        let listener = Listener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        Self {
            _directory: directory,
            path,
            listener,
        }
    }

    fn accept_hello(&self) -> Stream {
        let deadline = Instant::now() + Duration::from_secs(3);
        let stream = loop {
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "local connection did not arrive");
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("local accept failed: {:?}", error.kind()),
            }
        };
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut frame = String::new();
        BufReader::new(&stream).read_line(&mut frame).unwrap();
        assert!(matches!(
            serde_json::from_str::<Request>(&frame).unwrap(),
            Request::Hello {
                v: 1,
                watch: false,
                ..
            }
        ));
        stream
    }
}

#[test]
fn cancellation_interrupts_the_owned_connected_socket_before_its_deadline() {
    let endpoint = Endpoint::new();
    let operation = LocalOperation::new(Instant::now() + Duration::from_secs(5));
    let cancellation = operation.cancellation();
    std::thread::scope(|scope| {
        let (send, receive) = mpsc::sync_channel(1);
        scope.spawn(|| {
            send.send(operation.connect(&endpoint.path, Role::Both).map(|_| ()))
                .unwrap();
        });
        let _peer = endpoint.accept_hello();
        cancellation.cancel();
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(2)).unwrap(),
            Err(Error::Transport(io::ErrorKind::ConnectionAborted))
        ));
    });
}

#[test]
fn incoming_progress_does_not_renew_the_absolute_greeting_deadline() {
    let endpoint = Endpoint::new();
    let started = Instant::now();
    let operation = LocalOperation::new(started + Duration::from_millis(600));
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| operation.connect(&endpoint.path, Role::Both).map(|_| ()));
        let mut peer = endpoint.accept_hello();
        let writer = scope.spawn(move || {
            // An unterminated peer frame makes progress more often than the
            // read timeout. A renewed per-read budget would outlive this bound.
            for _ in 0..100 {
                if peer.write_all(b" ").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        });
        assert!(matches!(
            worker.join().unwrap(),
            Err(Error::Transport(io::ErrorKind::TimedOut))
        ));
        assert!(started.elapsed() < Duration::from_secs(2));
        writer.join().unwrap();
    });
}

#[test]
fn expired_or_cancelled_work_never_attempts_connection() {
    let path = PathBuf::from("private-nonexistent-socket");
    let expired = LocalOperation::new(Instant::now());
    let error = expired.connect(&path, Role::Both).err().unwrap();
    assert!(matches!(error, Error::Transport(io::ErrorKind::TimedOut)));
    assert!(!error.to_string().contains("private-nonexistent"));

    let cancelled = LocalOperation::new(Instant::now() + Duration::from_secs(5));
    cancelled.cancellation().cancel();
    assert!(matches!(
        cancelled.connect(&path, Role::Both),
        Err(Error::Transport(io::ErrorKind::ConnectionAborted))
    ));
}
