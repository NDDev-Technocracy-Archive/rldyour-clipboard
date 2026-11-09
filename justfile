default:
    @just --list

setup:
    cargo fetch --locked --manifest-path daemon/Cargo.toml

# Check source and cross-language protocol metadata without starting capture.
check:
    just --fmt --check
    cargo fmt --check --manifest-path daemon/Cargo.toml
    shellcheck scripts/*.sh install.sh uninstall.sh install-macos.sh uninstall-macos.sh
    ./scripts/check-version.sh
    ./scripts/check-consistency.sh

# Native daemon gates. OS clients and isolated end-to-end archives retain their CI lanes.
test:
    cargo clippy --locked --all-targets --all-features --manifest-path daemon/Cargo.toml -- -D warnings
    cargo test --locked --all-features --manifest-path daemon/Cargo.toml
    cargo clippy --locked --all-targets --no-default-features --manifest-path daemon/Cargo.toml -- -D warnings
    cargo test --locked --no-default-features --manifest-path daemon/Cargo.toml

build:
    cargo build --release --locked --all-features --manifest-path daemon/Cargo.toml
    cargo build --release --locked --no-default-features --manifest-path daemon/Cargo.toml

verify: check test build
