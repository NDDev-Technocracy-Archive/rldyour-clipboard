use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(crate) fn fingerprint(root: &Path) -> std::io::Result<String> {
    let mut files = vec![
        PathBuf::from("Cargo.toml"),
        PathBuf::from("Cargo.lock"),
        PathBuf::from("rust-toolchain.toml"),
    ];
    for directory in ["crates/api", "tools/clipboard-tools"] {
        collect(root, &PathBuf::from(directory), &mut files)?;
    }
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        let content = std::fs::read(root.join(&path))?;
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
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(64);
    for byte in hash.finalize() {
        text.push(HEX[usize::from(byte >> 4)] as char);
        text.push(HEX[usize::from(byte & 15)] as char);
    }
    Ok(text)
}

fn collect(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
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
            collect(root, &path, files)?;
        } else if path.extension().is_some_and(|value| value == "rs")
            || path.file_name().is_some_and(|value| value == "Cargo.toml")
        {
            files.push(path);
        }
    }
    Ok(())
}
