//! Real file bytes and malformed containers; no OS clipboard or installed archive.
use nddev_clipboard_api::files::{
    FileContent, FileManifest, FileNode, MAX_METADATA_BYTES, VERSION,
};
use nddev_clipboard_files::{Error, PreparedSelection, decode};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};

fn node(path: &[&str], content: FileContent) -> FileNode {
    FileNode {
        path: path.iter().map(|name| (*name).to_owned()).collect(),
        content,
    }
}

#[test]
fn a_selection_restores_real_unicode_files_after_the_source_is_deleted() {
    let bytes = 3 * 1024 * 1024 + 17;
    let mut source = tempfile::NamedTempFile::new().unwrap();
    let pattern = [0xa7; 8192];
    let mut left = bytes;
    while left > 0 {
        let count = left.min(pattern.len() as u64) as usize;
        source.write_all(&pattern[..count]).unwrap();
        left -= count as u64;
    }
    source.as_file_mut().rewind().unwrap();
    let expected_digest = hash(source.as_file_mut());
    let manifest = FileManifest::new(vec![
        node(&["empty.txt"], FileContent::File { bytes: 0 }),
        node(&["folder"], FileContent::Directory),
        node(&["folder", "large.bin"], FileContent::File { bytes }),
        node(&["folder", "sub"], FileContent::Directory),
        node(&["צילום.txt"], FileContent::File { bytes: 4 }),
        node(&["照片.png"], FileContent::File { bytes: 3 }),
    ])
    .unwrap();
    let prepared = PreparedSelection::new(manifest.clone()).unwrap();
    let mut container = tempfile::NamedTempFile::new().unwrap();
    prepared
        .write_to(
            container.as_file_mut(),
            |index| -> io::Result<Box<dyn Read>> {
                Ok(match index {
                    0 => Box::new(io::empty()),
                    2 => Box::new(File::open(source.path())?),
                    4 => Box::new(io::Cursor::new(b"text")),
                    5 => Box::new(io::Cursor::new([0, 0xff, 3])),
                    _ => panic!("only files are opened"),
                })
            },
        )
        .unwrap();
    assert_eq!(
        container.as_file().metadata().unwrap().len(),
        prepared.bytes()
    );
    #[cfg(unix)]
    {
        let listing = std::process::Command::new("tar")
            .arg("-tf")
            .arg(container.path())
            .output()
            .unwrap();
        assert!(listing.status.success());
        let names = String::from_utf8(listing.stdout).unwrap();
        assert_eq!(names.lines().next(), Some("manifest.json"));
        assert_eq!(names.lines().last(), Some("digests.json"));
        assert_eq!(names.lines().count(), 6);
    }
    let source_path = source.path().to_owned();
    source.close().unwrap();
    assert!(!source_path.exists());
    let destination = tempfile::tempdir().unwrap();
    container.as_file_mut().rewind().unwrap();
    let restored = decode(container.as_file_mut(), |node, input| {
        let path = node
            .path
            .iter()
            .fold(destination.path().to_owned(), |path, name| path.join(name));
        match node.content {
            FileContent::Directory => fs::create_dir(path),
            FileContent::File { .. } => {
                let mut file = File::options().write(true).create_new(true).open(path)?;
                io::copy(input, &mut file)?;
                Ok(())
            }
        }
    })
    .unwrap();
    assert!(restored == manifest);
    assert_eq!(
        hash(&mut File::open(destination.path().join("folder/large.bin")).unwrap()),
        expected_digest
    );
    assert_eq!(
        fs::metadata(destination.path().join("empty.txt"))
            .unwrap()
            .len(),
        0
    );
    assert!(destination.path().join("folder/sub").is_dir());
    assert_eq!(
        fs::read(destination.path().join("צילום.txt")).unwrap(),
        b"text"
    );
    assert_eq!(
        fs::read(destination.path().join("照片.png")).unwrap(),
        [0, 0xff, 3]
    );
}

fn hash(input: &mut impl Read) -> Vec<u8> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = input.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    hash.finalize().to_vec()
}

