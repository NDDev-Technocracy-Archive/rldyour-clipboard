//! One complete file selection, streamed through standard TAR framing.
//!
//! This codec owns no filesystem paths, clipboard, archive, threads or listener.
//! Receivers keep their output private until decoding succeeds. Names are opaque
//! components; a filesystem receiver must separately validate its target rules.
use nddev_clipboard_api::files::{
    FileContent, FileDigests, FileManifest, FileNode, MAX_METADATA_BYTES, SHA256_HEX_BYTES,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};

const BLOCK: u64 = 512;
const COPY_BUFFER: usize = 64 * 1024;
const MANIFEST: &str = "manifest.json";
const DIGESTS: &str = "digests.json";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Clipboard file manifest is invalid or too large")]
    Manifest,
    #[error("Clipboard file container is invalid")]
    Container,
    #[error("Clipboard file content is incomplete or has changed")]
    Integrity,
    #[error("Clipboard file I/O failed ({0:?})")]
    Io(io::ErrorKind),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// A validated manifest and the exact encoded length, without reading file bodies.
pub struct PreparedSelection {
    manifest: FileManifest,
    json: Vec<u8>,
    bytes: u64,
}

impl PreparedSelection {
    pub fn new(manifest: FileManifest) -> Result<Self, Error> {
        manifest.validate().map_err(|_| Error::Manifest)?;
        let json = encode_metadata(&manifest)?;
        let files = manifest
            .nodes
            .iter()
            .filter(|node| matches!(node.content, FileContent::File { .. }))
            .count();
        let empty =
            serde_json::to_vec(&FileDigests { sha256: Vec::new() }).map_err(|_| Error::Manifest)?;
        let footer = empty.len() + files * (SHA256_HEX_BYTES + 3) - usize::from(files > 0);
        let bytes = container_bytes(&manifest, json.len() as u64, footer as u64)?;
        if bytes > i64::MAX as u64 {
            return Err(Error::Manifest);
        }
        Ok(Self {
            manifest,
            json,
            bytes,
        })
    }

    pub fn manifest(&self) -> &FileManifest {
        &self.manifest
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Open each file only when it is streamed; the index is its manifest position.
    /// The source owns cancellation, non-following opens and source-change checks.
    pub fn write_to<W: Write, R: Read>(
        &self,
        output: W,
        mut open: impl FnMut(usize) -> io::Result<R>,
    ) -> Result<(), Error> {
        let mut output = Counted::new(output);
        {
            let mut archive = tar::Builder::new(&mut output);
            append(
                &mut archive,
                MANIFEST,
                self.json.len() as u64,
                self.json.as_slice(),
            )?;
            let mut digests = FileDigests { sha256: Vec::new() };
            for (index, node) in self.manifest.nodes.iter().enumerate() {
                let FileContent::File { bytes } = node.content else {
                    continue;
                };
                let mut source = Hashed::new(open(index)?);
                append(&mut archive, &object_name(index), bytes, &mut source)?;
                if source.count != bytes || !is_eof(&mut source.inner)? {
                    return Err(Error::Integrity);
                }
                digests.sha256.push(digest_hex(source.hasher));
            }
            let footer = encode_metadata(&digests)?;
            append(
                &mut archive,
                DIGESTS,
                footer.len() as u64,
                footer.as_slice(),
            )?;
            archive.finish()?;
        }
        if output.count != self.bytes {
            return Err(Error::Container);
        }
        Ok(())
    }
}

/// Decode one representation, whose reader must end at its declared boundary.
/// The callback must consume each file fully and keep outputs private until success.
/// Directories receive an empty reader. No archive member is unpacked to a path.
pub fn decode<R: Read>(
    input: R,
    mut receive: impl FnMut(&FileNode, &mut dyn Read) -> io::Result<()>,
) -> Result<FileManifest, Error> {
    let mut archive = tar::Archive::new(Counted::new(input));
    let mut entries = archive.entries()?.raw(true);
    let (manifest, manifest_bytes): (FileManifest, _) = metadata(&mut entries, MANIFEST)?;
    manifest.validate().map_err(|_| Error::Manifest)?;
    let mut actual = FileDigests { sha256: Vec::new() };
    for (index, node) in manifest.nodes.iter().enumerate() {
        match node.content {
            FileContent::Directory => receive(node, &mut io::empty())?,
            FileContent::File { bytes } => {
                let entry = member(&mut entries, &object_name(index))?;
                if entry.size() != bytes {
                    return Err(Error::Container);
                }
                let mut content = Hashed::new(entry);
                receive(node, &mut content)?;
                if content.count != bytes {
                    return Err(Error::Integrity);
                }
                actual.sha256.push(digest_hex(content.hasher));
            }
        }
    }
    let (expected, digest_bytes): (FileDigests, _) = metadata(&mut entries, DIGESTS)?;
    if expected.sha256 != actual.sha256 {
        return Err(Error::Integrity);
    }
    drop(entries);
    let mut input = archive.into_inner();
    let bytes = container_bytes(&manifest, manifest_bytes, digest_bytes)?;
    let remaining = bytes.checked_sub(input.count).ok_or(Error::Container)?;
    if !(2 * BLOCK..3 * BLOCK).contains(&remaining) {
        return Err(Error::Container);
    }
    let mut tail = [0; 3 * BLOCK as usize];
    input.read_exact(&mut tail[..remaining as usize])?;
    if tail[..remaining as usize].iter().any(|byte| *byte != 0) || !is_eof(&mut input)? {
        return Err(Error::Container);
    }
    Ok(manifest)
}

fn member<'a, R: Read>(
    entries: &mut tar::Entries<'a, R>,
    name: &str,
) -> Result<tar::Entry<'a, R>, Error> {
    let entry = entries.next().ok_or(Error::Container)??;
    if entry.header().entry_type() != tar::EntryType::Regular
        || entry.path_bytes().as_ref() != name.as_bytes()
        || entry.link_name_bytes().is_some()
    {
        return Err(Error::Container);
    }
    Ok(entry)
}

