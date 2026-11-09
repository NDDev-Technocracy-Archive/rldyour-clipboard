//! Source-only checks for the retained GNOME client. Never load it into Shell.
use crate::qualification::OwnedProcess;
use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use thiserror::Error;

const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_SOURCE_FILES: usize = 256;

#[derive(Debug, Error)]
pub(super) enum Error {
    #[error("cannot read extension source: {0}")]
    Io(#[from] io::Error),
    #[error("invalid extension metadata: {0}")]
    Metadata(#[from] serde_json::Error),
    #[error("invalid source-check expression: {0}")]
    Expression(#[from] regex::Error),
    #[error("{0}")]
    Invalid(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Metadata {
    uuid: String,
    name: String,
    description: String,
    shell_version: Vec<String>,
    url: String,
    settings_schema: Option<String>,
}

#[derive(Debug)]
struct Source {
    path: PathBuf,
    text: String,
}

pub(super) fn check(root: &Path) -> Result<(), Error> {
    let extension = root.join("extension");
    let sources = validate(&extension)?;
    for source in &sources {
        command(
            Command::new("node")
                .arg("--check")
                .arg(extension.join(&source.path)),
        )?;
    }
    command(
        Command::new("glib-compile-schemas")
            .args(["--strict", "--dry-run"])
            .arg(extension.join("schemas")),
    )?;
    println!("GNOME extension sources, syntax and settings are valid");
    Ok(())
}

fn command(command: &mut Command) -> Result<(), Error> {
    // These validators parse public source; neither executes extension code.
    // Diagnostics stay on stderr rather than in an unbounded captured buffer.
    command.stdout(Stdio::null()).stderr(Stdio::inherit());
    OwnedProcess::start(command)?
        .finish(Instant::now() + Duration::from_secs(10))
        .map_err(|_| Error::Invalid("extension source validator failed or timed out".into()))
}

fn read(path: &Path) -> Result<String, Error> {
    if !std::fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Invalid(
            "extension source must be a regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(Error::Invalid(
            "extension source exceeds its size limit".into(),
        ));
    }
    String::from_utf8(bytes).map_err(|_| Error::Invalid("extension source must be UTF-8".into()))
}

fn collect(
    directory: &Path,
    root: &Path,
    files: &mut Vec<Source>,
    remaining: &mut usize,
) -> Result<(), Error> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        if *remaining == 0 {
            return Err(Error::Invalid("extension tree has too many entries".into()));
        }
        *remaining -= 1;
        entries.push(entry?);
    }
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
            return Err(Error::Invalid(
                "extension tree contains a redirected or special file".into(),
            ));
        }
        if kind.is_dir() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::Invalid("extension source escapes its root".into()))?;
            if relative.components().count() > 16 {
                return Err(Error::Invalid(
                    "extension tree exceeds its depth limit".into(),
                ));
            }
            collect(&path, root, files, remaining)?;
        } else if path.extension().is_some_and(|extension| extension == "js") {
            if files.len() >= MAX_SOURCE_FILES {
                return Err(Error::Invalid(
                    "extension has too many JavaScript sources".into(),
                ));
            }
            files.push(Source {
                path: path
                    .strip_prefix(root)
                    .map_err(|_| Error::Invalid("extension source escapes its root".into()))?
                    .to_owned(),
                text: read(&path)?,
            });
        }
    }
    Ok(())
}

