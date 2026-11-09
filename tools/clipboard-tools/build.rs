#[path = "src/fingerprint.rs"]
mod fingerprint;

fn main() {
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo supplies the manifest");
    let root = std::path::PathBuf::from(manifest).join("../..");
    let fingerprint = fingerprint::fingerprint(&root, None).expect("read the tool's source inputs");
    println!("cargo:rustc-env=CLIPBOARD_TOOLS_INPUTS={fingerprint}");
    let target = std::env::var("TARGET").expect("Cargo supplies the build target");
    println!("cargo:rustc-env=CLIPBOARD_TOOLS_TARGET={target}");
    for name in fingerprint::INPUTS {
        println!("cargo:rerun-if-changed=../../{name}");
    }
}
