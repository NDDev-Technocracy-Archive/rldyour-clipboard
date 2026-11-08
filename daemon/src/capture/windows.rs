//! Event-driven Windows capture. A safe library owns the message-only window;
//! its creating thread owns the monitor for the entire receive loop.
#![forbid(unsafe_code)]

mod formats;
#[cfg(test)]
mod tests;

use super::Recorder;
use clipboard_win::{Clipboard, Getter, Monitor, formats as native, raw};
use formats::{BMP_PREFIX, file_uris, finish_bmp, strip_cf_html, utf16_to_utf8};

struct Formats {
    png: u32,
    html: u32,
    rtf: u32,
    excluded: [u32; 3],
    permission: [u32; 2],
}

impl Formats {
    fn new() -> Option<Self> {
        let id = |name| raw::register_format(name).map(|id| id.get());
        Some(Self {
            png: id("PNG")?,
            html: id("HTML Format")?,
            rtf: id("Rich Text Format")?,
            excluded: [
                id("ExcludeClipboardContentFromMonitorProcessing")?,
                id("Clipboard Viewer Ignore")?,
                id("ClipboardViewerIgnore")?,
            ],
            permission: [
                id("CanIncludeInClipboardHistory")?,
                id("CanUploadToCloudClipboard")?,
            ],
        })
    }
}

pub fn watch(recorder: &Recorder) {
    let Some(formats) = Formats::new() else {
        eprintln!("rldyour-clipboardd: native privacy formats could not be registered");
        return;
    };
    let Ok(mut monitor) = Monitor::new() else {
        eprintln!("rldyour-clipboardd: clipboard notification registration failed");
        return;
    };
    if receive(recorder, &formats, &mut monitor).is_err() {
        eprintln!("rldyour-clipboardd: clipboard notification loop failed");
    }
}

fn receive(
    recorder: &Recorder,
    formats: &Formats,
    monitor: &mut Monitor,
) -> clipboard_win::SysResult<()> {
    while monitor.recv()? {
        if let Some(parts) = read(formats, recorder.store.budget().max(0) as u64) {
            // The clipboard guard is gone before disk I/O, indexing or image
            // work. Another application can copy while the archive writes.
            recorder.record(parts, None);
        }
    }
    Ok(())
}

fn read(formats: &Formats, budget: u64) -> Option<Vec<(String, Vec<u8>)>> {
    // Match the library's bounded retry for a briefly occupied OS clipboard.
    // Keep ONE guard for privacy inspection and all representation reads.
    let _clipboard = Clipboard::new_attempts(10).ok()?;
    if concealed(formats) {
        return None;
    }
    let mut parts = Vec::new();
    push(&mut parts, "image/png", bytes(formats.png, budget, 0));
    push(
        &mut parts,
        "text/html",
        bytes(formats.html, budget, 0).map(strip_cf_html),
    );
    push(&mut parts, "text/rtf", bytes(formats.rtf, budget, 0));
    // Windows synthesizes Unicode from CF_TEXT using CF_LOCALE. Asking for
    // Unicode avoids treating a local ANSI code page as UTF-8.
    push(
        &mut parts,
        "text/plain;charset=utf-8",
        bytes(native::CF_UNICODETEXT, budget, 0).map(utf16_to_utf8),
    );
    if !parts.iter().any(|(mime, _)| mime == "image/png") {
        let format = if raw::is_format_avail(native::CF_DIBV5) {
            native::CF_DIBV5
        } else {
            native::CF_DIB
        };
        let bitmap = bytes(format, budget, BMP_PREFIX).and_then(|mut data| {
            finish_bmp(&mut data)?;
            Some(data)
        });
        push(&mut parts, "image/bmp", bitmap);
    }
    if raw::is_format_avail(native::CF_HDROP)
        && raw::size(native::CF_HDROP).is_some_and(|bytes| bytes.get() as u64 <= budget)
    {
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        if native::FileList.read_clipboard(&mut paths).is_ok() {
            // These are reference-only records until the durable file-content
            // model is implemented. Never claim that this saved file bytes.
            push(&mut parts, "text/uri-list", file_uris(&paths));
        }
    }
    Some(parts)
}

fn concealed(formats: &Formats) -> bool {
    if formats.excluded.iter().copied().any(raw::is_format_avail) {
        return true;
    }
    formats.permission.iter().any(|format| {
        if !raw::is_format_avail(*format) {
            return false;
        }
        // Inspect only the DWORD, never allocate an arbitrary-sized native
        // privacy payload. An offered unreadable or truncated flag opts out.
        let mut value = [0; 4];
        raw::get(*format, &mut value).ok() != Some(value.len()) || u32::from_le_bytes(value) == 0
    })
}

fn bytes(format: u32, budget: u64, prefix: usize) -> Option<Vec<u8>> {
    if !raw::is_format_avail(format) {
        return None;
    }
    let total = raw::size(format)?.get().checked_add(prefix)?;
    if total as u64 > budget {
        return None;
    }
    // RawData appends to this vector. DIB bytes therefore land directly after
    // their future BMP header, without another whole-payload allocation.
    let mut data = vec![0; prefix];
    native::RawData(format).read_clipboard(&mut data).ok()?;
    Some(data)
}

fn push(parts: &mut Vec<(String, Vec<u8>)>, mime: &str, data: Option<Vec<u8>>) {
    if let Some(data) = data
        && !data.is_empty()
    {
        parts.push((mime.to_owned(), data));
    }
}
