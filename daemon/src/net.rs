//! The local socket, under the one name the rest of the daemon uses.
//!
//! Linux and macOS have `std::os::unix::net`; Windows has had AF_UNIX since
//! Windows 10 but exposes it through a crate rather than the standard library.
//! Aliasing them here keeps every other module free of platform branches.
use std::io;

#[cfg(unix)]
pub(crate) use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
#[cfg(windows)]
pub(crate) use uds_windows::{UnixListener as Listener, UnixStream as Stream};

pub(crate) struct BoundListener {
    pub(crate) listener: Listener,
    pub(crate) activated: bool,
    pub(crate) pending: Option<Stream>,
}

/// Inspect existing activation authority before opening an archive. This never
/// creates an endpoint or reads a pending client's control/content bytes.
pub(crate) fn inherited(path: &std::path::Path) -> io::Result<Option<BoundListener>> {
    #[cfg(unix)]
    if let Some(inherited) = nddev_clipboard_platform::inherited_listener(path)? {
        return Ok(Some(BoundListener {
            listener: inherited.listener,
            activated: true,
            pending: inherited.pending,
        }));
    }
    #[cfg(windows)]
    let _ = path;
    Ok(None)
}

/// Publish a self-bound endpoint only after the caller owns its archive lease.
pub(crate) fn bind(path: &std::path::Path) -> std::io::Result<BoundListener> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("socket has no parent"))?;
    crate::storage_fs::private_dir(parent)?;
    let listener = bind_at(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(BoundListener {
        listener,
        activated: false,
        pending: None,
    })
}

fn bind_at(path: &std::path::Path) -> io::Result<Listener> {
    match Listener::bind(path) {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == io::ErrorKind::AddrInUse => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                if !std::fs::symlink_metadata(path)?.file_type().is_socket() {
                    return Err(error);
                }
            }
            #[cfg(windows)]
            if !nddev_clipboard_platform::is_unix_socket(path)? {
                return Err(error);
            }
            match Stream::connect(path) {
                Ok(_) => Err(error), // Never unlink another live daemon.
                Err(probe) if probe.kind() == io::ErrorKind::ConnectionRefused => {
                    std::fs::remove_file(path)?;
                    Listener::bind(path)
                }
                Err(probe) => Err(probe),
            }
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn path() -> std::path::PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "nddev-clipboard-{}-{}.sock",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn a_second_daemon_cannot_unlink_the_live_listener() {
        let path = path();
        let original = bind_at(&path).unwrap();
        assert_eq!(bind_at(&path).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        let _peer = Stream::connect(&path).unwrap();
        drop(original);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn only_a_dead_socket_is_replaced() {
        let path = path();
        drop(bind_at(&path).unwrap());
        let replacement = bind_at(&path).unwrap();
        drop(replacement);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_regular_file_is_never_removed() {
        let path = path();
        std::fs::write(&path, b"keep").unwrap();
        assert!(bind_at(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn an_occupied_directory_is_never_removed() {
        let path = path();
        std::fs::create_dir(&path).unwrap();
        let content = path.join("keep");
        std::fs::write(&content, b"retained directory content").unwrap();
        assert!(bind_at(&path).is_err());
        assert_eq!(
            std::fs::read(content).unwrap(),
            b"retained directory content"
        );
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_link_to_a_dead_socket_is_never_removed_or_followed() {
        let endpoint = path();
        let link = path();
        drop(bind_at(&endpoint).unwrap());
        #[cfg(unix)]
        std::os::unix::fs::symlink(&endpoint, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&endpoint, &link).unwrap();
        assert!(bind_at(&link).is_err());
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(std::fs::symlink_metadata(&endpoint).is_ok());
        std::fs::remove_file(link).unwrap();
        std::fs::remove_file(endpoint).unwrap();
    }
}
