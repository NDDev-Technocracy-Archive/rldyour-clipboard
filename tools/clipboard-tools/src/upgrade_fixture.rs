//! Pinned unmodified release fixture. No installer or daemon runs here.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Component, Path};
use std::process::{Command, Stdio};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    schema: u32,
    repository: String,
    repository_id: u64,
    release_id: u64,
    tag: String,
    artifacts: BTreeMap<String, Artifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    asset_id: u64,
    name: String,
    size: u64,
    sha256: String,
    member: String,
    binary: String,
}

pub(super) fn prepare(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let release: Release = serde_json::from_slice(&std::fs::read(
        root.join("daemon/tests/fixtures/baseline-release.json"),
    )?)?;
    let target = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let artifact = release
        .artifacts
        .get(&target)
        .ok_or("unsupported fixture target")?;
    if release.schema != 1
        || !release
            .repository
            .starts_with("NDDev-Technocracy-OpenNetwork/")
    {
        return Err("invalid release fixture identity".into());
    }
    for name in [&artifact.name, &artifact.member, &artifact.binary] {
        if Path::new(name)
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
            || name.starts_with('-')
            || name.contains('\\')
            || name.contains(':')
        {
            return Err("release fixture paths must be confined".into());
        }
    }
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid release fixture digest".into());
    }
    check_identity(
        &format!("repos/{}", release.repository),
        release.repository_id,
    )?;
    check_identity(
        &format!(
            "repos/{}/releases/{}",
            release.repository, release.release_id
        ),
        release.release_id,
    )?;
    let directory = root.join(".build/upgrade-fixture").join(&target);
    std::fs::create_dir_all(&directory)?;
    let archive = directory.join(&artifact.name);
    struct Partial(Option<std::path::PathBuf>);
    impl Drop for Partial {
        fn drop(&mut self) {
            if let Some(path) = &self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let mut partial = Partial(None);
    match std::fs::symlink_metadata(&archive) {
        Ok(metadata) if !metadata.is_file() => {
            return Err("fixture archive must be a regular file".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let temporary =
                directory.join(format!("{}.partial-{}", artifact.name, std::process::id()));
            let file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)?;
            partial.0 = Some(temporary);
            let status = Command::new("gh")
                .args([
                    "api",
                    &format!(
                        "repos/{}/releases/assets/{}",
                        release.repository, artifact.asset_id
                    ),
                    "--header",
                    "Accept: application/octet-stream",
                ])
                .env("GH_PROMPT_DISABLED", "1")
                .stdin(Stdio::null())
                .stdout(file)
                .status()?;
            if !status.success() {
                return Err("baseline release asset download failed".into());
            }
        }
        Err(error) => return Err(error.into()),
    }
    let mut file = std::fs::File::open(partial.0.as_deref().unwrap_or(&archive))?;
    if file.metadata()?.len() != artifact.size {
        return Err("baseline release archive size mismatch".into());
    }
    let mut hash = Sha256::new();
    let mut chunk = [0; 64 * 1024];
    loop {
        let bytes = file.read(&mut chunk)?;
        if bytes == 0 {
            break;
        }
        hash.update(&chunk[..bytes]);
    }
    if !crate::fingerprint::hex(&hash.finalize()).eq_ignore_ascii_case(&artifact.sha256) {
        return Err("baseline release archive digest mismatch".into());
    }
    drop(file);
    if let Some(temporary) = partial.0.take() {
        std::fs::rename(temporary, &archive)?;
    }
    // Select only the pinned executable member, not an installer or UI bundle.
    #[cfg(windows)]
    let tar =
        std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot is unset")?)
            .join("System32/tar.exe");
    #[cfg(not(windows))]
    let tar = std::path::PathBuf::from("/usr/bin/tar");
    if !Command::new(tar)
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&directory)
        .arg(&artifact.member)
        .stdin(Stdio::null())
        .status()?
        .success()
    {
        return Err("baseline executable extraction failed".into());
    }
    let binary = directory.join(&artifact.binary);
    if !std::fs::symlink_metadata(&binary)?.is_file()
        || !binary
            .canonicalize()?
            .starts_with(directory.canonicalize()?)
    {
        return Err("extracted baseline executable is not confined".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700))?;
    }
    println!("verified baseline fixture {} for {target}", release.tag);
    Ok(())
}

fn check_identity(endpoint: &str, expected: u64) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("gh")
        .args([
            "api",
            endpoint,
            "--header",
            "Accept: application/vnd.github+json",
            "--jq",
            ".id",
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success()
        || output.stdout.len() > 32
        || String::from_utf8(output.stdout)?.trim().parse::<u64>()? != expected
    {
        return Err("baseline release identity mismatch".into());
    }
    Ok(())
}
