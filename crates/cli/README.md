# Native client

`nddev-clipboard` is a standalone command for the product's local v1 API on
Windows, macOS ARM/Intel and Linux. The Rust SDK owns its connection. The daemon
owns the archive and capture; this command starts neither a daemon nor a collector.
This client belongs to the unreleased product migration, not the existing release assets.

Build it on an execution host with `cargo build --locked -p nddev-clipboard`.
Use the same complete [TOML profile](../../docs/configuration.md) as the daemon:

```sh
nddev-clipboard --config /absolute/clipboard.toml list --favorites
nddev-clipboard --config /absolute/clipboard.toml list --query phrase --kind text
nddev-clipboard --config /absolute/clipboard.toml pin 42
nddev-clipboard --config /absolute/clipboard.toml get 42 --mime image/png > restored.png
nddev-clipboard --config /absolute/clipboard.toml stats
nddev-clipboard --config /absolute/clipboard.toml watch --seconds 30
```

Control output is escaped JSON, one value per line. A list returns the shared
desktop page, including the exact decimal `key` for every 64-bit archive ID.
Pass that key back as the command's ID. `more` and `--before` retain the provider's
keyset ordering and frame-size limit. `--favorites` selects pinned entries;
`--unpinned` selects the others. No client-side archive scan implements filtering.

`get` streams the selected original representation to standard output; redirect
binary images or files to a receiving file or process. It does not change the OS
clipboard. `pin`, `unpin`, `remove` and `clear` return the shared changed result.
Clear preserves favorites; explicit remove can delete a favorite. Statistics
report the archive counters and policy from the provider.

Normal commands use a finite provider deadline, configurable with the global
`--timeout-ms N` option. Watch instead requires positive `--seconds N` and cannot
combine the two deadline options. It uses a separate SDK subscription, flushes its
validated v1 Hello first, then emits notifications until its absolute lifetime
ends. A successful Hello establishes readiness; the provider registers that
subscription before sending it and serializes later events behind it. Watch has
no automatic reconnect or replay promise. Reconcile a new list after disconnect.

The connection deadline covers socket connection, negotiation and later socket
I/O. Output backpressure belongs to the receiving process: the SDK cannot interrupt
an arbitrary external writer. A failure closes the owned connection and returns
a nonzero exit status. A sent mutation may have completed before a transport
failure; read its resulting state before repeating uncertain work.

`--help`, `--version` and argument refusal do not read a profile or connect to a
daemon. `just test-native-client` runs the real command and daemon selected from
Cargo's artifact messages, with a fresh private synthetic profile and capture
disabled. No installed binary, user's archive or system clipboard is used.
