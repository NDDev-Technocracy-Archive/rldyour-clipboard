//! Flat manifests for quiescent, caller-owned release outputs, never a user's archive.
use crate::fingerprint::hex;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{File, symlink_metadata};
use std::io::{Read, Write};
use std::path::{Component, Path};

const MANIFEST: &str = "SHA256SUMS";
const MAX_ENTRIES: usize = 1024;

pub(super) fn write(root: &Path, relative: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if relative.as_os_str().is_empty() {
        return Err("an owned artifact directory is required".into());
    }
    let mut directory = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("artifact directory must be a normal relative path".into());
        };
        directory.push(name);
        if !symlink_metadata(&directory)?.is_dir() {
            return Err("artifact directory is redirected or not a directory".into());
        }
    }
    let target = directory.join(MANIFEST);
    match symlink_metadata(&target) {
        Ok(metadata) if !metadata.is_file() => {
            return Err("checksum manifest must be a regular file".into());
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    let mut names = Vec::new();
    let mut identities = BTreeSet::new();
    for (index, entry) in std::fs::read_dir(&directory)?.enumerate() {
        if index >= MAX_ENTRIES {
            return Err("artifact directory exceeds its entry limit".into());
        }
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            continue;
        }
        if !kind.is_file() {
            return Err("artifact entries must be regular files or untraversed directories".into());
        }
        let name = entry.file_name();
        let name = name.to_str().ok_or("artifact filename must be UTF-8")?;
        if name == MANIFEST {
            continue;
        }
        if name.eq_ignore_ascii_case(MANIFEST)
            || !portable_name(name)
            || !identities.insert(name.to_ascii_lowercase())
        {
            return Err("artifact filenames must be unique portable release names".into());
        }
        names.push(name.to_owned());
    }
    if names.is_empty() {
        return Err("cannot create an empty artifact manifest".into());
    }
    names.sort();
    let mut text = String::new();
    let mut buffer = vec![0u8; 64 * 1024];
    for name in &names {
        let path = directory.join(name);
        if !symlink_metadata(&path)?.is_file() {
            return Err("artifact changed before hashing".into());
        }
        let mut source = File::open(path)?;
        let before = source.metadata()?;
        if !before.is_file() {
            return Err("artifact is not a regular file".into());
        }
        let mut hash = Sha256::new();
        let mut length = 0u64;
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            length = length
                .checked_add(count as u64)
                .ok_or("artifact size overflow")?;
            hash.update(&buffer[..count]);
        }
        let after = source.metadata()?;
        if length != before.len()
            || after.len() != before.len()
            || after.modified()? != before.modified()?
        {
            return Err("artifact changed while hashing".into());
        }
        text.push_str(&hex(&hash.finalize()));
        text.push_str("  ");
        text.push_str(name);
        text.push('\n');
    }
    let mut output = tempfile::NamedTempFile::new_in(&directory)?;
    output.write_all(text.as_bytes())?;
    output.as_file().sync_all()?;
    output.persist(target)?;
    println!("release checksums prepared for {} files", names.len());
    Ok(())
}

fn portable_name(name: &str) -> bool {
    if name.len() > 255
        || name.ends_with('.')
        || !name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
    {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_sorted_files_excludes_the_manifest_and_leaves_nested_inputs_untouched() {
        let root = tempfile::tempdir().unwrap();
        let artifacts = root.path().join("dist");
        std::fs::create_dir(&artifacts).unwrap();
        std::fs::write(artifacts.join("z.zip"), b"abc").unwrap();
        File::create(artifacts.join("a.tar.gz"))
            .unwrap()
            .set_len(3 * 1024 * 1024)
            .unwrap();
        std::fs::write(artifacts.join(MANIFEST), b"previous manifest").unwrap();
        std::fs::create_dir(artifacts.join("stage")).unwrap();
        std::fs::write(artifacts.join("stage/private.txt"), b"not a release asset").unwrap();
        write(root.path(), Path::new("dist")).unwrap();
        let first = std::fs::read(artifacts.join(MANIFEST)).unwrap();
        assert_eq!(
            String::from_utf8(first.clone()).unwrap(),
            concat!(
                "bbd05cf6097ac9b1f89ea29d2542c1b7b67ee46848393895f5a9e43fa1f621e5  a.tar.gz\n",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  z.zip\n"
            )
        );
        write(root.path(), Path::new("dist")).unwrap();
        assert_eq!(std::fs::read(artifacts.join(MANIFEST)).unwrap(), first);
        assert_eq!(
            std::fs::read(artifacts.join("stage/private.txt")).unwrap(),
            b"not a release asset"
        );
        assert_eq!(std::fs::read_dir(&artifacts).unwrap().count(), 4);
    }

    #[test]
    fn refuses_ambiguous_inputs_without_replacing_the_previous_manifest() {
        let root = tempfile::tempdir().unwrap();
        let artifacts = root.path().join("dist");
        std::fs::create_dir(&artifacts).unwrap();
        std::fs::write(artifacts.join("release.zip"), b"abc").unwrap();
        std::fs::write(artifacts.join(MANIFEST), b"previous manifest").unwrap();
        std::fs::write(artifacts.join("ambiguous name.zip"), b"refuse").unwrap();
        assert!(write(root.path(), Path::new("dist")).is_err());
        assert!(write(root.path(), Path::new("../dist")).is_err());
        assert_eq!(
            std::fs::read(artifacts.join(MANIFEST)).unwrap(),
            b"previous manifest"
        );
        assert_eq!(std::fs::read_dir(&artifacts).unwrap().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_redirected_inputs_before_publication() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let artifacts = root.path().join("dist");
        std::fs::create_dir(&artifacts).unwrap();
        std::fs::write(artifacts.join("release.zip"), b"abc").unwrap();
        std::fs::write(root.path().join("untouched"), b"sentinel").unwrap();
        symlink(root.path().join("untouched"), artifacts.join(MANIFEST)).unwrap();
        assert!(write(root.path(), Path::new("dist")).is_err());
        assert_eq!(
            std::fs::read(root.path().join("untouched")).unwrap(),
            b"sentinel"
        );
        std::fs::remove_file(artifacts.join(MANIFEST)).unwrap();
        symlink(root.path().join("untouched"), artifacts.join("link.zip")).unwrap();
        assert!(write(root.path(), Path::new("dist")).is_err());
        std::fs::remove_file(artifacts.join("link.zip")).unwrap();
        symlink(&artifacts, root.path().join("redirect")).unwrap();
        assert!(write(root.path(), Path::new("redirect")).is_err());
        assert!(!artifacts.join(MANIFEST).exists());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_case_collisions_on_a_case_sensitive_release_host() {
        let root = tempfile::tempdir().unwrap();
        let artifacts = root.path().join("dist");
        std::fs::create_dir(&artifacts).unwrap();
        std::fs::write(artifacts.join("release.zip"), b"first").unwrap();
        std::fs::write(artifacts.join("RELEASE.zip"), b"second").unwrap();
        assert!(write(root.path(), Path::new("dist")).is_err());
        assert!(!artifacts.join(MANIFEST).exists());
    }
}
