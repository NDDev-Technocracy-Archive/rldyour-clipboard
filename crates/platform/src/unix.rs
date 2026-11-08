use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::Path,
};

pub struct ActivatedListener {
    pub listener: UnixListener,
    /// Validation may accept the connection which triggered socket activation.
    /// The application must serve it before returning to the ordinary accept loop.
    pub pending: Option<UnixStream>,
}

pub fn inherited_listener(configured: &Path) -> io::Result<Option<ActivatedListener>> {
    let Some(listener) = acquire()? else {
        return Ok(None);
    };
    let activated = validate_listener(listener, configured)?;
    #[cfg(target_os = "linux")]
    mark_close_on_exec(3)?;
    Ok(Some(activated))
}

#[cfg(target_os = "linux")]
fn acquire() -> io::Result<Option<UnixListener>> {
    let owner = std::env::var("LISTEN_PID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok());
    if owner != Some(std::process::id()) {
        return Ok(None);
    }
    let count = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| io::Error::other("invalid activation descriptor count"))?;
    if count == 0 {
        return Ok(None);
    }
    if count != 1 {
        return Err(io::Error::other("expected exactly one activation socket"));
    }
    // Borrow the inherited descriptor, rather than claiming ownership based
    // only on environment text. Even a forged claim cannot close somebody
    // else's Rust-owned file. The original stays borrowed until process exit;
    // this independently owned duplicate is closed on every failure.
    Ok(Some(UnixListener::from(duplicate_descriptor(3)?)))
}

#[cfg(target_os = "linux")]
fn duplicate_descriptor(descriptor: libc::c_int) -> io::Result<OwnedFd> {
    // SAFETY: fcntl accepts an integer and reports an invalid descriptor as an
    // OS error. On success this command creates a fresh descriptor, with
    // close-on-exec set, which belongs only to this call. It retains no pointer.
    let duplicate = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 3) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the successful duplication above gave us this fresh owned fd.
    Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
}

