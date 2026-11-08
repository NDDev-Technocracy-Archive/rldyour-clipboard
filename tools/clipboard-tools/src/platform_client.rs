use nddev_clipboard_api::wire::Role;
use nddev_clipboard_api::{Config, FileConfig};
use nddev_clipboard_sdk::{Client, Error, LocalOperation, LocalTransport};
use serde::Deserialize;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct OwnedProcess(Child);

impl OwnedProcess {
    fn start(command: &mut Command) -> io::Result<Self> {
        Ok(Self(command.stdin(Stdio::null()).spawn()?))
    }

    fn finish(&mut self, deadline: Instant) -> Result<()> {
        loop {
            if let Some(status) = self.0.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err("platform client command failed".into())
                };
            }
            if Instant::now() >= deadline {
                return Err("platform client command exceeded its deadline".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn qualify(root: &Path) -> Result<()> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return Err("retained platform clients require a Linux or macOS execution host".into());
    }
    if std::env::args_os().nth(2).is_some() {
        return Err("unexpected platform-client argument".into());
    }
    #[cfg(unix)]
    let parent = Path::new("/tmp").canonicalize()?;
    #[cfg(windows)]
    let parent = std::env::temp_dir();
    let mut builder = tempfile::Builder::new();
    builder.prefix("cb-client-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    let directory = builder.tempdir_in(parent)?;
    // Build from this checked source and consume Cargo's actual artifact path.
    // An installed or caller-supplied old daemon might ignore the new profile.
    let daemon = build_daemon(root, directory.path())?;
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

#[derive(Deserialize)]
struct BuildMessage {
    reason: String,
    executable: Option<PathBuf>,
    target: Option<BuildTarget>,
}

#[derive(Deserialize)]
struct BuildTarget {
    name: String,
    kind: Vec<String>,
}

fn build_daemon(root: &Path, directory: &Path) -> Result<PathBuf> {
    let path = directory.join("build-messages.jsonl");
    let output = std::fs::File::create_new(&path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "--package",
            "rldyour-clipboardd",
            "--bin",
            "rldyour-clipboardd",
            "--message-format=json",
        ])
        .stdout(output);
    OwnedProcess::start(&mut command)?.finish(Instant::now() + Duration::from_secs(120))?;
    if std::fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
        return Err("compiler artifact receipt exceeded its bound".into());
    }
    let mut artifacts = Vec::new();
    for line in io::BufReader::new(std::fs::File::open(path)?).lines() {
        let message: BuildMessage = serde_json::from_str(&line?)?;
        if message.reason == "compiler-artifact"
            && message.target.as_ref().is_some_and(|target| {
                target.name == "rldyour-clipboardd" && target.kind.iter().any(|kind| kind == "bin")
            })
            && let Some(path) = message.executable
        {
            artifacts.push(path);
        }
    }
    let [executable] = artifacts.as_slice() else {
        return Err("expected one current daemon compiler artifact".into());
    };
    if !executable.is_absolute() || !std::fs::symlink_metadata(executable)?.is_file() {
        return Err("daemon compiler artifact is not an absolute regular file".into());
    }
    Ok(executable.clone())
}
