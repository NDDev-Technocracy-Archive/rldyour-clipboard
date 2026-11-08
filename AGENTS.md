# NDDev Clipboard

This is an independent public product. The daemon owns capture, the SQLite index and
content-addressed archive. Integration consumers use the public contract and never open the
database or start a second collector. NDS identity, replication and telemetry belong in its
separate adapter. Keep private organization material and real clipboard data outside this
repository, logs, screenshots and artifacts.

The product version comes from the root Cargo workspace. API and SDK path requirements must
resolve to that same version; Cargo checks them. Generate schemas and client constants from
`crates/api` with `clipboard-tools`; do not maintain copied protocol tables. Preserve the v1
wire format, immutable published releases and existing archive/installation identities.

## Layout

- `crates/api`: typed configuration, wire protocol, content rules and desktop presentation types.
- `crates/sdk`: streaming client over a caller-owned connection; no archive access or collector.
- `daemon`: native capture, serving, private filesystem operations, store and maintenance.
- `tools/clipboard-tools`: source verifier and contract generator. Its binary checks its source
  fingerprint before operating. Use a verified runner artifact on a source-only machine.
- `ui`: shared history view and its isolated browser harness. The harness uses synthetic data;
  it is not a native tray application. Public design assets are immutable inputs with a digest.
- `extension`, `macos`, `python`: existing GNOME, AppKit and Python clients under migration.
- `daemon/systemd`, `daemon/launchd`, root installers and legacy CI: migration remains part of
  issue #14. Do not deploy them with the new configuration until all callers are qualified.

## Verification

Build-free checks run with `just check`. Builds, tests and applications run on execution hosts
or CI. `just build-tools` prepares the verifier there; `just generate` refreshes committed
contracts and UI projections. `just test` and `just test-browser` belong on execution hosts.
Keep the Windows occupied-path preservation regression enabled; its failure blocks release.
The temporary branch workflows do not qualify installation or replace the final delivery gates.

The native boundary ADR is under discussion. No new native exception is approved implicitly.
API, SDK and tooling forbid unsafe Rust. Keep legacy native code contained until the agreed
boundary is implemented and every unsafe operation has its native qualification.

## Data and lifecycle

- Never inspect or export a user's clipboard. Use a short private synthetic root, a complete
  configuration file and `capture_native = false` for protocol/process fixtures. Do not restart
  installed services or the desktop session to exercise development code.
- A daemon acquires the archive lease before publishing its endpoint. A second profile cannot
  open the same archive. Keep socket activation consistent with the selected configuration.
- Never unlink a live listener, ordinary file, directory, symlink or unrelated reparse point.
  Connection refusal alone does not classify an occupied Windows endpoint.
- Private directories must already be private or be created privately. Never chmod an existing
  shared directory. Archive/index/blob operations reject redirects and special files.
- A complete TOML profile owns paths, budget, retention and native capture. Missing or invalid
  fields fail before opening an archive. Existing archives require explicit policy at profile
  initialization; do not infer it from legacy environment variables.
- Favorites are the durable `pinned` flag. Expiry, budget eviction and clear preserve pins;
  explicit remove may delete one. Keyset paging follows the archive's ordering, not just IDs.
- Pin acknowledgement flushes content before its FULL-synchronous SQLite commit. POSIX also
  synchronizes relevant directory entries. Windows flushes writable file handles and WAL;
  do not claim a portable directory-fsync or a hardware power-loss guarantee.
- Control frames are bounded and payloads are streamed through the same buffered reader.
  An interrupted exchange must not reuse a connection with leftover payload bytes.
- Secret/transient hints are checked at capture and again at commit. Do not log content,
  previews, search terms, source file paths or native diagnostic payloads.

GNOME Wayland capture requires the compositor extension; X11 capture belongs to the daemon.
Mutter's memory selection source serves one MIME representation. Preserve the existing native
format and restore behavior when moving clients to the common window. File references and
copied image bytes currently have different retention semantics. Durable file content is the
selected product behavior; its storage encoding, migration and native acceptance remain in
issue #14. Existing reference-only records must not be presented as saved file bytes.
