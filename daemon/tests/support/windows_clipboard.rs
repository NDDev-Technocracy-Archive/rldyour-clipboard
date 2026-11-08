//! Shared synthetic inputs and exclusive access to an isolated CI clipboard.
use clipboard_win::Clipboard;

// Generated 1x1 RGBA PNG, pixel (0x12, 0x34, 0x56, 0xff).
pub(super) const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 16, 50, 9, 251, 15, 0, 2,
    148, 1, 156, 29, 91, 70, 95, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

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
