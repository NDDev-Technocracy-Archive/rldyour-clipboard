use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) const INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "daemon/tests/fixtures/baseline-release.json",
    "crates/api",
    "tools/clipboard-tools",
];

pub(crate) fn fingerprint(root: &Path, directory: Option<&Path>) -> std::io::Result<String> {
    let mut files = Vec::new();
    if let Some(directory) = directory {
        collect(root, directory, &mut files, true)?;
    } else {
        for input in INPUTS {
            let relative = PathBuf::from(input);
            if std::fs::symlink_metadata(root.join(input))?.is_dir() {
                collect(root, &relative, &mut files, false)?;
            } else {
                files.push(relative);
            }
        }
    }
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        let file = root.join(&path);
        if !std::fs::symlink_metadata(&file)?.is_file() {
            return Err(std::io::Error::other("source input must be a regular file"));
        }
        let content = std::fs::read(file)?;
        let name = path
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update((content.len() as u64).to_le_bytes());
        hash.update(content);
    }
    Ok(hex(&hash.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[usize::from(byte >> 4)] as char);
        text.push(HEX[usize::from(byte & 15)] as char);
    }
    text
}

fn collect(
    root: &Path,
    relative: &Path,
    files: &mut Vec<PathBuf>,
    all: bool,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = relative.join(entry.file_name());
        if kind.is_symlink() {
            return Err(std::io::Error::other(
                "tool source must not contain filesystem links",
            ));
        }
        if kind.is_dir() {
            collect(root, &path, files, all)?;
        } else if all
            || path.extension().is_some_and(|value| value == "rs")
            || path.file_name().is_some_and(|value| value == "Cargo.toml")
        {
            files.push(path);
        }
    }
    Ok(())
}