#[cfg(target_os = "macos")]
fn acquire() -> io::Result<Option<UnixListener>> {
    unsafe extern "C" {
        fn launch_activate_socket(
            name: *const libc::c_char,
            descriptors: *mut *mut libc::c_int,
            count: *mut usize,
        ) -> libc::c_int;
    }
    let mut descriptors = std::ptr::null_mut();
    let mut count = 0usize;
    // SAFETY: the name is a static terminated C string. Both output pointers
    // are initialized, properly aligned locals alive for this synchronous API.
    // On success launchd transfers a malloc-owned array of count owned fds.
    let status = unsafe { launch_activate_socket(c"sock".as_ptr(), &mut descriptors, &mut count) };
    if matches!(status, libc::ENOENT | libc::ESRCH) {
        return Ok(None);
    }
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    if descriptors.is_null() {
        return Err(io::Error::other("activation returned no descriptors"));
    }
    // SAFETY: success provides exactly count initialized integers in the
    // caller-owned allocation. Each fd is transferred once into OwnedFd.
    // free releases only that allocation; the fds then have independent RAII.
    let mut owned: Vec<OwnedFd> = unsafe {
        let owned = std::slice::from_raw_parts(descriptors, count)
            .iter()
            .map(|&descriptor| OwnedFd::from_raw_fd(descriptor))
            .collect();
        libc::free(descriptors.cast());
        owned
    };
    if owned.len() != 1 {
        return Err(io::Error::other("expected exactly one launchd socket"));
    }
    let listener = UnixListener::from(owned.pop().expect("one descriptor was checked"));
    mark_close_on_exec(listener.as_raw_fd())?;
    Ok(Some(listener))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn acquire() -> io::Result<Option<UnixListener>> {
    Ok(None)
}

fn validate_listener(listener: UnixListener, configured: &Path) -> io::Result<ActivatedListener> {
    if socket_type(&listener)? != libc::SOCK_STREAM {
        return Err(io::Error::other(
            "activation descriptor is not a stream socket",
        ));
    }
    if listener.local_addr()?.as_pathname() != Some(configured) {
        return Err(io::Error::other(
            "activation endpoint differs from configuration",
        ));
    }
    // Darwin does not expose SO_ACCEPTCONN through getsockopt. A nonblocking
    // accept proves this is a listener on both OSes. Preserve any accepted
    // connection, including the first client that woke launchd/systemd.
    listener.set_nonblocking(true)?;
    let accepted = listener.accept();
    listener.set_nonblocking(false)?;
    let pending = match accepted {
        Ok((stream, _)) => Some(stream),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => None,
        Err(error) => return Err(error),
    };
    // BSD may inherit the listener's nonblocking flag on the accepted stream.
    // The daemon's framed reader relies on blocking I/O with explicit timeouts.
    if let Some(stream) = &pending {
        stream.set_nonblocking(false)?;
    }
    Ok(ActivatedListener { listener, pending })
}

fn socket_type(listener: &UnixListener) -> io::Result<libc::c_int> {
    let mut kind: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&kind) as libc::socklen_t;
    // SAFETY: the listener owns a live fd. The OS either rejects its type or
    // writes SO_TYPE into the initialized integer with the supplied capacity.
    // Both output pointers live for the call; the returned length is checked.
    let result = unsafe {
        libc::getsockopt(
            listener.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&mut kind as *mut libc::c_int).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of_val(&kind) {
        return Err(io::Error::other(
            "invalid activation socket type information",
        ));
    }
    Ok(kind)
}

fn mark_close_on_exec(descriptor: libc::c_int) -> io::Result<()> {
    // SAFETY: these fcntl commands take an integer, retain no pointers and
    // report invalid descriptors through their return values. No fd is closed
    // or transferred, and all existing descriptor flags are preserved.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: same integer-only contract as above; FD_CLOEXEC is a valid flag.
    if unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixDatagram;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = PathBuf::from("/tmp").canonicalize().unwrap().join(format!(
                "cb-activation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn validation_preserves_the_client_that_triggered_activation() {
        let fixture = Fixture::new();
        let path = fixture.0.join("listener");
        let listener = UnixListener::bind(&path).unwrap();
        let mut client = UnixStream::connect(&path).unwrap();
        client.write_all(b"first request").unwrap();
        let activated = validate_listener(listener, &path).unwrap();
        let mut first = activated.pending.unwrap();
        let mut content = [0; 13];
        first.read_exact(&mut content).unwrap();
        assert_eq!(&content, b"first request");
    }

    #[test]
    fn validation_keeps_an_empty_listener_ready_for_a_later_client() {
        let fixture = Fixture::new();
        let path = fixture.0.join("listener");
        let activated = validate_listener(UnixListener::bind(&path).unwrap(), &path).unwrap();
        assert!(activated.pending.is_none());
        let _client = UnixStream::connect(&path).unwrap();
        assert!(activated.listener.accept().is_ok());
    }

    #[test]
    fn validation_refuses_datagrams_files_and_a_different_endpoint() {
        let fixture = Fixture::new();
        let path = fixture.0.join("listener");
        let datagram = UnixDatagram::bind(&path).unwrap();
        let descriptor: OwnedFd = datagram.into();
        assert!(validate_listener(UnixListener::from(descriptor), &path).is_err());
        std::fs::remove_file(&path).unwrap();
        let file = std::fs::File::create(&path).unwrap();
        let descriptor: OwnedFd = file.into();
        assert!(validate_listener(UnixListener::from(descriptor), &path).is_err());
        std::fs::remove_file(&path).unwrap();
        let listener = UnixListener::bind(&path).unwrap();
        assert!(
            validate_listener(listener.try_clone().unwrap(), &fixture.0.join("different")).is_err()
        );
        assert!(UnixStream::connect(&path).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn duplicating_a_descriptor_preserves_its_original_owner() {
        let fixture = Fixture::new();
        let path = fixture.0.join("retained");
        let original = std::fs::File::create(&path).unwrap();
        let duplicate = duplicate_descriptor(original.as_raw_fd()).unwrap();
        assert_ne!(duplicate.as_raw_fd(), original.as_raw_fd());
        drop(duplicate);
        assert!(original.metadata().unwrap().is_file());
        assert!(duplicate_descriptor(-1).is_err());
    }
}
