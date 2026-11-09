//! Check the workspace's one product version before packaging.
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub(super) enum Error {
    #[error("cannot read product version input: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid product version manifest: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("{0}")]
    Invalid(&'static str),
}

pub(super) fn check(root: &Path, manifest: &toml::Value, tag: Option<&str>) -> Result<(), Error> {
    let version = env!("CARGO_PKG_VERSION");
    if tag.is_some_and(|tag| tag != format!("v{version}")) {
        return Err(Error::Invalid(
            "release tag differs from the product version",
        ));
    }
    let workspace = manifest
        .get("workspace")
        .ok_or(Error::Invalid("workspace is missing"))?;
    if workspace
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        != Some(version)
    {
        return Err(Error::Invalid(
            "workspace version differs from the checked tool",
        ));
    }
    let lock: toml::Value = toml::from_str(&read(&root.join("Cargo.lock"))?)?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or(Error::Invalid("Cargo lock packages are missing"))?;
    let members = workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .ok_or(Error::Invalid("workspace members are missing"))?;
    let mut names = BTreeSet::new();
    for member in members {
        let member = member
            .as_str()
            .ok_or(Error::Invalid("workspace member must be a literal path"))?;
        let relative = Path::new(member);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(Error::Invalid(
                "workspace member escapes the product source",
            ));
        }
        let directory = root.join(relative).canonicalize()?;
        if !directory.starts_with(root) {
            return Err(Error::Invalid(
                "workspace member escapes the product source",
            ));
        }
        let member: toml::Value = toml::from_str(&read(&directory.join("Cargo.toml"))?)?;
        let package = member
            .get("package")
            .ok_or(Error::Invalid("workspace member package is missing"))?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .ok_or(Error::Invalid("workspace package name is missing"))?;
        if !names.insert(name.to_owned()) {
            return Err(Error::Invalid("workspace has duplicate package names"));
        }
        let declared = package
            .get("version")
            .ok_or(Error::Invalid("workspace member version is missing"))?;
        if declared.as_str() != Some(version)
            && declared.get("workspace").and_then(toml::Value::as_bool) != Some(true)
        {
            return Err(Error::Invalid(
                "workspace member has a different product version",
            ));
        }
        let selected: Vec<_> = packages
            .iter()
            .filter(|package| {
                package.get("name").and_then(toml::Value::as_str) == Some(name)
                    && package.get("source").is_none()
            })
            .collect();
        let [locked] = selected.as_slice() else {
            return Err(Error::Invalid(
                "Cargo lock must identify each workspace package once",
            ));
        };
        if locked.get("version").and_then(toml::Value::as_str) != Some(version) {
            return Err(Error::Invalid("Cargo lock has a different product version"));
        }
    }
    let changelog = read(&root.join("CHANGELOG.md"))?;
    let prefix = format!("## {version} — ");
    let mut headings = changelog
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix));
    let release = headings
        .next()
        .ok_or(Error::Invalid("changelog is missing this product version"))?;
    if headings.next().is_some() || release.is_empty() {
        return Err(Error::Invalid("changelog has an ambiguous product version"));
    }
    if tag.is_some() && release.to_ascii_lowercase().contains("unreleased") {
        return Err(Error::Invalid(
            "an unreleased product cannot be published by tag",
        ));
    }
    println!("workspace, Cargo lock and changelog versions agree ({version})");
    Ok(())
}

fn read(path: &Path) -> Result<String, Error> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Invalid(
            "product version input must be a regular file",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::Invalid(
            "product version input exceeds its size limit",
        ));
    }
    String::from_utf8(bytes).map_err(|_| Error::Invalid("product version input must be UTF-8"))
}
