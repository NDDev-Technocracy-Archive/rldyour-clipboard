#![cfg(windows)]

use nddev_clipboard_platform::is_unix_socket;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "cb-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn native_file_tag_distinguishes_real_endpoints_from_files_and_redirects() {
    use std::os::windows::fs::{symlink_dir, symlink_file};

    let fixture = Fixture::new();
    let file = fixture.0.join("ordinary");
    let directory = fixture.0.join("directory");
    let file_link = fixture.0.join("file-link");
    let directory_link = fixture.0.join("directory-link");
    let endpoint = fixture.0.join("endpoint.sock");
    let socket_link = fixture.0.join("socket-link");
    std::fs::write(&file, b"keep ordinary content").unwrap();
    std::fs::create_dir(&directory).unwrap();
    symlink_file(&file, &file_link).unwrap();
    symlink_dir(&directory, &directory_link).unwrap();
    let listener = uds_windows::UnixListener::bind(&endpoint).unwrap();
    symlink_file(&endpoint, &socket_link).unwrap();
    for path in [&file, &directory, &file_link, &directory_link, &socket_link] {
        assert!(!is_unix_socket(path).unwrap());
    }
    assert!(is_unix_socket(&endpoint).unwrap());
    drop(listener);
    assert!(is_unix_socket(&endpoint).unwrap());
    assert_eq!(std::fs::read(file).unwrap(), b"keep ordinary content");
    assert!(is_unix_socket(&fixture.0.join("absent")).is_err());
}
