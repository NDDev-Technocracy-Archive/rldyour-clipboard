use nddev_clipboard_api::{Config, ConfigError, FileConfig};
use std::path::{Path, PathBuf};

fn model(root: &Path) -> FileConfig {
    FileConfig {
        schema: 1,
        archive_dir: root.join("archive"),
        socket_path: root.join("s"),
        budget_bytes: 5 * 1024 * 1024 * 1024,
        retention_days: 7,
        capture_native: false,
    }
}

#[test]
fn file_settings_round_trip_without_opening_the_archive() {
    let root = tempfile::tempdir().unwrap();
    let settings = Config::new(model(root.path())).unwrap();
    let file = root.path().join("clipboard.toml");
    std::fs::write(&file, settings.to_toml().unwrap()).unwrap();
    let parsed = Config::read(&file).unwrap();
    assert_eq!(parsed.archive_dir(), root.path().join("archive"));
    assert_eq!(parsed.socket_path(), root.path().join("s"));
    assert_eq!(parsed.budget_bytes(), 5 * 1024 * 1024 * 1024);
    assert_eq!(parsed.retention_seconds(), Some(604_800));
    assert!(!parsed.capture_native());
    assert!(!root.path().join("archive").exists());
    assert!(!root.path().join("s").exists());
}

#[test]
fn configuration_has_no_missing_field_or_invalid_value_fallback() {
    let root = tempfile::tempdir().unwrap();
    let text = Config::new(model(root.path())).unwrap().to_toml().unwrap();
    for line in text.lines().filter(|line| line.contains('=')) {
        let incomplete = text
            .lines()
            .filter(|candidate| *candidate != line)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            Config::parse(&incomplete),
            Err(ConfigError::Format)
        ));
    }
    assert!(matches!(
        Config::parse(&(text.clone() + "unexpected = 1\n")),
        Err(ConfigError::Format)
    ));
    assert!(matches!(
        Config::parse(&(text + "schema = 1\n")),
        Err(ConfigError::Format)
    ));
    let mut value = model(root.path());
    value.schema = 2;
    assert!(matches!(Config::new(value), Err(ConfigError::Schema)));
    let mut value = model(root.path());
    value.budget_bytes = u64::MAX;
    assert!(matches!(Config::new(value), Err(ConfigError::Budget)));
    let mut value = model(root.path());
    value.retention_days = 36_501;
    assert!(matches!(Config::new(value), Err(ConfigError::Retention)));
}

#[test]
fn paths_cannot_select_network_transports_or_parent_traversal() {
    let root = tempfile::tempdir().unwrap();
    for path in ["127.0.0.1:12345", "tcp://127.0.0.1:12345", "relative.sock"] {
        let mut value = model(root.path());
        value.socket_path = PathBuf::from(path);
        assert!(matches!(Config::new(value), Err(ConfigError::Socket)));
    }
    let mut value = model(root.path());
    value.archive_dir = root.path().join("child/../archive");
    assert!(matches!(Config::new(value), Err(ConfigError::Archive)));
    let mut value = model(root.path());
    value.socket_path = root.path().join("s".repeat(200));
    assert!(matches!(Config::new(value), Err(ConfigError::Socket)));
}

#[test]
fn explicit_zero_disables_only_the_selected_eviction_policy() {
    let root = tempfile::tempdir().unwrap();
    let mut value = model(root.path());
    value.retention_days = 0;
    value.budget_bytes = 0;
    let settings = Config::new(value).unwrap();
    assert_eq!(settings.retention_seconds(), None);
    assert_eq!(settings.budget_bytes(), i64::MAX);
}

#[test]
fn untrusted_toml_never_appears_in_configuration_errors() {
    let error = Config::parse("private-token-value and private-path are not TOML")
        .err()
        .unwrap();
    assert!(!format!("{error:?} {error}").contains("private-token-value"));
}

#[test]
fn size_and_file_type_boundaries_precede_parsing() {
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(
        Config::read(root.path()),
        Err(ConfigError::FileType) | Err(ConfigError::Io(_))
    ));
    let file = root.path().join("large.toml");
    std::fs::write(&file, vec![b'x'; 16 * 1024 + 1]).unwrap();
    assert!(matches!(Config::read(&file), Err(ConfigError::TooLarge)));
}

#[cfg(unix)]
#[test]
fn redirected_files_and_fifos_are_refused_without_following_or_waiting() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("settings.toml");
    std::fs::write(
        &file,
        Config::new(model(root.path())).unwrap().to_toml().unwrap(),
    )
    .unwrap();
    let link = root.path().join("link.toml");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert!(Config::read(&link).is_err());
    let fifo = root.path().join("fifo.toml");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(matches!(Config::read(&fifo), Err(ConfigError::FileType)));
}
