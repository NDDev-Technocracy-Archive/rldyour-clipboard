//! Retain upstream Cargo license texts with source-tool artifacts.
use crate::qualification::OwnedProcess;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub(super) enum Error {
    #[error("cannot prepare source-tool notices: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid source-tool notice metadata: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(&'static str),
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolution,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    license: Option<String>,
    license_file: Option<PathBuf>,
    manifest_path: PathBuf,
}

#[derive(Deserialize)]
struct Resolution {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Dependency {
    pkg: String,
    dep_kinds: Vec<DependencyKind>,
}

#[derive(Deserialize)]
struct DependencyKind {
    kind: Option<String>,
}

#[derive(Serialize)]
struct File {
    path: String,
    sha256: String,
}

#[derive(Serialize)]
struct Notice {
    name: String,
    version: String,
    license_expression: String,
    source: String,
    files: Vec<File>,
}

pub(super) fn bundle(root: &Path) -> Result<(), Error> {
    let mut output = tempfile::tempfile()?;
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            env!("CLIPBOARD_TOOLS_TARGET"),
        ])
        .stdout(output.try_clone()?)
        .stderr(Stdio::inherit());
    OwnedProcess::start(&mut command)?
        .finish(Instant::now() + Duration::from_secs(30))
        .map_err(|_| Error::Invalid("Cargo notice metadata failed or exceeded its deadline"))?;
    if output.metadata()?.len() > 16 * 1024 * 1024 {
        return Err(Error::Invalid(
            "Cargo notice metadata exceeded its size limit",
        ));
    }
    output.rewind()?;
    let metadata: Metadata = serde_json::from_reader(output)?;
    let packages: BTreeMap<_, _> = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect();
    let nodes: BTreeMap<_, _> = metadata
        .resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    if packages.len() != metadata.packages.len() || nodes.len() != metadata.resolve.nodes.len() {
        return Err(Error::Invalid(
            "Cargo notice metadata has duplicate identities",
        ));
    }
    let selected: Vec<_> = metadata
        .packages
        .iter()
        .filter(|package| package.name == env!("CARGO_PKG_NAME") && package.source.is_none())
        .collect();
    let [tool] = selected.as_slice() else {
        return Err(Error::Invalid(
            "Cargo notice metadata must identify exactly this source tool",
        ));
    };
    let mut pending = vec![tool.id.as_str()];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let node = nodes.get(id).ok_or(Error::Invalid(
            "Cargo notice metadata has an unresolved node",
        ))?;
        for dependency in &node.deps {
            if dependency.dep_kinds.is_empty()
                || dependency
                    .dep_kinds
                    .iter()
                    .any(|kind| !matches!(kind.kind.as_deref(), None | Some("build" | "dev")))
            {
                return Err(Error::Invalid(
                    "Cargo notice metadata has an unknown dependency kind",
                ));
            }
            if dependency
                .dep_kinds
                .iter()
                .any(|kind| kind.kind.as_deref() != Some("dev"))
            {
                pending.push(dependency.pkg.as_str());
            }
        }
    }
    let destination = root.join(".build/source-tool");
    for directory in [root.join(".build"), destination.clone()] {
        if !std::fs::symlink_metadata(directory)?.is_dir() {
            return Err(Error::Invalid(
                "source-tool artifact directory is redirected or missing",
            ));
        }
    }
    // A fresh, owned tree: never merge with stale or redirected notice files.
    let licenses = destination.join("dependency-licenses");
    std::fs::create_dir(&licenses)?;
    for name in ["LICENSE", "NOTICE", "THIRD_PARTY_NOTICES.md"] {
        let bytes = read(&root.join(name))?;
        let mut file = std::fs::File::create_new(destination.join(name))?;
        std::io::Write::write_all(&mut file, &bytes)?;
    }
    let mut notices = Vec::new();
    for id in seen {
        let package = packages.get(id).ok_or(Error::Invalid(
            "Cargo notice metadata has an unresolved package",
        ))?;
        let directory = package
            .manifest_path
            .parent()
            .ok_or(Error::Invalid("Cargo package has no source directory"))?
            .canonicalize()?;
        let license = package
            .license
            .as_deref()
            .ok_or(Error::Invalid("Cargo package has no license expression"))?;
        if package.source.is_none() {
            if !directory.starts_with(root) || license != "Apache-2.0" {
                return Err(Error::Invalid(
                    "source-tool local dependency is not an owned Apache package",
                ));
            }
            continue;
        }
        if package.source.as_deref()
            != Some("registry+https://github.com/rust-lang/crates.io-index")
        {
            return Err(Error::Invalid(
                "source-tool dependency has an unqualified source",
            ));
        }
        if !safe_name(&package.name) || !safe_name(&package.version) {
            return Err(Error::Invalid(
                "Cargo package identity is not a safe artifact path",
            ));
        }
        let mut files = BTreeSet::new();
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let upper = entry.file_name().to_string_lossy().to_uppercase();
            if [
                "LICENSE",
                "LICENCE",
                "COPYING",
                "NOTICE",
                "COPYRIGHT",
                "AUTHORS",
            ]
            .iter()
            .any(|prefix| upper.starts_with(prefix))
                && entry.file_type()?.is_file()
            {
                files.insert(entry.path());
            }
        }
        if let Some(path) = &package.license_file {
            files.insert(path.clone());
        }
        if files.is_empty() || files.len() > 32 {
            return Err(Error::Invalid(
                "Cargo package license texts are missing or exceed their count limit",
            ));
        }
        let prefix = format!("{}-{}", package.name, package.version);
        std::fs::create_dir(licenses.join(&prefix))?;
        let mut texts = Vec::new();
        for path in files {
            if !path.canonicalize()?.starts_with(&directory) {
                return Err(Error::Invalid("Cargo license file escapes its package"));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| safe_name(name))
                .ok_or(Error::Invalid(
                    "Cargo license text has an invalid file name",
                ))?;
            let bytes = read(&path)?;
            let relative = format!("dependency-licenses/{prefix}/{name}");
            let target = destination.join(&relative);
            let mut file = std::fs::File::create_new(target)?;
            std::io::Write::write_all(&mut file, &bytes)?;
            texts.push(File {
                path: relative,
                sha256: crate::fingerprint::hex(&Sha256::digest(&bytes)),
            });
        }
        notices.push(Notice {
            name: package.name.clone(),
            version: package.version.clone(),
            license_expression: license.to_owned(),
            source: format!(
                "https://crates.io/crates/{}/{}",
                package.name, package.version
            ),
            files: texts,
        });
    }
    notices.sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));
    let manifest = serde_json::json!({
        "schema_version": 1, "package": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION"),
        "target": env!("CLIPBOARD_TOOLS_TARGET"), "packages": notices,
    });
    let mut file = std::fs::File::create_new(destination.join("cargo-dependency-notices.json"))?;
    std::io::Write::write_all(
        &mut file,
        (serde_json::to_string_pretty(&manifest)? + "\n").as_bytes(),
    )?;
    println!("retained source-tool Cargo dependency license texts");
    Ok(())
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+".contains(&byte))
}

fn read(path: &Path) -> Result<Vec<u8>, Error> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Invalid("license text must be a regular file"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err(Error::Invalid(
            "license text is empty or exceeds its size limit",
        ));
    }
    Ok(bytes)
}
