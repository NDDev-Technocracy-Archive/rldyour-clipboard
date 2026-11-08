//! Persistent file-selection contract. Names are content, never diagnostics.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MIME: &str = "application/vnd.nddev.clipboard.files.v1+tar";
pub const VERSION: u32 = 1;
pub const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_NODES: usize = 16_384;
pub const MAX_DEPTH: usize = 64;
pub const MAX_COMPONENT_BYTES: usize = 1024;
pub const SHA256_HEX_BYTES: usize = 64;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileManifest {
    pub version: u32,
    pub nodes: Vec<FileNode>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileNode {
    pub path: Vec<String>,
    pub content: FileContent,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileContent {
    Directory,
    File { bytes: u64 },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileDigests {
    pub sha256: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("Clipboard file manifest is invalid or exceeds its format limits")]
pub struct ManifestError;

impl FileManifest {
    pub fn new(nodes: Vec<FileNode>) -> Result<Self, ManifestError> {
        let manifest = Self {
            version: VERSION,
            nodes,
        };
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate once at the external boundary, before file or archive effects.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != VERSION || self.nodes.is_empty() || self.nodes.len() > MAX_NODES {
            return Err(ManifestError);
        }
        let mut parents = BTreeMap::new();
        let mut previous: Option<&[String]> = None;
        let mut metadata_bytes = 0usize;
        let mut payload_bytes = 0u64;
        for node in &self.nodes {
            if node.path.is_empty()
                || node.path.len() > MAX_DEPTH
                || previous.is_some_and(|path| path >= node.path.as_slice())
            {
                return Err(ManifestError);
            }
            for name in &node.path {
                if name.is_empty()
                    || name == "."
                    || name == ".."
                    || name.len() > MAX_COMPONENT_BYTES
                    || name.contains(['/', '\0'])
                {
                    return Err(ManifestError);
                }
                metadata_bytes = metadata_bytes
                    .checked_add(name.len())
                    .ok_or(ManifestError)?;
            }
            if metadata_bytes > MAX_METADATA_BYTES
                || (node.path.len() > 1
                    && parents.get(&node.path[..node.path.len() - 1]) != Some(&true))
            {
                return Err(ManifestError);
            }
            if let FileContent::File { bytes } = node.content {
                payload_bytes = payload_bytes.checked_add(bytes).ok_or(ManifestError)?;
                if payload_bytes > i64::MAX as u64 {
                    return Err(ManifestError);
                }
            }
            parents.insert(
                node.path.clone(),
                matches!(node.content, FileContent::Directory),
            );
            previous = Some(&node.path);
        }
        Ok(())
    }
}
