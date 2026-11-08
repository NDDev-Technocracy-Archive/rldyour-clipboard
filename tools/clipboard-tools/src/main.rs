use std::path::Path;

#[path = "../fingerprint.rs"]
mod fingerprint;

fn main() -> std::process::ExitCode {
    let result = run();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("clipboard-tools: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let command = std::env::args()
        .nth(1)
        .ok_or("expected check or generate")?;
    let root = std::env::current_dir()?.canonicalize()?;
    let manifest: toml::Value = toml::from_str(&std::fs::read_to_string(root.join("Cargo.toml"))?)?;
    let package = &manifest["workspace"]["package"];
    if package["repository"].as_str() != Some(env!("CARGO_PKG_REPOSITORY")) {
        return Err("run the tool from the NDDev Clipboard source root".into());
    }
    if fingerprint::fingerprint(&root)? != env!("CLIPBOARD_TOOLS_INPUTS") {
        return Err("tool inputs changed; prepare clipboard-tools from these sources".into());
    }
    match command.as_str() {
        "check" => generated(&root, false),
        "generate" => generated(&root, true),
        "install-tool" => {
            let directory = root.join(".tools/bin");
            std::fs::create_dir_all(&directory)?;
            let target = directory.join(format!("clipboard-tools{}", std::env::consts::EXE_SUFFIX));
            std::fs::copy(std::env::current_exe()?, target)?;
            Ok(())
        }
        _ => Err("expected check or generate".into()),
    }
}

fn generated(root: &Path, write: bool) -> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_string_pretty(&nddev_clipboard_api::config_schema())? + "\n";
    let path = root.join("contracts/config.schema.json");
    if write {
        std::fs::create_dir_all(path.parent().ok_or("schema has no parent")?)?;
        std::fs::write(&path, schema)?;
    } else if std::fs::read_to_string(&path)? != schema {
        return Err("configuration schema differs; run clipboard-tools generate".into());
    }
    println!(
        "configuration contract is current ({})",
        env!("CARGO_PKG_VERSION")
    );
    Ok(())
}
