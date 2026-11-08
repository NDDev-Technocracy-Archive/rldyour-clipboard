# Complete local configuration

The current development source uses one complete TOML profile. Clients resolve its endpoint
through the Rust reader, either in-process or with `rldyour-clipboardd --socket-json`. Discovery
does not open an archive, connect to a daemon or start capture. The schema is generated from
the Rust configuration type in [config.schema.json](../contracts/config.schema.json).

| Field | Meaning |
| --- | --- |
| `schema` | Configuration schema, currently `1` |
| `archive_dir` | Absolute private local directory for the existing archive format |
| `socket_path` | Absolute private local AF_UNIX endpoint |
| `budget_bytes` | Storage budget in bytes; `0` disables budget eviction |
| `retention_days` | Age policy for unpinned entries; `0` disables age expiry |
| `capture_native` | Whether this daemon starts its native watcher |

Every field is required. Unknown or duplicate fields, invalid values, non-regular profile files
and profiles larger than 16 KiB are rejected. Validation precedes archive and endpoint effects.
Filesystem operations enforce private placement and reject redirects. An existing shared
directory is refused without changing its permissions.

The default profile location is separate from the preserved archive location:

| Platform | Profile |
| --- | --- |
| Linux | `$XDG_CONFIG_HOME/nddev/clipboard/config.toml`, or `~/.config/nddev/clipboard/config.toml` |
| macOS | `~/Library/Application Support/nddev/clipboard/config.toml` |
| Windows | `%APPDATA%\nddev\clipboard\config.toml` |

Pass `--config` with an absolute path to select another profile. Initialize a fresh profile with
`--init-config`; initialization creates a private complete file without replacing an existing
different profile. The new-archive seed uses a 5 GiB budget, seven-day retention and native
capture enabled. Those values are initialization policy, not fallback values for missing fields.

For an existing archive, provide its budget, retention and capture policy explicitly with
`--budget-bytes`, `--retention-days` and `--capture true|false`, together with the selected
`--archive` and `--socket`. Initialization does not infer an existing archive's settings from
environment variables. Repeating the same initialization preserves the existing file.

`--check-config` validates without opening the archive. `--socket-json` returns only the selected
endpoint as a JSON string. Settings errors use generic diagnostics rather than echoing paths.

Before an installed upgrade, migrate the service manager, all clients and acceptance harnesses
together. The retained legacy installers are not yet qualified for this source profile. Existing
release archives keep their original configuration behavior; do not combine their installers
with the development daemon. Installation migration remains part of issue #14.
