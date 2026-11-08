//! Real native clipboard on an isolated Windows runner; all payloads are ours.
use super::*;
use crate::{outbox::Watchers, store::Store, testing::TempDir};
use clipboard_win::options;
use std::io::Read;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

// A generated 1x1 RGBA PNG with pixel (0x12, 0x34, 0x56, 0xff).
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 16, 50, 9, 251, 15, 0, 2,
    148, 1, 156, 29, 91, 70, 95, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

struct SyntheticClipboard;
impl Drop for SyntheticClipboard {
    fn drop(&mut self) {
        if let Ok(_clipboard) = Clipboard::new_attempts(10) {
            let _ = clipboard_win::empty();
        }
    }
}

fn set_text(text: &str) {
    let _clipboard = Clipboard::new_attempts(10).unwrap();
    clipboard_win::empty().unwrap();
    raw::set_string_with(text, options::NoClear).unwrap();
}

fn content(store: &Store, entry: i64, mime: &str) -> Vec<u8> {
    let (_, mut file, bytes) = store.open_part(entry, Some(mime)).unwrap().unwrap();
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).unwrap();
    assert_eq!(actual.len() as u64, bytes);
    actual
}

#[test]
fn native_monitor_archives_multiformat_bytes_and_privacy_flags_exclude_whole_copies() {
    let _cleanup = SyntheticClipboard;
    // Before monitor registration, replace only this isolated CI desktop's
    // clipboard. Never read any pre-existing content even in the fixture.
    set_text("synthetic startup marker");
    let root = TempDir::new();
    let budget = 8 * 1024 * 1024;
    let store = Arc::new(Store::open(root.path(), budget).unwrap());
    let (ready_tx, ready_rx) = mpsc::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let target = Arc::clone(&store);
    let worker = std::thread::spawn(move || {
        let recorder = Recorder {
            store: target,
            watchers: Arc::new(Watchers::default()),
        };
        let mut monitor = Monitor::new().unwrap();
        let formats = Formats::new().unwrap();
        ready_tx.send(monitor.shutdown_channel()).unwrap();
        let outcome = receive(&recorder, &formats, &mut monitor).map_err(|error| error.to_string());
        stopped_tx.send(outcome).unwrap();
    });
    let shutdown = ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let formats = Formats::new().unwrap();
    let text = "Native Windows: русский қазақша العربية 中文 🖇";
    let html = "<p>Native <b>synthetic</b> copy</p>";
    let rtf = br"{\rtf1\ansi native synthetic copy}";
    let paths = [
        root.path().join("first photo.png"),
        root.path().join("二.txt"),
    ];
    std::fs::write(&paths[0], PNG).unwrap();
    std::fs::write(&paths[1], b"synthetic file").unwrap();
    {
        let _clipboard = Clipboard::new_attempts(10).unwrap();
        clipboard_win::empty().unwrap();
        raw::set_string_with(text, options::NoClear).unwrap();
        raw::set_html_with(formats.html, html, options::NoClear).unwrap();
        raw::set_without_clear(formats.rtf, rtf).unwrap();
        raw::set_without_clear(formats.png, PNG).unwrap();
        raw::set_file_list(
            &paths
                .iter()
                .map(|path| path.to_str().unwrap())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let entry = loop {
        let entries = store.list(20, None, None, None, None).unwrap();
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.mimes.contains(&"image/png".to_owned()))
        {
            break entry.id;
        }
        assert!(Instant::now() < deadline, "native copy was not archived");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        content(&store, entry, "text/plain;charset=utf-8"),
        text.as_bytes()
    );
    assert_eq!(content(&store, entry, "text/html"), html.as_bytes());
    assert_eq!(content(&store, entry, "text/rtf"), rtf);
    assert_eq!(content(&store, entry, "image/png"), PNG);
    assert_eq!(
        content(&store, entry, "text/uri-list"),
        file_uris(&paths).unwrap()
    );
    store.set_pinned(entry, true).unwrap();
    drop(shutdown);
    stopped_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();

    for hint in [
        "ExcludeClipboardContentFromMonitorProcessing",
        "Clipboard Viewer Ignore",
        "ClipboardViewerIgnore",
        "CanIncludeInClipboardHistory",
        "CanUploadToCloudClipboard",
    ] {
        for value in [vec![0], vec![0; 4]] {
            let _clipboard = Clipboard::new_attempts(10).unwrap();
            clipboard_win::empty().unwrap();
            raw::set_string_with("synthetic excluded text", options::NoClear).unwrap();
            raw::set_without_clear(formats.png, PNG).unwrap();
            raw::set_without_clear(raw::register_format(hint).unwrap().get(), &value).unwrap();
            drop(_clipboard);
            assert!(read(&formats, budget as u64).is_none(), "{hint}");
        }
    }
    for hint in ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
        let _clipboard = Clipboard::new_attempts(10).unwrap();
        clipboard_win::empty().unwrap();
        raw::set_string_with("synthetic permitted text", options::NoClear).unwrap();
        raw::set_without_clear(
            raw::register_format(hint).unwrap().get(),
            &1u32.to_le_bytes(),
        )
        .unwrap();
        drop(_clipboard);
        let parts = read(&formats, budget as u64).unwrap();
        assert!(
            parts
                .iter()
                .any(|(mime, bytes)| mime == "text/plain;charset=utf-8"
                    && bytes == b"synthetic permitted text")
        );
    }
    assert!(read(&formats, 1).unwrap().is_empty());

    // A native DIB without PNG follows the BMP path. The header is assembled
    // in the same vector, including OS-synthesized V5 data when available.
    let mut dib = vec![0; 48];
    dib[..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&2i32.to_le_bytes());
    dib[8..12].copy_from_slice(&1i32.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&24u16.to_le_bytes());
    dib[40..48].copy_from_slice(&[0x56, 0x34, 0x12, 0x89, 0x67, 0x45, 0, 0]);
    {
        let _clipboard = Clipboard::new_attempts(10).unwrap();
        clipboard_win::empty().unwrap();
        raw::set_without_clear(native::CF_DIB, &dib).unwrap();
    }
    let parts = read(&formats, budget as u64).unwrap();
    let bitmap = parts.iter().find(|(mime, _)| mime == "image/bmp").unwrap();
    assert_eq!(&bitmap.1[..2], b"BM");
    #[cfg(feature = "thumbnails")]
    {
        let image = image::load_from_memory(&bitmap.1).unwrap().to_rgba8();
        assert_eq!((image.width(), image.height()), (2, 1));
        assert_eq!(image.get_pixel(0, 0).0, [0x12, 0x34, 0x56, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0x45, 0x67, 0x89, 255]);
    }
}
