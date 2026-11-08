//! Explicit profile initialization. The public API owns reading and validation.
use nddev_clipboard_api::{Config, FileConfig};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub(crate) struct Initialization {
    pub archive: Option<PathBuf>,
    pub socket: Option<PathBuf>,
    pub budget: Option<u64>,
    pub retention: Option<u32>,
    pub capture: Option<bool>,
}

impl Initialization {
    pub(crate) fn is_empty(&self) -> bool {
        self.archive.is_none()
            && self.socket.is_none()
            && self.budget.is_none()
            && self.retention.is_none()
            && self.capture.is_none()
    }
}

pub(crate) fn initialize(path: &Path, selection: Initialization) -> std::io::Result<()> {
    if !path.is_absolute() {
        return Err(std::io::Error::other("configuration path must be absolute"));
    }
    if path.try_exists()? && selection.is_empty() {
        Config::read(path).map_err(std::io::Error::other)?;
        return Ok(());
    }
    let mut file: FileConfig = toml::from_str(include_str!("../../config/clipboard.seed.toml"))
        .map_err(std::io::Error::other)?;
    file.archive_dir = selection
        .archive
        .map(Ok)
        .unwrap_or_else(nddev_clipboard_api::default_archive_dir)
        .map_err(std::io::Error::other)?;
    file.socket_path = selection
        .socket
        .map(Ok)
        .unwrap_or_else(|| nddev_clipboard_api::default_socket_path(&file.archive_dir))
        .map_err(std::io::Error::other)?;
    if file.archive_dir.try_exists()?
        && (selection.budget.is_none()
            || selection.retention.is_none()
            || selection.capture.is_none())
    {
        return Err(std::io::Error::other(
            "existing archive: supply --budget-bytes, --retention-days and --capture explicitly",
        ));
    }
    if let Some(value) = selection.budget {
        file.budget_bytes = value;
    }
    if let Some(value) = selection.retention {
        file.retention_days = value;
    }
    if let Some(value) = selection.capture {
        file.capture_native = value;
    }
    let config = Config::new(file).map_err(std::io::Error::other)?;
    let text = config.to_toml().map_err(std::io::Error::other)?;
    if path.try_exists()? {
        let existing = Config::read(path).map_err(std::io::Error::other)?;
        if existing.to_toml().map_err(std::io::Error::other)? == text {
            return Ok(());
        }
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "existing configuration differs",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("configuration has no parent"))?;
    crate::storage_fs::private_dir(parent)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".clipboard-config-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(&temporary)?;
    let result = (|| {
        output.write_all(text.as_bytes())?;
        output.sync_all()?;
        // A hard link publishes only complete bytes and refuses an occupied name.
        // It does not replace a concurrent owner's file or redirect.
        std::fs::hard_link(&temporary, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    drop(output);
    std::fs::remove_file(&temporary)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    result
}
