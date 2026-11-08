//! One-shot resource evidence for safe native data reading. The fixture is
//! generated here; this program never opens the clipboard or a user archive.
#![forbid(unsafe_code)]

#[cfg(target_os = "macos")]
#[path = "../src/capture/macos/data_reader.rs"]
mod data_reader;

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This native data measurement requires macOS.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use data_reader::DataReader;
    use objc2::rc::autoreleasepool;
    use objc2_foundation::{NSData, NSDataReadingOptions, NSString};
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    let mode = std::env::args().nth(1).ok_or("expected stream or copy")?;
    let size_mib: usize = std::env::args()
        .nth(2)
        .ok_or("expected fixture size in MiB")?
        .parse()?;
    if !matches!(mode.as_str(), "stream" | "copy") || !(1..=256).contains(&size_mib) {
        return Err("expected stream|copy and a fixture of 1..=256 MiB".into());
    }
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let name = format!(
        "nddev-native-data-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(std::env::temp_dir().join(&name))?;
    let fixture = Fixture(std::env::temp_dir().join(name));
    let chunk: Vec<u8> = (0..=255).cycle().take(64 * 1024).collect();
    let mut expected = Sha256::new();
    for _ in 0..size_mib * 16 {
        file.write_all(&chunk)?;
        expected.update(&chunk);
    }
    drop(file);

    autoreleasepool(|_| -> Result<(), Box<dyn std::error::Error>> {
        let path = NSString::from_str(fixture.0.to_str().ok_or("non-UTF-8 fixture path")?);
        // Reading a file avoids a second full-sized Rust fixture allocation
        // before the measurement. The NSData is immutable in both cases.
        let native =
            NSData::dataWithContentsOfFile_options_error(&path, NSDataReadingOptions::empty())
                .map_err(|_| "could not read the native fixture")?;
        let started = Instant::now();
        let mut actual = Sha256::new();
        match mode.as_str() {
            "stream" => {
                let mut reader = DataReader::new(&native);
                assert_eq!(reader.content_length(), size_mib * 1024 * 1024);
                let mut buffer = [0; 64 * 1024];
                loop {
                    let count = reader.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    actual.update(&buffer[..count]);
                }
            }
            "copy" => {
                let bytes = native.to_vec();
                actual.update(std::hint::black_box(&bytes));
            }
            _ => unreachable!(),
        }
        assert_eq!(actual.finalize(), expected.finalize());
        println!(
            "native_data mode={mode} bytes={} elapsed_us={} byte_exact=true",
            native.len(),
            started.elapsed().as_micros()
        );
        Ok(())
    })
}
