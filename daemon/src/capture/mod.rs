//! Native clipboard capture, where the platform allows it.
//!
//! On macOS and Windows the daemon can watch the clipboard itself, so it does:
//! a tray client that had to relay every copy would be a second process in the
//! path for no gain.
//!
//! On Linux it depends on the display protocol. X11 selections are readable by
//! any client, so there the daemon watches `CLIPBOARD` directly — which is
//! also how remote copies arrive, because `xrdp-chansrv` takes X11 selection
//! ownership whenever the RDP peer copies. Wayland is the different case:
//! Mutter implements neither `wlr-data-control` nor `ext-data-control-v1` and
//! its maintainers have said it will not — reading another application's
//! clipboard is treated as something a compositor should not hand out. There
//! the GNOME Shell extension captures and streams entries in over the socket.

use crate::outbox::Watchers;
use crate::proto::Response;
use crate::session::now;
use crate::store::{Accepted, Store};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::time::Duration;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

/// Starts watching the clipboard, if this platform lets the daemon do it.
///
/// Returns whether a watcher was started, which is only used to say so in the
/// log: a platform without one is the expected state, not a degraded one.
pub(crate) fn spawn(store: Arc<Store>, watchers: Arc<Watchers>) -> bool {
    // On Linux the X11 watcher is only worth starting when an X server is
    // actually reachable — a Wayland-only session has no `DISPLAY`, and there
    // the extension is the capture path by design.
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none() {
        return false;
    }

    // Rendezvous: a successful send means startup received the handoff. A
    // queued success could race a timeout and leave a late observer running.
    let (sender, ready) = mpsc::sync_channel(0);
    let cancelled = Arc::new(AtomicBool::new(false));
    let startup = Startup {
        sender,
        cancelled: Arc::clone(&cancelled),
    };
    let spawned = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            let recorder = Recorder { store, watchers };
            watch(&recorder, startup);
        });

    if spawned.is_err() {
        eprintln!("rldyour-clipboardd: could not start the clipboard watcher");
        return false;
    }
    // A thread existing is not a ready native observer. Do not serve the
    // first client until notification registration / initial changeCount has
    // completed, or a copy immediately after the greeting can be lost.
    if ready.recv_timeout(Duration::from_secs(5)).is_err() {
        cancelled.store(true, Ordering::Release);
        eprintln!("rldyour-clipboardd: native clipboard observer did not become ready");
        return false;
    }
    true
}

struct Startup {
    sender: SyncSender<()>,
    cancelled: Arc<AtomicBool>,
}
impl Startup {
    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
    fn complete(self) -> bool {
        !self.cancelled() && self.sender.send(()).is_ok()
    }
}

/// The backend the platform provides.
///
/// On a GNOME X11 session the shell extension watches the same selection;
/// a copy seen by both is one event to the archive, which the store's
/// content deduplication absorbs.
#[cfg(target_os = "linux")]
fn watch(recorder: &Recorder, startup: Startup) {
    x11::watch(recorder, startup);
}

#[cfg(target_os = "macos")]
fn watch(recorder: &Recorder, startup: Startup) {
    macos::watch(recorder, startup);
}

#[cfg(target_os = "windows")]
fn watch(recorder: &Recorder, startup: Startup) {
    windows::watch(recorder, startup);
}

/// Platforms with no display protocol the daemon can read directly.
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn watch(_: &Recorder, _: Startup) {}

/// What a platform watcher hands one clipboard event to.
///
/// Keeping the archive behind this means a backend deals only in
/// `(mime, bytes)` pairs and never learns how entries are stored, hashed or
/// announced.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
pub(crate) struct Recorder {
    store: Arc<Store>,
    watchers: Arc<Watchers>,
}

#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    allow(dead_code)
)]
impl Recorder {
    /// Archives one clipboard event holding every representation given.
    ///
    /// Secrets and empty events are dropped by the store, so a backend can
    /// pass on whatever the platform offered without deciding anything.
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn record(&self, parts: Vec<(String, Vec<u8>)>, source: Option<&str>) {
        self.record_borrowed(
            parts
                .iter()
                .map(|(mime, bytes)| (mime.as_str(), bytes.as_slice())),
            source,
        );
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn record_borrowed<'a>(
        &self,
        parts: impl IntoIterator<Item = (&'a str, &'a [u8])>,
        source: Option<&str>,
    ) {
        self.record_readers(
            parts
                .into_iter()
                .map(|(mime, bytes)| (mime, std::io::Cursor::new(bytes), bytes.len() as u64)),
            source,
        );
    }

    pub(crate) fn record_readers<'a, R: std::io::Read>(
        &self,
        parts: impl IntoIterator<Item = (&'a str, R, u64)>,
        source: Option<&str>,
    ) {
        let parts: Vec<_> = parts.into_iter().collect();
        if parts.is_empty() || crate::kind::any_sensitive(parts.iter().map(|(mime, _, _)| *mime)) {
            return;
        }

        let mut accepted = Vec::with_capacity(parts.len());
        for (mime, mut content, bytes) in parts {
            match self.store.accept(mime, &mut content, bytes) {
                Ok(part) => accepted.push(part),
                // One representation the archive will not take must not cost
                // the entry its others.
                Err(error) => {
                    eprintln!(
                        "rldyour-clipboardd: representation skipped ({})",
                        error.code()
                    );
                }
            }
        }

        self.commit(accepted, source);
    }

    fn commit(&self, accepted: Vec<Accepted>, source: Option<&str>) {
        match self.store.commit(&accepted, source, now()) {
            Ok(Some(done)) => {
                for evicted in done.evicted {
                    self.watchers
                        .broadcast(&Response::Removed { entry: evicted });
                }
                if done.entry != 0 {
                    let event = if done.created {
                        Response::Added {
                            entry: done.summary,
                        }
                    } else {
                        Response::Updated {
                            entry: done.summary,
                        }
                    };
                    self.watchers.broadcast(&event);
                }
            }
            // Empty, or a secret the store refused. Nothing to announce.
            Ok(None) => {}
            Err(error) => eprintln!("rldyour-clipboardd: could not archive a copy: {error}"),
        }
        self.store.discard(accepted);
    }
}
