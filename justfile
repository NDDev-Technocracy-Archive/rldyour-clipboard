set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Source checks only. Prepare clipboard-tools from a verified runner artifact.
check:
    cargo fmt --all --check
    cargo deny --locked --all-features check
    cargo shear --locked --deny-warnings
    .tools/bin/clipboard-tools check
    ./scripts/check-extension.sh
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
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    cargo nextest run --locked --workspace --all-features
    cargo test --locked --workspace --doc

test-browser:
    cd ui && bun run build:preview
    cd ui && bun run test
