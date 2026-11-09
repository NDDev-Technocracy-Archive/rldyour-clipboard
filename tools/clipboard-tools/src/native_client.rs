//! Drive the real native CLI and daemon with a private synthetic archive.
use crate::qualification::{OwnedProcess, build_bins, private_directory};
use nddev_clipboard_api::wire::{Response, Role};
use nddev_clipboard_api::{Config, FileConfig};
use nddev_clipboard_sdk::{Client, Error, LocalOperation, LocalTransport};
use serde_json::Value;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture<'a> {
    binary: &'a Path,
    directory: &'a Path,
    profile: &'a Path,
}

impl Fixture<'_> {
    fn output(&self, arguments: &[&OsStr], success: bool) -> Result<Vec<u8>> {
        let stdout = self.directory.join("command.stdout");
        let stderr = self.directory.join("command.stderr");
        let mut command = Command::new(self.binary);
        command
            .arg("--config")
            .arg(self.profile)
            .args(arguments)
            .stdout(std::fs::File::create(&stdout)?)
            .stderr(std::fs::File::create(&stderr)?);
        let mut process = OwnedProcess::start(&mut command)?;
        let finished = process.finish(Instant::now() + Duration::from_secs(15));
        let status = process.0.try_wait()?.ok_or("CLI did not finish")?;
        if status.success() != success {
            return Err("native CLI returned an unexpected exit status".into());
        }
        if success {
            finished?;
            require(
                std::fs::metadata(&stderr)?.len() == 0,
                "successful CLI wrote diagnostics",
            )?;
        }
        let bound = 4 * 1024 * 1024;
        require(
            std::fs::metadata(&stdout)?.len() <= bound,
            "CLI output exceeded its fixture bound",
        )?;
        require(
            std::fs::metadata(&stderr)?.len() <= 64 * 1024,
            "CLI diagnostics exceeded their fixture bound",
        )?;
        Ok(std::fs::read(stdout)?)
    }

    fn json(&self, arguments: &[&str]) -> Result<Value> {
        let arguments: Vec<_> = arguments.iter().map(OsStr::new).collect();
        Ok(serde_json::from_slice(&self.output(&arguments, true)?)?)
    }
}

