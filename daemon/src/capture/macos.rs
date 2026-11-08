//! Clipboard capture on macOS.
//!
//! `NSPasteboard` has no change notification: the documented way to notice a
//! copy is to watch `changeCount`, which every AppKit clipboard manager does.
//! Reading it is a cheap Mach message, so a half-second poll costs far less
//! than it sounds — and nothing is read from the pasteboard itself until the
//! count has actually moved.
#![forbid(unsafe_code)]

mod data_reader;

use super::{Recorder, Startup};
use data_reader::DataReader;
use objc2::rc::Retained;
use objc2::rc::autoreleasepool;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSString};
use std::io::Read;
use std::time::Duration;

/// How often `changeCount` is read. Fast enough that a copy is archived before
/// the user can reach for the picker, cheap enough to leave running.
const POLL: Duration = Duration::from_millis(500);

/// Pasteboard types worth archiving, mapped to the mime names the rest of the
/// system uses. Keeping one table means the archive holds the same mime for
/// the same content whichever platform recorded it.
const TYPES: &[(&str, &str)] = &[
    ("public.png", "image/png"),
    ("public.jpeg", "image/jpeg"),
    ("public.tiff", "image/tiff"),
    ("public.html", "text/html"),
    ("public.rtf", "text/rtf"),
    ("public.file-url", "text/uri-list"),
    ("public.url", "text/uri-list"),
    ("public.utf8-plain-text", "text/plain;charset=utf-8"),
    // The legacy names, still set by older applications.
    ("NSStringPboardType", "text/plain;charset=utf-8"),
    ("NeXT TIFF v4.0 pasteboard type", "image/tiff"),
];

/// Observe new copies. The shared privacy policy is checked before native
/// payload reads and again when the archive commits the event.
pub(super) fn watch(recorder: &Recorder, startup: Startup) {
    let pasteboard = NSPasteboard::generalPasteboard();
    // Start from whatever is already there rather than archiving the contents
    // of the clipboard as though it had just been copied.
    let mut seen = pasteboard.changeCount();
    if !startup.complete() {
        return;
    }

    loop {
        std::thread::sleep(POLL);

        let current = pasteboard.changeCount();
        if current == seen {
            continue;
        }
        seen = current;

        if let Some(parts) = read(&pasteboard) {
            // Several representations must describe one copy, even when a
            // producer changes the pasteboard while AppKit materialises data.
            if pasteboard.changeCount() != current {
                continue;
            }
            // Each reader owns an immutable Foundation snapshot and supplies
            // bounded chunks to the archive's existing streaming writer.
            recorder.record_readers(
                parts.iter().map(|(mime, data)| {
                    let reader = DataReader::new(data);
                    let bytes = reader.content_length() as u64;
                    (mime.as_str(), reader, bytes)
                }),
                None,
            );
        }
    }
}

