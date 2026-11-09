//! Owned execution-host fixtures shared by the real product client gates.
use serde::Deserialize;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(super) struct OwnedProcess(pub(super) Child);

impl OwnedProcess {
    pub(super) fn start(command: &mut Command) -> io::Result<Self> {
        Ok(Self(command.stdin(Stdio::null()).spawn()?))
    }

    pub(super) fn finish(&mut self, deadline: Instant) -> Result<()> {
        loop {
            if let Some(status) = self.0.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err("owned qualification command failed".into())
                };
            }
            if Instant::now() >= deadline {
                return Err("owned qualification command exceeded its deadline".into());
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

pub(super) fn private_directory(prefix: &str) -> io::Result<tempfile::TempDir> {
    #[cfg(unix)]
    let parent = Path::new("/tmp").canonicalize()?;
    #[cfg(windows)]
    let parent = std::env::temp_dir().canonicalize()?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(parent)
}

#[derive(Deserialize)]
struct BuildMessage {
    reason: String,
    executable: Option<PathBuf>,
    target: Option<BuildTarget>,
    message: Option<Diagnostic>,
}

#[derive(Deserialize)]
struct BuildTarget {
    name: String,
    kind: Vec<String>,
}

#[derive(Deserialize)]
struct Diagnostic {
    rendered: Option<String>,
}

/// Each selected product package has one matching binary. Return Cargo's actual
/// executable paths; an installed binary or a guessed target directory is not evidence.
pub(super) fn build_bins(root: &Path, directory: &Path, names: &[&str]) -> Result<Vec<PathBuf>> {
    let path = directory.join("build-messages.jsonl");
    let output = std::fs::File::create_new(&path)?;
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args(["build", "--locked", "--bins", "--message-format=json"]);
    for name in names {
        command.args(["--package", name]);
    }
    command.stdout(output);
    let result =
        OwnedProcess::start(&mut command)?.finish(Instant::now() + Duration::from_secs(120));
    if std::fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
        return Err("compiler artifact receipt exceeded its bound".into());
    }
    let mut artifacts = vec![Vec::new(); names.len()];
    for line in io::BufReader::new(std::fs::File::open(path)?).lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let message: BuildMessage = serde_json::from_str(&line)?;
        if let Some(rendered) = message.message.and_then(|diagnostic| diagnostic.rendered) {
            eprint!("{rendered}");
        }
        if message.reason == "compiler-artifact"
            && let Some(target) = message.target
            && target.kind.iter().any(|kind| kind == "bin")
            && let Some(index) = names.iter().position(|name| *name == target.name)
            && let Some(path) = message.executable
        {
            artifacts[index].push(path);
        }
    }
    result?;
    artifacts
        .into_iter()
        .map(|paths| {
            let [executable] = paths.as_slice() else {
                return Err("expected one compiler artifact per product binary".into());
            };
            if !executable.is_absolute() || !std::fs::symlink_metadata(executable)?.is_file() {
                return Err("compiler artifact is not an absolute regular file".into());
            }
            Ok(executable.clone())
        })
        .collect()
}