pub(crate) fn qualify(root: &Path) -> Result<()> {
    if std::env::args_os().nth(2).is_some() {
        return Err("unexpected native-client argument".into());
    }
    let directory = private_directory("cb-cli-")?;
    let binaries = build_bins(
        root,
        directory.path(),
        &["rldyour-clipboardd", "nddev-clipboard"],
    )?;
    let [daemon, binary] = binaries.as_slice() else {
        return Err("expected the current daemon and native CLI artifacts".into());
    };
    let missing = directory.path().join("absent.toml");
    let fixture = Fixture {
        binary,
        directory: directory.path(),
        profile: &missing,
    };
    for argument in ["--help", "--version"] {
        require(
            !fixture.output(&[OsStr::new(argument)], true)?.is_empty(),
            "CLI informational output is missing",
        )?;
    }
    for arguments in [
        vec!["get", "9223372036854775808"],
        vec!["remove", "0"],
        vec!["list", "--favorites", "--unpinned"],
        vec!["list", "--limit", "0"],
        vec!["--timeout-ms", "0", "stats"],
        vec!["--timeout-ms", "1", "watch", "--seconds", "1"],
        vec!["watch"],
        vec!["stats", "unexpected"],
    ] {
        let arguments: Vec<_> = arguments.into_iter().map(OsStr::new).collect();
        require(
            fixture.output(&arguments, false)?.is_empty(),
            "refused CLI emitted content",
        )?;
    }
    require(
        !missing.exists(),
        "argument handling created a missing profile",
    )?;

    let endpoint = directory.path().join("clipboard.sock");
    let config = Config::new(FileConfig {
        schema: 1,
        archive_dir: directory.path().join("archive"),
        socket_path: endpoint.clone(),
        budget_bytes: 8 * 1024 * 1024,
        retention_days: 9,
        capture_native: false,
    })?;
    let profile = directory.path().join("clipboard.toml");
    std::fs::write(&profile, config.to_toml()?)?;
    let mut command = Command::new(daemon);
    command.arg("--config").arg(&profile).stdout(Stdio::null());
    for key in ["LISTEN_FDS", "LISTEN_PID", "LISTEN_FDNAMES"] {
        command.env_remove(key);
    }
    let mut provider = OwnedProcess::start(&mut command)?;
    let mut writer = connect(&endpoint, &mut provider)?;
    let fixture = Fixture {
        binary,
        directory: directory.path(),
        profile: &profile,
    };
    let small = b"native CLI synthetic \x1b[31m\n\"quoted\"";
    let large = vec![b'x'; 3 * 1024 * 1024];
    let binary_representation = b"\x00\xff\x80\xfe\r\n\x1a\x00";
    let draft = writer.begin(Some("native CLI qualification"))?;
    let mut text_input = small.as_slice();
    writer.part_from(draft, "text/plain", small.len() as u64, &mut text_input)?;
    let mut binary_input = binary_representation.as_slice();
    writer.part_from(
        draft,
        "application/octet-stream",
        binary_representation.len() as u64,
        &mut binary_input,
    )?;
    let first = writer.commit(draft)?;
    let second = record(&mut writer, &large)?;
    let first_key = first.to_string();
    let second_key = second.to_string();
    let page = fixture.json(&["list", "--query", "native", "--kind", "text"])?;
    require(
        page["items"]
            .as_array()
            .is_some_and(|items| items.len() == 1),
        "CLI query/kind did not filter the archive",
    )?;
    require(
        page["items"][0]["key"] == first_key,
        "CLI key changed its exact archive identifier",
    )?;
    let raw = fixture.output(&[OsStr::new("list")], true)?;
    require(
        !raw.contains(&0x1b),
        "CLI did not escape terminal control content",
    )?;
    let page = fixture.json(&["list", "--limit", "1"])?;
    require(
        page["more"] == true
            && page["items"]
                .as_array()
                .is_some_and(|items| items.len() == 1),
        "CLI did not preserve provider paging",
    )?;
    let page = fixture.json(&["list", "--before", &second_key])?;
    require(
        page["items"][0]["key"] == first_key,
        "CLI cursor lost the older entry",
    )?;
    require(
        fixture.output(&[OsStr::new("get"), OsStr::new(&second_key)], true)? == large,
        "CLI large retrieval changed the original bytes",
    )?;
    require(
        fixture.output(
            &[
                OsStr::new("get"),
                OsStr::new(&first_key),
                OsStr::new("--mime"),
                OsStr::new("application/octet-stream"),
            ],
            true,
        )? == binary_representation,
        "CLI binary output changed non-UTF-8 representation bytes",
    )?;
    fixture.json(&["pin", &first_key])?;
    let page = fixture.json(&["list", "--favorites"])?;
    require(
        page["items"]
            .as_array()
            .is_some_and(|items| items.len() == 1)
            && page["items"][0]["key"] == first_key,
        "CLI favorites lost the pinned entry",
    )?;
    let stats = fixture.json(&["stats"])?;
    require(
        stats["entries"] == 2
            && stats["pinned"] == 1
            && stats["retention_days"] == 9
            && stats["budget"] == 8 * 1024 * 1024,
        "CLI statistics did not reflect the real profile/archive",
    )?;
    fixture.output(
        &[
            OsStr::new("get"),
            OsStr::new(&first_key),
            OsStr::new("--mime"),
            OsStr::new("application/no-such-mime"),
        ],
        false,
    )?;
    fixture.output(
        &[OsStr::new("get"), OsStr::new("9223372036854775807")],
        false,
    )?;
    fixture.json(&["clear"])?;
    require(
        fixture.json(&["stats"])?["entries"] == 1,
        "CLI clear did not preserve the favorite",
    )?;
    fixture.json(&["unpin", &first_key])?;
    fixture.json(&["remove", &first_key])?;
    require(
        fixture.json(&["stats"])?["entries"] == 0,
        "CLI management did not remove the selected entry",
    )?;

    // The SDK's real separate subscription establishes readiness through its
    // greeting, then receives four native archive mutations in order.
    let mut subscription =
        LocalOperation::new(Instant::now() + Duration::from_secs(5)).subscribe(&endpoint)?;
    let observed = record(&mut writer, b"subscription synthetic")?;
    require(
        matches!(subscription.next_event()?, Response::Added { entry } if entry.id == observed),
        "subscription missed its first post-greeting add",
    )?;
    writer.pin(observed, true)?;
    require(
        matches!(subscription.next_event()?, Response::Updated { entry } if entry.id == observed && entry.pinned),
        "subscription did not observe the favorite update",
    )?;
    writer.remove(observed)?;
    require(
        matches!(subscription.next_event()?, Response::Removed { entry } if entry == observed),
        "subscription did not observe removal",
    )?;
    writer.clear()?;
    require(
        matches!(subscription.next_event()?, Response::Cleared {}),
        "subscription did not observe clear",
    )?;
    drop(subscription);

    let operation = LocalOperation::new(Instant::now() + Duration::from_secs(5));
    let cancel = operation.cancellation();
    let mut subscription = operation.subscribe(&endpoint)?;
    cancel.cancel();
    require(
        matches!(
            subscription.next_event(),
            Err(Error::Transport(io::ErrorKind::ConnectionAborted))
        ),
        "subscription cancellation did not close its own operation",
    )?;
    drop(subscription);
    let mut subscription =
        LocalOperation::new(Instant::now() + Duration::from_millis(200)).subscribe(&endpoint)?;
    require(
        matches!(
            subscription.next_event(),
            Err(Error::Transport(io::ErrorKind::TimedOut))
        ),
        "idle subscription outlived its absolute deadline",
    )?;

    // Observe the actual CLI's flushed greeting before one mutation. This is a
    // readiness boundary, rather than sleeping or repeating an event until seen.
    let watch_output = directory.path().join("watch.stdout");
    let watch_error = directory.path().join("watch.stderr");
    let mut command = Command::new(binary);
    command
        .arg("--config")
        .arg(&profile)
        .args(["watch", "--seconds", "1"])
        .stdout(std::fs::File::create_new(&watch_output)?)
        .stderr(std::fs::File::create_new(&watch_error)?);
    let mut watcher = OwnedProcess::start(&mut command)?;
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frames = frames(&watch_output)?;
        if !frames.is_empty() {
            require(
                matches!(frames[0], Response::Hello { v: 1, .. }),
                "CLI watch did not establish a valid greeting",
            )?;
            break;
        }
        require(
            watcher.0.try_wait()?.is_none(),
            "CLI watch exited before readiness",
        )?;
        require(
            Instant::now() < ready_deadline,
            "CLI watch greeting exceeded its fixture deadline",
        )?;
        std::thread::sleep(Duration::from_millis(5));
    }
    let watched = record(&mut writer, b"CLI watch synthetic")?;
    watcher.finish(ready_deadline)?;
    let events = frames(&watch_output)?;
    require(
        matches!(events.as_slice(), [Response::Hello { .. }, Response::Added { entry }] if entry.id == watched),
        "CLI watch lost or changed the first post-greeting mutation",
    )?;
    require(
        std::fs::metadata(&watch_error)?.len() == 0,
        "successful CLI watch wrote diagnostics",
    )?;

    // A successful CLI watch ends at its own finite lifetime even without events.
    let started = Instant::now();
    let idle = fixture.output(
        &[
            OsStr::new("watch"),
            OsStr::new("--seconds"),
            OsStr::new("1"),
        ],
        true,
    )?;
    require(
        matches!(
            serde_json::from_slice::<Response>(&idle)?,
            Response::Hello { v: 1, .. }
        ),
        "idle CLI watch emitted more than its greeting",
    )?;
    require(
        started.elapsed() >= Duration::from_secs(1) && started.elapsed() < Duration::from_secs(4),
        "CLI watch did not honor its finite observation lifetime",
    )?;
    println!(
        "native CLI: real browsing, exact streamed bytes, favorites, policy, management and bounded separate notifications passed"
    );
    Ok(())
}

fn record(client: &mut Client<LocalTransport>, payload: &[u8]) -> Result<i64> {
    let draft = client.begin(Some("native CLI qualification"))?;
    let mut input = payload;
    client.part_from(draft, "text/plain", payload.len() as u64, &mut input)?;
    Ok(client.commit(draft)?)
}

fn frames(path: &Path) -> Result<Vec<Response>> {
    require(
        std::fs::metadata(path)?.len() <= 128 * 1024,
        "watch fixture exceeded its output bound",
    )?;
    let bytes = std::fs::read(path)?;
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| line.last() == Some(&b'\n'))
        .map(|line| serde_json::from_slice(line).map_err(Into::into))
        .collect()
}

fn connect(endpoint: &Path, provider: &mut OwnedProcess) -> Result<Client<LocalTransport>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match LocalOperation::new(deadline).connect(endpoint, Role::Both) {
            Ok(client) => return Ok(client),
            Err(Error::Transport(io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused))
                if Instant::now() < deadline =>
            {
                require(
                    provider.0.try_wait()?.is_none(),
                    "owned provider exited before greeting",
                )?;
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn require(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