/// Reads every representation worth keeping, or nothing at all when the
/// pasteboard says its contents are a secret.
fn read(pasteboard: &NSPasteboard) -> Option<Vec<(String, Retained<NSData>)>> {
    autoreleasepool(|pool| {
        let available = pasteboard.types()?;
        let offered: Vec<String> = available.iter().map(|name| name.to_string()).collect();

        if offered.iter().any(|name| crate::kind::is_sensitive(name)) {
            return None;
        }

        let mut parts = Vec::new();
        if let Some(items) = pasteboard.pasteboardItems() {
            let mut urls = Vec::new();
            let key = NSString::from_str("public.file-url");
            for item in items.iter() {
                if let Some(data) = item.dataForType(&key) {
                    if DataReader::new(&data).read_to_end(&mut urls).is_err() {
                        return None;
                    }
                    urls.extend_from_slice(b"\r\n");
                }
            }
            if !urls.is_empty() {
                parts.push(("text/uri-list".to_string(), NSData::with_bytes(&urls)));
            }
        }
        for (uti, mime) in TYPES {
            if parts.iter().any(|(seen, _)| seen == mime) {
                continue;
            }
            if !offered.iter().any(|name| name == uti) {
                continue;
            }
            let key = NSString::from_str(uti);
            let Some(data) = pasteboard.dataForType(&key) else {
                continue;
            };
            if data.length() != 0 {
                parts.push((mime.to_string(), data));
            }
        }

        let _ = pool;
        Some(parts)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{outbox::Watchers, store::Store, testing::TempDir};
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboardItem, NSPasteboardWriting};
    use objc2_foundation::NSArray;
    use std::sync::Arc;

    struct PrivatePasteboard(Retained<NSPasteboard>);

    impl PrivatePasteboard {
        fn new() -> Self {
            Self(NSPasteboard::pasteboardWithUniqueName())
        }

        fn write(&self, items: &[&NSPasteboardItem]) {
            self.0.clearContents();
            let objects: Vec<&ProtocolObject<dyn NSPasteboardWriting>> = items
                .iter()
                .map(|item| ProtocolObject::from_ref(*item))
                .collect();
            assert!(self.0.writeObjects(&NSArray::from_slice(&objects)));
        }
    }

    impl Drop for PrivatePasteboard {
        fn drop(&mut self) {
            self.0.clearContents();
        }
    }

    fn native_item(parts: &[(&str, &[u8])]) -> Retained<NSPasteboardItem> {
        let item = NSPasteboardItem::new();
        for (uti, bytes) in parts {
            assert!(item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(uti)));
        }
        item
    }

    #[test]
    fn native_multiformat_copy_streams_exact_bytes_through_the_real_archive() {
        autoreleasepool(|_| {
            let pasteboard = PrivatePasteboard::new();
            let binary: Vec<u8> = (0..=255).cycle().take(3 * 1024 * 1024 + 37).collect();
            let text = b"synthetic native copy";
            let html = b"<b>synthetic native copy</b>";
            let first_url = b"file:///synthetic/first%20photo.png";
            let second_url = b"file:///synthetic/second.txt";
            let first = native_item(&[
                ("public.utf8-plain-text", text),
                ("public.html", html),
                ("public.png", &binary),
                ("public.file-url", first_url),
            ]);
            let second = native_item(&[("public.file-url", second_url)]);
            pasteboard.write(&[&first, &second]);
            let count = pasteboard.0.changeCount();
            let parts = read(&pasteboard.0).unwrap();
            assert_eq!(pasteboard.0.changeCount(), count);
            let root = TempDir::new();
            let store = Arc::new(Store::open(root.path(), 16 * 1024 * 1024).unwrap());
            let recorder = Recorder {
                store: Arc::clone(&store),
                watchers: Arc::new(Watchers::default()),
            };
            recorder.record_readers(
                parts.iter().map(|(mime, data)| {
                    let reader = DataReader::new(data);
                    let bytes = reader.content_length() as u64;
                    (mime.as_str(), reader, bytes)
                }),
                None,
            );
            let entries = store.list(20, None, None, None, None).unwrap();
            assert_eq!(entries.len(), 1);
            let urls = [first_url.as_slice(), b"\r\n", second_url, b"\r\n"].concat();
            for (mime, expected) in [
                ("text/plain;charset=utf-8", text.as_slice()),
                ("text/html", html.as_slice()),
                ("image/png", binary.as_slice()),
                ("text/uri-list", urls.as_slice()),
            ] {
                let (_, mut content, bytes) =
                    store.open_part(entries[0].id, Some(mime)).unwrap().unwrap();
                assert_eq!(bytes, expected.len() as u64);
                let mut actual = Vec::new();
                content.read_to_end(&mut actual).unwrap();
                assert_eq!(actual, expected, "{mime}");
            }
            assert_eq!(entries[0].mimes.len(), 4);
        });
    }

    #[test]
    fn native_privacy_hints_refuse_the_entire_multiformat_copy() {
        autoreleasepool(|_| {
            let pasteboard = PrivatePasteboard::new();
            // These are native macOS producer names. Other platforms' MIME
            // hints are not valid NSPasteboardItem UTI names.
            for hint in [
                "org.nspasteboard.ConcealedType",
                "org.nspasteboard.TransientType",
                "org.nspasteboard.AutoGeneratedType",
                "com.agilebits.onepassword",
                "de.petermaurer.TransientPasteboardType",
                "com.typeit4me.clipping",
            ] {
                let upper = hint.to_ascii_uppercase();
                let item = native_item(&[
                    ("public.utf8-plain-text", b"synthetic excluded text"),
                    ("public.png", b"synthetic excluded image"),
                    (&upper, b"1"),
                ]);
                pasteboard.write(&[&item]);
                assert!(read(&pasteboard.0).is_none(), "{hint}");
            }
        });
    }
}
