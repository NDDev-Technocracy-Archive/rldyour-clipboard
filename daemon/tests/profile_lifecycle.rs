//! Actual product processes and private synthetic archives, with capture disabled.
use nddev_clipboard_api::{Config, FileConfig};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::net::UnixStream as Stream;
#[cfg(windows)]
use uds_windows::UnixStream as Stream;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        #[cfg(unix)]
        let parent = PathBuf::from("/tmp").canonicalize().unwrap();
        #[cfg(windows)]
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = parent.join(format!(
            "cb-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self(root)
    }

    fn profile(&self, name: &str, archive: &str, endpoint: &str) -> PathBuf {
        let config = Config::new(FileConfig {
            schema: 1,
            archive_dir: self.0.join(archive),
            socket_path: self.0.join(endpoint),
            budget_bytes: 8 * 1024 * 1024,
            retention_days: 9,
            capture_native: false,
        })
        .unwrap();
        let path = self.0.join(name);
        std::fs::write(&path, config.to_toml().unwrap()).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rldyour-clipboardd"));
    for name in ["LISTEN_FDS", "LISTEN_PID", "LISTEN_FDNAMES"] {
        command.env_remove(name);
    }
    command
}
fn run(args: &[&std::ffi::OsStr]) -> Output {
    command().args(args).output().unwrap()
}

struct Daemon(Child);
impl Daemon {
    fn start(profile: &Path, endpoint: &Path) -> (Self, Peer) {
        let mut process = command();
        process.arg("--config").arg(profile);
        Self::start_command(process, endpoint)
    }

    fn start_command(mut process: Command, endpoint: &Path) -> (Self, Peer) {
        let mut daemon = Self(process.stdout(Stdio::null()).spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(stream) = Stream::connect(endpoint) {
                return (daemon, Peer::new(stream));
            }
            assert!(daemon.0.try_wait().unwrap().is_none(), "daemon exited");
            assert!(
                Instant::now() < deadline,
                "daemon did not publish its endpoint"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}

struct Peer(BufReader<Stream>);
impl Peer {
    fn new(stream: Stream) -> Self {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut peer = Self(BufReader::new(stream));
        let hello = peer.request(json!({"op":"hello", "v":1, "role":"both", "watch":false}));
        assert_eq!(hello["v"], 1);
        assert_eq!(hello["retention_days"], 9);
        peer
    }
    fn send(&mut self, frame: Value) {
        serde_json::to_writer(self.0.get_mut(), &frame).unwrap();
        self.0.get_mut().write_all(b"\n").unwrap();
    }
    fn frame(&mut self) -> Value {
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        assert!(line.len() <= 65536 && line.ends_with('\n'));
        let frame: Value = serde_json::from_str(&line).unwrap();
        assert_ne!(frame["ev"], "error", "{frame}");
        frame
    }
    fn request(&mut self, frame: Value) -> Value {
        self.send(frame);
        self.frame()
    }
    fn record(&mut self, text: &[u8]) -> u64 {
        let draft = self.request(json!({"op":"begin", "req":1}))["draft"].clone();
        self.send(
            json!({"op":"part", "req":2, "draft":draft, "mime":"text/plain", "bytes":text.len()}),
        );
        self.0.get_mut().write_all(text).unwrap();
        assert_eq!(self.frame()["ev"], "ok");
        self.request(json!({"op":"commit", "req":3, "draft":draft}))["entry"]
            .as_u64()
            .unwrap()
    }
}

#[test]
fn configuration_validation_and_discovery_have_no_archive_effects() {
    let fixture = Fixture::new();
    let profile = fixture.profile("clipboard.toml", "unused", "unused.sock");
    let output = run(&[
        "--socket-json".as_ref(),
        "--config".as_ref(),
        profile.as_os_str(),
    ]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<PathBuf>(&output.stdout).unwrap(),
        fixture.0.join("unused.sock")
    );
    assert!(!fixture.0.join("unused").exists());
    assert!(!fixture.0.join("unused.sock").exists());
    std::fs::write(&profile, "capture_native = false\n").unwrap();
    assert!(
        !run(&["--config".as_ref(), profile.as_os_str()])
            .status
            .success()
    );
    assert!(!fixture.0.join("unused").exists());
    assert!(!fixture.0.join("unused.sock").exists());
}

#[test]
fn initialization_never_guesses_an_existing_archives_policy_or_overwrites_a_profile() {
    let fixture = Fixture::new();
    let archive = fixture.0.join("existing");
    std::fs::create_dir(&archive).unwrap();
    let profile = fixture.0.join("clipboard.toml");
    let base = [
        "--init-config".as_ref(),
        "--config".as_ref(),
        profile.as_os_str(),
        "--archive".as_ref(),
        archive.as_os_str(),
    ];
    assert!(!run(&base).status.success());
    assert!(!profile.exists());
    assert_eq!(std::fs::read_dir(&archive).unwrap().count(), 0);
    let endpoint = fixture.0.join("explicit.sock");
    let mut complete = base.to_vec();
    complete.extend([
        "--socket".as_ref(),
        endpoint.as_os_str(),
        "--budget-bytes".as_ref(),
        "0".as_ref(),
        "--retention-days".as_ref(),
        "0".as_ref(),
        "--capture".as_ref(),
        "false".as_ref(),
    ]);
    assert!(run(&complete).status.success());
    let first = std::fs::read(&profile).unwrap();
    assert!(run(&complete).status.success());
    assert_eq!(std::fs::read(&profile).unwrap(), first);
    let last = complete.len() - 1;
    complete[last] = "true".as_ref();
    assert!(!run(&complete).status.success());
    assert_eq!(std::fs::read(&profile).unwrap(), first);
    assert!(!endpoint.exists());
    assert_eq!(std::fs::read_dir(&archive).unwrap().count(), 0);
}

#[test]
fn one_archive_owner_preserves_pins_and_bytes_through_an_abrupt_restart() {
    let fixture = Fixture::new();
    let profile = fixture.profile("clipboard.toml", "archive", "first.sock");
    let (daemon, mut peer) = Daemon::start(&profile, &fixture.0.join("first.sock"));
    let entry = peer.record(b"synthetic durable content");
    assert_eq!(
        peer.request(json!({"op":"pin", "req":4, "entry":entry, "pinned":true}))["ev"],
        "ok"
    );
    let other = fixture.profile("other.toml", "archive", "second.sock");
    let mut second = Daemon(command().arg("--config").arg(&other).spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = second.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "second archive owner stayed alive"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!fixture.0.join("second.sock").exists());
    assert_eq!(peer.request(json!({"op":"stats", "req":5}))["pinned"], 1);
    drop(peer);
    drop(daemon);
    let (_restarted, mut peer) = Daemon::start(&profile, &fixture.0.join("first.sock"));
    assert_eq!(peer.request(json!({"op":"clear", "req":6}))["ev"], "ok");
    let items = peer.request(json!({"op":"list", "req":7, "pinned":true}));
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    assert_eq!(items["items"][0]["id"], entry);
    let blob = peer.request(json!({"op":"fetch", "req":8, "entry":entry}));
    let mut bytes = vec![0; blob["bytes"].as_u64().unwrap().try_into().unwrap()];
    peer.0.read_exact(&mut bytes).unwrap();
    assert_eq!(bytes, b"synthetic durable content");
}

#[test]
fn the_rust_sdk_streams_real_multiformat_entries_and_recovers_from_a_refused_request() {
    use nddev_clipboard_api::wire::Role;
    use nddev_clipboard_sdk::{Client, Error};

    let fixture = Fixture::new();
    let profile = fixture.profile("clipboard.toml", "archive", "sdk.sock");
    let endpoint = fixture.0.join("sdk.sock");
    let (_daemon, _peer) = Daemon::start(&profile, &endpoint);
    let stream = Stream::connect(endpoint).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut client = Client::new(stream, Role::Both).unwrap();
    assert!(client.list(10, None, None, None).unwrap().items.is_empty());

    let payload: Vec<u8> = (0..=255).cycle().take(3 * 1024 * 1024).collect();
    let html = b"<p>Isolated SDK fixture</p>";
    let draft = client.begin(Some("SDK qualification")).unwrap();
    client
        .part_from(
            draft,
            "application/octet-stream",
            payload.len() as u64,
            &mut payload.as_slice(),
        )
        .unwrap();
    client
        .part_from(draft, "text/html", html.len() as u64, &mut html.as_slice())
        .unwrap();
    let entry = client.commit(draft).unwrap();
    client.pin(entry, true).unwrap();
    let page = client.list(10, None, None, Some(true)).unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, entry);
    assert_eq!(page.items[0].mimes.len(), 2);
    assert!(!page.more);

    let mut restored = Vec::new();
    let blob = client
        .fetch_to(
            entry,
            Some("application/octet-stream"),
            false,
            &mut restored,
        )
        .unwrap();
    assert_eq!(blob.bytes, payload.len() as u64);
    assert_eq!(blob.mime, "application/octet-stream");
    assert_eq!(restored, payload);
    restored.clear();
    client
        .fetch_to(entry, Some("text/html"), false, &mut restored)
        .unwrap();
    assert_eq!(restored, html);
    assert!(
        matches!(client.pin(i64::MAX, true), Err(Error::Remote { code }) if code == "no-such-entry")
    );
    assert_eq!(
        client.list(10, None, None, Some(true)).unwrap().items[0].id,
        entry
    );

    let discarded = client.begin(None).unwrap();
    client.abort(discarded).unwrap();
    assert_eq!(client.list(10, None, None, None).unwrap().items.len(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn systemd_activation_serves_the_triggering_connection_without_a_second_listener() {
    let fixture = Fixture::new();
    let profile = fixture.profile("clipboard.toml", "archive", "systemd.sock");
    let endpoint = fixture.0.join("systemd.sock");
    let mut process = Command::new("systemd-socket-activate");
    process
        .arg("--listen")
        .arg(&endpoint)
        .arg(env!("CARGO_BIN_EXE_rldyour-clipboardd"))
        .arg("--config")
        .arg(&profile);
    for name in ["LISTEN_FDS", "LISTEN_PID", "LISTEN_FDNAMES"] {
        process.env_remove(name);
    }
    let (_daemon, mut peer) = Daemon::start_command(process, &endpoint);
    let entry = peer.record(b"systemd activation fixture");
    assert_eq!(
        peer.request(json!({"op":"pin", "req":4, "entry":entry, "pinned":true}))["ev"],
        "ok"
    );
    assert_eq!(
        peer.request(json!({"op":"list", "req":5, "pinned":true}))["items"][0]["id"],
        entry
    );
}

#[cfg(target_os = "macos")]
#[test]
fn launchd_activation_serves_the_triggering_connection_without_a_second_listener() {
    fn xml(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
    struct Job {
        target: String,
        active: bool,
        log: PathBuf,
    }
    impl Job {
        fn stop(&mut self) {
            let result = Command::new("launchctl")
                .args(["bootout", &self.target])
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "launchd fixture cleanup failed: {}",
                String::from_utf8_lossy(&result.stderr)
            );
            self.active = false;
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            if self.active && std::thread::panicking() {
                if let Ok(log) = std::fs::read_to_string(&self.log) {
                    eprintln!("owned launchd fixture stderr: {log}");
                }
                if let Ok(state) = Command::new("launchctl")
                    .args(["print", &self.target])
                    .output()
                {
                    let state = String::from_utf8_lossy(&state.stdout);
                    let summary = state
                        .lines()
                        .map(str::trim)
                        .filter(|line| {
                            ["state =", "runs =", "last exit code =", "pid ="]
                                .iter()
                                .any(|prefix| line.starts_with(prefix))
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    eprintln!("owned launchd fixture state: {summary}");
                }
            }
            if self.active {
                let _ = Command::new("launchctl")
                    .args(["bootout", &self.target])
                    .output();
            }
        }
    }
    let fixture = Fixture::new();
    let profile = fixture.profile("clipboard.toml", "archive", "launchd.sock");
    let endpoint = fixture.0.join("launchd.sock");
    let identity = Command::new("id").arg("-u").output().unwrap();
    assert!(identity.status.success());
    let uid: u32 = String::from_utf8(identity.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let domain = format!("gui/{uid}");
    let label = format!("ai.nddev.clipboard.fixture.{}", std::process::id());
    let plist = fixture.0.join("activation.plist");
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{label}</string>
<key>ProgramArguments</key><array><string>{binary}</string><string>--config</string><string>{profile}</string></array>
<key>StandardErrorPath</key><string>{log}</string>
<key>Sockets</key><dict><key>sock</key><dict><key>SockFamily</key><string>Unix</string><key>SockType</key><string>stream</string><key>SockPassive</key><true/><key>SockPathName</key><string>{endpoint}</string><key>SockPathMode</key><integer>384</integer></dict></dict>
</dict></plist>"#,
        binary = xml(env!("CARGO_BIN_EXE_rldyour-clipboardd")),
        profile = xml(profile.to_str().unwrap()),
        log = xml(fixture.0.join("activation.log").to_str().unwrap()),
        endpoint = xml(endpoint.to_str().unwrap())
    );
    std::fs::write(&plist, content).unwrap();
    let checked = Command::new("plutil")
        .arg("-lint")
        .arg(&plist)
        .output()
        .unwrap();
    assert!(checked.status.success(), "invalid launchd fixture");
    let loaded = Command::new("launchctl")
        .arg("bootstrap")
        .arg(&domain)
        .arg(&plist)
        .output()
        .unwrap();
    assert!(
        loaded.status.success(),
        "launchd fixture bootstrap failed: {}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    let mut job = Job {
        target: format!("{domain}/{label}"),
        active: true,
        log: fixture.0.join("activation.log"),
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    let stream = loop {
        if let Ok(stream) = Stream::connect(&endpoint) {
            break stream;
        }
        assert!(
            Instant::now() < deadline,
            "launchd did not publish the fixture endpoint"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut peer = Peer::new(stream);
    let entry = peer.record(b"launchd activation fixture");
    assert_eq!(
        peer.request(json!({"op":"pin", "req":4, "entry":entry, "pinned":true}))["ev"],
        "ok"
    );
    assert_eq!(
        peer.request(json!({"op":"list", "req":5, "pinned":true}))["items"][0]["id"],
        entry
    );
    drop(peer);
    job.stop();
}