fn small() -> (PreparedSelection, Vec<u8>) {
    let selection = PreparedSelection::new(
        FileManifest::new(vec![node(&["file"], FileContent::File { bytes: 6 })]).unwrap(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    selection
        .write_to(&mut bytes, |_| Ok(io::Cursor::new(b"secret")))
        .unwrap();
    (selection, bytes)
}

fn consume(_: &FileNode, input: &mut dyn Read) -> io::Result<()> {
    io::copy(input, &mut io::sink()).map(|_| ())
}

#[test]
fn an_empty_directory_is_a_complete_deterministic_selection() {
    let manifest = FileManifest::new(vec![node(&["empty"], FileContent::Directory)]).unwrap();
    let selection = PreparedSelection::new(manifest.clone()).unwrap();
    let mut first = Vec::new();
    let mut second = Vec::new();
    let no_files = |_| -> io::Result<io::Empty> { panic!("directories have no source") };
    selection.write_to(&mut first, no_files).unwrap();
    selection.write_to(&mut second, no_files).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len() as u64, selection.bytes());
    assert!(decode(first.as_slice(), consume).unwrap() == manifest);
}

#[test]
fn content_digest_detects_a_change_that_tar_header_checksums_cannot() {
    let (_, mut bytes) = small();
    let start = bytes
        .windows(6)
        .position(|bytes| bytes == b"secret")
        .unwrap();
    bytes[start] ^= 1;
    assert!(matches!(
        decode(bytes.as_slice(), consume),
        Err(Error::Integrity)
    ));
}

#[test]
fn incomplete_sources_receivers_and_containers_cannot_report_success() {
    let (selection, bytes) = small();
    assert!(
        selection
            .write_to(io::sink(), |_| Ok(io::Cursor::new(b"short")))
            .is_err()
    );
    assert!(
        selection
            .write_to(io::sink(), |_| Ok(io::Cursor::new(b"too long")))
            .is_err()
    );
    assert!(matches!(
        decode(bytes.as_slice(), |_, _| Ok(())),
        Err(Error::Integrity)
    ));
    for length in [0, 511, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode(&bytes[..length], consume).is_err());
    }
    let mut extra = bytes.clone();
    extra.extend_from_slice(b"unexpected trailing member");
    assert!(decode(extra.as_slice(), consume).is_err());
}

#[test]
fn manifest_names_and_topology_are_checked_before_source_io() {
    for nodes in [
        vec![node(&["..", "escape"], FileContent::File { bytes: 1 })],
        vec![node(&["/absolute"], FileContent::File { bytes: 1 })],
        vec![node(&["orphan", "child"], FileContent::File { bytes: 1 })],
        vec![
            node(&["same"], FileContent::File { bytes: 1 }),
            node(&["same"], FileContent::Directory),
        ],
        vec![
            node(&["parent"], FileContent::File { bytes: 1 }),
            node(&["parent", "child"], FileContent::File { bytes: 1 }),
        ],
        vec![
            node(
                &["name"],
                FileContent::File {
                    bytes: i64::MAX as u64,
                },
            ),
            node(&["next"], FileContent::File { bytes: 1 }),
        ],
    ] {
        assert!(
            PreparedSelection::new(FileManifest {
                version: VERSION,
                nodes
            })
            .is_err()
        );
    }
}

#[test]
fn links_extensions_and_oversized_metadata_are_refused_before_the_receiver() {
    let (selection, _) = small();
    for kind in [
        tar::EntryType::Symlink,
        tar::EntryType::Link,
        tar::EntryType::GNULongName,
    ] {
        let mut archive = tar::Builder::new(Vec::new());
        let manifest = serde_json::to_vec(selection.manifest()).unwrap();
        append(
            &mut archive,
            "manifest.json",
            tar::EntryType::Regular,
            &manifest,
        );
        append(&mut archive, "objects/00000000", kind, b"secret");
        let bytes = archive.into_inner().unwrap();
        assert!(
            decode(bytes.as_slice(), |_, _| panic!(
                "invalid member reached the receiver"
            ))
            .is_err()
        );
    }
    let mut header = tar::Header::new_ustar();
    header.set_path("manifest.json").unwrap();
    header.set_size(MAX_METADATA_BYTES as u64 + 1);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    assert!(matches!(
        decode(header.as_bytes().as_slice(), consume),
        Err(Error::Manifest)
    ));
}

fn append(archive: &mut tar::Builder<Vec<u8>>, path: &str, kind: tar::EntryType, content: &[u8]) {
    let mut header = tar::Header::new_ustar();
    header.set_size(content.len() as u64);
    header.set_entry_type(kind);
    header.set_mode(0o600);
    if kind.is_symlink() || kind.is_hard_link() {
        header.set_link_name("unrelated").unwrap();
    }
    archive.append_data(&mut header, path, content).unwrap();
}