fn validate(extension: &Path) -> Result<Vec<Source>, Error> {
    if !std::fs::symlink_metadata(extension)?.is_dir() {
        return Err(Error::Invalid(
            "extension must be a regular source directory".into(),
        ));
    }
    let mut sources = Vec::new();
    collect(extension, extension, &mut sources, &mut 1024)?;
    let metadata: Metadata = serde_json::from_str(&read(&extension.join("metadata.json"))?)?;
    if metadata.name.is_empty()
        || metadata.description.is_empty()
        || metadata.url.is_empty()
        || metadata.shell_version.is_empty()
        || !Regex::new(r"^[A-Za-z0-9._-]+@[A-Za-z0-9._-]+$")?.is_match(&metadata.uuid)
        || metadata.uuid.ends_with("gnome.org")
    {
        return Err(Error::Invalid(
            "extension metadata is missing a valid identity or required field".into(),
        ));
    }
    let schema_dir = extension.join("schemas");
    if !std::fs::symlink_metadata(&schema_dir)?.is_dir() {
        return Err(Error::Invalid(
            "extension settings must be a regular source directory".into(),
        ));
    }
    let mut schema_ids = BTreeSet::new();
    let mut declared = BTreeSet::new();
    let schema_pattern = Regex::new(r#"schema id="([^"]+)""#)?;
    let key_pattern = Regex::new(r#"<key name="([^"]+)""#)?;
    let mut count = 0;
    for entry in std::fs::read_dir(&schema_dir)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(".gschema.xml"))
        {
            count += 1;
            if count > MAX_SOURCE_FILES {
                return Err(Error::Invalid(
                    "extension has too many settings schemas".into(),
                ));
            }
            let text = read(&path)?;
            schema_ids.extend(
                schema_pattern
                    .captures_iter(&text)
                    .map(|capture| capture[1].to_owned()),
            );
            declared.extend(
                key_pattern
                    .captures_iter(&text)
                    .map(|capture| capture[1].to_owned()),
            );
        }
    }
    if let Some(schema) = metadata.settings_schema
        && !schema_ids.contains(&schema)
    {
        return Err(Error::Invalid(
            "extension declares an undefined settings schema".into(),
        ));
    }
    if !sources
        .iter()
        .any(|source| source.path == Path::new("extension.js"))
        || !sources
            .iter()
            .any(|source| source.path == Path::new("prefs.js"))
    {
        return Err(Error::Invalid(
            "extension entry point or preferences source is missing".into(),
        ));
    }
    let toolkit = Regex::new(r"gi://(Gtk|Adw|Gdk)")?;
    let shell = Regex::new(r"gi://(St|Clutter|Meta|Shell)")?;
    let deprecated = Regex::new(r"imports\.(mainloop|lang|byteArray)|from 'gi://ByteArray'")?;
    let blocking = Regex::new(r"\.(read_bytes|write_bytes|splice|read_line)\(")?;
    let registration = Regex::new(r"GObject\.registerClass\([ \t\r\n]*(\{)?")?;
    let settings = Regex::new(
        r"(?:get_(?:boolean|int|string|double|strv)|set_strv|settings\.bind)\('([^']+)'|(?m:^const SHORTCUT = '([^']+)')",
    )?;
    let mut used = BTreeSet::new();
    let mut style_used = BTreeSet::new();
    let style_pattern = Regex::new(r"styleClass: '([^']+)'|style_class_name\('([^']+)'\)")?;
    for source in &sources {
        let in_shell = source.path == Path::new("extension.js") || source.path.starts_with("lib");
        if (in_shell && toolkit.is_match(&source.text))
            || (source.path == Path::new("prefs.js") && shell.is_match(&source.text))
        {
            return invalid(
                source,
                "imports a library belonging to the other GNOME process",
            );
        }
        if deprecated.is_match(&source.text) {
            return invalid(source, "uses a deprecated GJS module");
        }
        if in_shell {
            if source.text.lines().any(|line| {
                blocking.is_match(line) && !line.contains("//") && !line.contains("_async")
            }) {
                return invalid(source, "uses a blocking stream operation in GNOME Shell");
            }
            if source.path != Path::new("lib/compat.js")
                && ["orientation:", "vertical: true", ".set_bytes("]
                    .iter()
                    .any(|pattern| source.text.contains(pattern))
            {
                return invalid(source, "uses a version-dependent API outside lib/compat.js");
            }
        }
        for capture in registration.captures_iter(&source.text) {
            let end = capture
                .get(0)
                .expect("a regex capture has the whole match")
                .end();
            if capture.get(1).is_none()
                || !source.text[end..]
                    .chars()
                    .take(500)
                    .collect::<String>()
                    .contains("GTypeName:")
            {
                return invalid(
                    source,
                    "registers a GObject class without an explicit GTypeName",
                );
            }
        }
        for capture in settings.captures_iter(&source.text) {
            if let Some(key) = capture.get(1).or_else(|| capture.get(2)) {
                used.insert(key.as_str().to_owned());
            }
        }
        for capture in style_pattern.captures_iter(&source.text) {
            if let Some(classes) = capture.get(1).or_else(|| capture.get(2)) {
                style_used.extend(
                    classes
                        .as_str()
                        .split_whitespace()
                        .filter(|name| name.starts_with("rldyour-"))
                        .map(str::to_owned),
                );
            }
        }
    }
    if let Some(missing) = used.difference(&declared).next() {
        return Err(Error::Invalid(format!(
            "extension reads undefined setting {missing}"
        )));
    }
    for unused in declared.difference(&used) {
        println!("note: extension setting {unused} has no static literal reader");
    }
    let css = read(&extension.join("stylesheet.css"))?;
    let defined: BTreeSet<_> = Regex::new(r"\.([a-z][a-z0-9-]*)")?
        .captures_iter(&css)
        .map(|capture| capture[1].to_owned())
        .collect();
    for missing in style_used.difference(&defined) {
        println!("note: extension style class {missing} has no rule");
    }
    Ok(sources)
}

