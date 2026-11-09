set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Source checks only. Prepare clipboard-tools from a verified runner artifact.
check:
    cargo fmt --all --check
    cargo deny --locked --all-features check
    cargo shear --locked --deny-warnings
    .tools/bin/clipboard-tools check
    .tools/bin/clipboard-tools check-extension
    cd ui && bun run check

generate:
    .tools/bin/clipboard-tools generate
    cd ui && bun run generate

# Execution host or runner only.
build-tools:
    cargo build --locked -p clipboard-tools
    target/debug/clipboard-tools install-tool

# Execution host or runner only; the occupied-path regression stays enabled.
test:
    .tools/bin/clipboard-tools prepare-upgrade-fixture
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    cargo nextest run --locked --workspace --all-features
    cargo test --locked --workspace --doc
    .tools/bin/clipboard-tools qualify-native-client

test-browser:
    cd ui && bun run build:preview
    cd ui && bun run test

# Linux/macOS execution host only: builds this source and uses a private fixture.
test-platform-client:
    .tools/bin/clipboard-tools qualify-platform-client

# All supported native execution hosts: real CLI and daemon, synthetic profile.
test-native-client:
    .tools/bin/clipboard-tools qualify-native-client