fn metadata<R: Read, T: DeserializeOwned>(
    entries: &mut tar::Entries<'_, R>,
    name: &str,
) -> Result<(T, u64), Error> {
    let mut entry = member(entries, name)?;
    let bytes = entry.size();
    if bytes > MAX_METADATA_BYTES as u64 {
        return Err(Error::Manifest);
    }
    let mut json = Vec::with_capacity(bytes as usize);
    entry.read_to_end(&mut json)?;
    let value = serde_json::from_slice(&json).map_err(|_| Error::Container)?;
    Ok((value, bytes))
}

fn append<W: Write>(
    archive: &mut tar::Builder<W>,
    name: &str,
    bytes: u64,
    input: impl Read,
) -> io::Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    archive.append_data(&mut header, name, input)
}

fn object_name(index: usize) -> String {
    format!("objects/{index:08x}")
}

fn digest_hex(hasher: Sha256) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = hasher.finalize();
    let mut output = String::with_capacity(SHA256_HEX_BYTES);
    for byte in digest.as_slice() {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 15)] as char);
    }
    output
}

fn encode_metadata(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > MAX_METADATA_BYTES - self.0.len() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut output = Bounded(Vec::new());
    serde_json::to_writer(&mut output, value).map_err(|_| Error::Manifest)?;
    Ok(output.0)
}

fn member_bytes(bytes: u64) -> Result<u64, Error> {
    bytes
        .checked_add(BLOCK - 1)
        .map(|value| value / BLOCK)
        .and_then(|blocks| blocks.checked_add(1))
        .and_then(|blocks| blocks.checked_mul(BLOCK))
        .ok_or(Error::Manifest)
}

fn container_bytes(manifest: &FileManifest, json: u64, digests: u64) -> Result<u64, Error> {
    let mut bytes = member_bytes(json)?;
    for node in &manifest.nodes {
        if let FileContent::File { bytes: size } = node.content {
            bytes = bytes
                .checked_add(member_bytes(size)?)
                .ok_or(Error::Manifest)?;
        }
    }
    bytes
        .checked_add(member_bytes(digests)?)
        .and_then(|size| size.checked_add(2 * BLOCK))
        .ok_or(Error::Manifest)
}

fn is_eof(input: &mut impl Read) -> io::Result<bool> {
    loop {
        match input.read(&mut [0]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            value => return value.map(|count| count == 0),
        }
    }
}

struct Hashed<R> {
    inner: R,
    hasher: Sha256,
    count: u64,
}
impl<R> Hashed<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            count: 0,
        }
    }
}
impl<R: Read> Read for Hashed<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let limit = buffer.len().min(COPY_BUFFER);
        let count = self.inner.read(&mut buffer[..limit])?;
        self.hasher.update(&buffer[..count]);
        self.count += count as u64;
        Ok(count)
    }
}

struct Counted<T> {
    inner: T,
    count: u64,
}
impl<T> Counted<T> {
    fn new(inner: T) -> Self {
        Self { inner, count: 0 }
    }
}
impl<T: Read> Read for Counted<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        self.count += count as u64;
        Ok(count)
    }
}
impl<T: Write> Write for Counted<T> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(buffer)?;
        self.count += count as u64;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