fn invalid<T>(source: &Source, reason: &str) -> Result<T, Error> {
    Err(Error::Invalid(format!(
        "{} {reason}",
        source.path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("schemas")).unwrap();
        std::fs::create_dir(root.path().join("lib")).unwrap();
        std::fs::write(
            root.path().join("metadata.json"),
            include_str!("../../../extension/metadata.json"),
        )
        .unwrap();
        std::fs::write(root.path().join("schemas/settings.gschema.xml"), include_str!("../../../extension/schemas/org.gnome.shell.extensions.rldyour-clipboard.gschema.xml")).unwrap();
        std::fs::write(
            root.path().join("stylesheet.css"),
            include_str!("../../../extension/stylesheet.css"),
        )
        .unwrap();
        std::fs::write(
            root.path().join("extension.js"),
            include_str!("../../../extension/extension.js"),
        )
        .unwrap();
        std::fs::write(
            root.path().join("prefs.js"),
            include_str!("../../../extension/prefs.js"),
        )
        .unwrap();
        root
    }

    #[test]
    fn existing_metadata_settings_and_gnome_process_boundaries_are_preserved() {
        let fixture = fixture();
        validate(fixture.path()).unwrap();
        for (source, text, reason) in [
            (
                "extension.js",
                "import Gtk from 'gi://Gtk';",
                "other GNOME process",
            ),
            (
                "prefs.js",
                "import Shell from 'gi://Shell';",
                "other GNOME process",
            ),
            ("lib/unsafe.js", "stream.read_bytes(5);", "blocking stream"),
            (
                "lib/unsafe.js",
                "imports.mainloop.timeout_add(5, cb);",
                "deprecated GJS",
            ),
            (
                "lib/unsafe.js",
                "source.set_bytes(data);",
                "version-dependent API",
            ),
            (
                "lib/unsafe.js",
                "GObject.registerClass(class Indicator {});",
                "GTypeName",
            ),
            (
                "lib/unsafe.js",
                "GObject.registerClass({}, class Indicator {});",
                "GTypeName",
            ),
            (
                "lib/unsafe.js",
                "settings.get_boolean('not-declared');",
                "undefined setting",
            ),
        ] {
            let path = fixture.path().join(source);
            let previous = std::fs::read(&path).ok();
            std::fs::write(&path, text).unwrap();
            assert!(
                validate(fixture.path())
                    .unwrap_err()
                    .to_string()
                    .contains(reason),
                "{source}: {reason}"
            );
            if let Some(previous) = previous {
                std::fs::write(path, previous).unwrap();
            } else {
                std::fs::remove_file(path).unwrap();
            }
        }
        validate(fixture.path()).unwrap();
    }

    #[test]
    fn source_bounds_and_missing_declared_schema_fail_closed() {
        let fixture = fixture();
        let metadata = fixture.path().join("metadata.json");
        let text = std::fs::read_to_string(&metadata).unwrap();
        std::fs::write(
            &metadata,
            text.replace(
                "org.gnome.shell.extensions.rldyour-clipboard",
                "not-defined",
            ),
        )
        .unwrap();
        assert!(
            validate(fixture.path())
                .unwrap_err()
                .to_string()
                .contains("undefined settings schema")
        );
        std::fs::write(&metadata, text).unwrap();
        let file = std::fs::File::create(fixture.path().join("lib/over-limit.js")).unwrap();
        file.set_len(MAX_SOURCE_BYTES + 1).unwrap();
        assert!(
            validate(fixture.path())
                .unwrap_err()
                .to_string()
                .contains("size limit")
        );
    }
}
