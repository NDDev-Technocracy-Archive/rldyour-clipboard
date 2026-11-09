use crate::qualification::{OwnedProcess, build_bins, private_directory};
use nddev_clipboard_api::wire::Role;
use nddev_clipboard_api::{Config, FileConfig};
use nddev_clipboard_sdk::{Client, Error, LocalOperation, LocalTransport};
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn qualify(root: &Path) -> Result<()> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return Err("retained platform clients require a Linux or macOS execution host".into());
    }
    if std::env::args_os().nth(2).is_some() {
        return Err("unexpected platform-client argument".into());
    }
    let directory = private_directory("cb-client-")?;
    let binaries = build_bins(root, directory.path(), &["rldyour-clipboardd"])?;
    let [daemon] = binaries.as_slice() else {
        return Err("expected the current daemon artifact".into());
    };
    let endpoint = directory.path().join("clipboard.sock");
    let retention_days = 9;
    let config = Config::new(FileConfig {
        schema: 1,
        archive_dir: directory.path().join("archive"),
        socket_path: endpoint.clone(),
        budget_bytes: 8 * 1024 * 1024,
        retention_days,
        capture_native: false,
    })?;
    let profile = directory.path().join("clipboard.toml");
    std::fs::write(&profile, config.to_toml()?)?;
    let mut command = Command::new(&daemon);
    command.arg("--config").arg(&profile).stdout(Stdio::null());
    for key in ["LISTEN_FDS", "LISTEN_PID", "LISTEN_FDNAMES"] {
        command.env_remove(key);
    }
    let mut provider = OwnedProcess::start(&mut command)?;
    let mut client = connect(&endpoint, &mut provider)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    if cfg!(target_os = "linux") {
        let mut command = Command::new("gjs");
        command
            .arg("-m")
            .arg(root.join("extension/tests/client.js"))
            .arg(&profile)
            .arg(&daemon)
            .arg(retention_days.to_string());
        OwnedProcess::start(&mut command)?.finish(deadline)?;
    } else {
        for payload in [
            b"native socket synthetic".to_vec(),
            [
                b"native socket synthetic large:".as_slice(),
                &vec![b'x'; 3 * 1024 * 1024],
            ]
            .concat(),
        ] {
            record(&mut client, &payload)?;
        }
        let executable = directory.path().join("socket-client-test");
        let mut compiler = Command::new("swiftc");
        compiler.args([
            "-swift-version",
            "6",
            "-parse-as-library",
            "-warnings-as-errors",
        ]);
        for source in [
            "macos/Models.swift",
            "macos/GeneratedContract.swift",
            "macos/SocketClient.swift",
            "macos/tests/SocketTests.swift",
        ] {
            compiler.arg(root.join(source));
        }
        compiler.arg("-o").arg(&executable);
        OwnedProcess::start(&mut compiler)?.finish(deadline)?;
        let mut command = Command::new(&executable);
        command
            .arg(&profile)
            .arg(&daemon)
            .arg(retention_days.to_string());
        OwnedProcess::start(&mut command)?.finish(deadline)?;
    }
    Ok(())
}

fn connect(endpoint: &Path, provider: &mut OwnedProcess) -> Result<Client<LocalTransport>> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match LocalOperation::new(deadline).connect(endpoint, Role::Both) {
            Ok(client) => return Ok(client),
            Err(Error::Transport(io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused))
                if Instant::now() < deadline =>
            {
                if provider.0.try_wait()?.is_some() {
                    return Err("owned client fixture provider exited before its greeting".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn record(client: &mut Client<LocalTransport>, payload: &[u8]) -> Result<()> {
    let draft = client.begin(Some("platform client qualification"))?;
    let mut input = payload;
    client.part_from(draft, "text/plain", payload.len() as u64, &mut input)?;
    client.commit(draft)?;
    Ok(())
}
