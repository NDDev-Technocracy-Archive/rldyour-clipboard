//! Shared synthetic inputs and exclusive access to an isolated CI clipboard.
use clipboard_win::Clipboard;

#[path = "image.rs"]
mod image;
pub(super) const PNG: &[u8] = image::PNG;

pub(super) struct NativeClipboard {
    _lease: std::fs::File,
}
impl NativeClipboard {
    pub(super) fn new() -> Self {
        assert_eq!(
            std::env::var("NDDEV_CLIPBOARD_NATIVE_TESTS").as_deref(),
            Ok("isolated"),
            "Windows native acceptance requires an isolated CI desktop; never run it on a user desktop"
        );
        let root = std::env::var_os("RUNNER_TEMP").expect("isolated runner directory");
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(std::path::Path::new(&root).join("nddev-native-clipboard.lock"))
            .unwrap();
        lease.lock().unwrap();
        let _clipboard = Clipboard::new_attempts(10).unwrap();
        clipboard_win::empty().unwrap();
        Self { _lease: lease }
    }
}
impl Drop for NativeClipboard {
    fn drop(&mut self) {
        if let Ok(_clipboard) = Clipboard::new_attempts(10) {
            let _ = clipboard_win::empty();
        }
    }
}
