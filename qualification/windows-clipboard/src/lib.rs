//! Evaluation only. All clipboard writes must run on an isolated CI desktop.
#![forbid(unsafe_code)]

pub fn entry() {
    let Some(report) = std::env::args_os().nth(1) else {
        std::process::exit(2);
    };
    let result = std::panic::catch_unwind(qualify)
        .unwrap_or_else(|_| Err("native qualification panicked".into()));
    let output = match &result {
        Ok(note) => format!("PASS {note}\n"),
        Err(error) => format!("FAIL {error}\n"),
    };
    if std::fs::write(report, output).is_err() || result.is_err() {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn qualify() -> Result<String, Box<dyn std::error::Error>> {
    Err("Windows native qualification requires Windows".into())
}

#[cfg(windows)]
fn qualify() -> Result<String, Box<dyn std::error::Error>> {
    use clipboard_win::{Clipboard, Getter, Monitor, Setter, formats, options, raw};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    #[derive(Debug)]
    struct Snapshot {
        text: String,
        html: String,
        binary: Vec<u8>,
        files: Vec<PathBuf>,
    }

    let binary_format = raw::register_format("NDDev Clipboard Synthetic Binary")
        .ok_or("could not register the synthetic format")?
        .get();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (events_tx, events_rx) = mpsc::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let outcome = (|| -> Result<(), String> {
            let mut monitor = Monitor::new().map_err(|error| error.to_string())?;
            ready_tx
                .send(monitor.shutdown_channel())
                .map_err(|error| error.to_string())?;
            while monitor.recv().map_err(|error| error.to_string())? {
                let clipboard = Clipboard::new_attempts(10).map_err(|error| error.to_string())?;
                // Read only our transaction. An unrelated registration event
                // must not make the probe inspect pre-existing desktop data.
                if !raw::is_format_avail(binary_format) {
                    continue;
                }
                let sequence = raw::seq_num();
                let mut snapshot = Snapshot {
                    text: String::new(),
                    html: String::new(),
                    binary: Vec::new(),
                    files: Vec::new(),
                };
                formats::Unicode
                    .read_clipboard(&mut snapshot.text)
                    .map_err(|error| error.to_string())?;
                formats::Html::new()
                    .ok_or("could not register HTML")?
                    .read_clipboard(&mut snapshot.html)
                    .map_err(|error| error.to_string())?;
                formats::RawData(binary_format)
                    .read_clipboard(&mut snapshot.binary)
                    .map_err(|error| error.to_string())?;
                formats::FileList
                    .read_clipboard(&mut snapshot.files)
                    .map_err(|error| error.to_string())?;
                if raw::seq_num() != sequence {
                    return Err("clipboard changed during a locked multi-format read".into());
                }
                drop(clipboard);
                events_tx
                    .send(snapshot)
                    .map_err(|error| error.to_string())?;
            }
            Ok(())
        })();
        let _ = stopped_tx.send(outcome);
    });
    let shutdown = ready_rx.recv_timeout(Duration::from_secs(5))?;
    let text = "Synthetic copy: русский, қазақша, العربية, 中文, 🖇";
    let html = "<b>Synthetic &amp; rich copy</b>";
    let binary: Vec<u8> = (0..=255).cycle().take(3 * 1024 * 1024 + 37).collect();
    let files = [r"C:\synthetic\first photo.png", r"C:\synthetic\二番目.txt"];
    {
        let clipboard = Clipboard::new_attempts(10)?;
        clipboard_win::empty()?;
        raw::set_string_with(text, options::NoClear)?;
        formats::Html::new()
            .ok_or("could not register HTML")?
            .write_clipboard(html)?;
        raw::set_without_clear(binary_format, &binary)?;
        raw::set_file_list(&files)?;
        drop(clipboard);
    }
    let captured = events_rx.recv_timeout(Duration::from_secs(5))?;
    assert_eq!(captured.text, text);
    assert_eq!(captured.html, html);
    assert!(captured.binary.starts_with(&binary));
    assert!(
        captured.binary[binary.len()..]
            .iter()
            .all(|byte| *byte == 0)
    );
    assert_eq!(captured.files, files.map(PathBuf::from));
    // The monitor belongs to its creating thread. Only its library-provided
    // shutdown handle crosses the thread boundary; no callback/static borrow.
    drop(shutdown);
    stopped_rx
        .recv_timeout(Duration::from_secs(5))?
        .map_err(std::io::Error::other)?;
    worker
        .join()
        .map_err(|_| "native monitor worker panicked")?;
    let _clipboard = Clipboard::new_attempts(10)?;
    clipboard_win::empty()?;
    Ok(format!(
        "monitor=true unicode=true html=true file_list=true shutdown=true binary_bytes={} native_allocation_bytes={}",
        binary.len(),
        captured.binary.len()
    ))
}
