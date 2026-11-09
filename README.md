# NDDev Clipboard

A local clipboard archive with a Rust daemon, a GNOME history picker and a
native macOS menu. Text, rich text, images and file references retain their
portable representations. Identical content is deduplicated on disk.

The product runs independently of NDS. Its daemon owns the archive and capture lifecycle;
clients use its local protocol. An NDS adapter can browse and fetch explicitly selected content
through that same interface. See [integration](docs/integration.md) and the
[release history](https://github.com/NDDev-Technocracy-OpenNetwork/nddev-clipboard/releases).

The current source is an unreleased product migration. The common history window is being
qualified separately from native tray integration, file/photo restoration and installation.
Use installers from their matching published release archives until the migration in
[issue #14](https://github.com/NDDev-Technocracy-OpenNetwork/nddev-clipboard/issues/14) is complete.
The new source requires a [complete TOML profile](docs/configuration.md); the retained legacy
installers are not yet qualified for it.

**Unpinned entries expire seven days after their latest capture. Pinned entries
stay until explicitly removed.** Re-copying recorded content refreshes its
capture timestamp; pinning and restoring do not. Unpinning an old entry makes
it eligible for the next cleanup. Expiry runs at startup and every minute, in
bounded batches (up to 1,024 entries per wake); a large backlog drains over
successive wakes. Cleanup removes metadata, search rows and unreferenced blobs.

## Architecture and resource policy

| Component | Responsibility |
|---|---|
| Rust `crates/api` | Configuration, v1 messages, content rules and generated schemas |
| Rust `crates/sdk` | Streaming client, with an owned-transport API and bounded native connector |
| Rust `config` / `server` / `maintenance` | Policy, bounded connections, lifecycle and expiry |
| Rust `store` | SQLite metadata/FTS, content-addressed blobs, leases, thumbnails |
| Rust `storage_fs` | Private archive paths, platform file opens and pin synchronization |
| Rust `capture` | Native macOS/Windows watchers; X11 XFIXES on Linux |
| GNOME extension | Wayland selection access, async streaming, picker and restore |
| macOS AppKit client | Menu, search, paging, pinning, multi-format restore |
| Native Rust client | Shared wire types, streaming SDK and finite command/subscription lifetimes |
| Python client | Retained release API under migration |
| Shared `ui` | History window under development, with public design assets and a synthetic browser harness |

The daemon streams blob I/O in 64 KiB chunks. Thumbnail/image work is serialized
and stays outside the index lock. Control frames are limited to 64 KiB, active
connections to 32, drafts to four per connection and representations to eight
per draft. Watcher queues are bounded and writes time out; a slow reader cannot
block capture. Large UI restores and BMP transcodes use private temporary
files instead of extra full-size buffers. Mac restore payloads are mapped from
an immediately unlinked file descriptor, leaving no persistent restore cache.
Both desktop UIs cap a restored representation at 512 MiB. Native OS APIs may themselves
materialize clipboard data; the archive does not promise constant memory for
those APIs or image decoding.

The default disk budget is 5 GiB. Oldest unpinned entries are evicted under
pressure; pins are preserved. A single entry larger than the budget is refused.
Pins may exhaust that budget, leaving no room for new unpinned entries. The
archive is private (0700 directory / 0600 newly created files and Unix socket),
local and unencrypted. Archive paths must be absolute and must not pass through
symlinks, Windows reparse points or `..` components. Keep a custom archive on
its real path; existing redirected archives must be relocated before upgrading.
Password-manager, concealed, transient and clipboard-history exclusion hints
are checked before capture and again before commit. These hints depend on the
source application; the utility cannot identify arbitrary unmarked secrets.

## Published Linux / GNOME release

GNOME Shell 46–50 is supported. Wayland capture requires the in-shell extension;
X11 capture runs in the daemon. `xclip` is recommended for X11 restores.
From a release archive, no Rust toolchain is needed. A source install requires
the toolchain pinned by that release and `glib-compile-schemas`.

```sh
./install.sh
gnome-extensions enable rldyour-clipboard@nddev-opennetwork
```

On Wayland, log out and back in once to load newly installed extension code.
The installer never restarts your desktop. Click the panel icon or use the
configurable shortcut (default **Super+V**). Recent and Favorites support
search, kind filters and paging. Click the star to keep a record indefinitely.
Enter/click pastes into the previous window; Shift+Enter only restores it to
the clipboard. Terminal paste shortcuts can be configured in preferences.

Wayland restore uses the one MIME representation supported by Mutter's memory
selection source. X11 `xclip` offers compatible text aliases; image restores
request BMP for remote-desktop compatibility. File entries store references,
not copies of the referenced files. `./uninstall.sh` preserves the archive.

## Published macOS 12+ release

Choose the release matching your architecture (`arm64` or `x86_64`), or build
from source with the toolchains pinned by that release.

```sh
./install-macos.sh
```

Two login agents run native capture and the AppKit menu. Click the clipboard
icon in the menu bar, search Recent or Pinned, and use the pin button to keep a
record. Choosing a row restores portable text/HTML/RTF/image representations
or multiple file URLs to the system clipboard; press **⌘V** in your application
to paste. No Accessibility permission is required. The client queries only
while you open/search/page the menu and marks restored content autogenerated
to avoid immediately recording a duplicate. `./uninstall-macos.sh` removes
code and agents, preserving the archive.

## Published Windows release

The daemon captures using `AddClipboardFormatListener`, without polling. It
normalizes CF_HTML and CF_DIB into portable MIME formats; native CF_HDROP file
reference capture is not yet implemented.
The Windows release contains the daemon; there is no Windows history GUI yet.
Create `%LOCALAPPDATA%\rldyour-clipboard`, copy the executable there and register
it under the current user's `Run` key to start at sign-in:

```powershell
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" `
    /v rldyour-clipboardd /t REG_SZ /f `
    /d "$env:LOCALAPPDATA\rldyour-clipboard\rldyour-clipboardd.exe"
```

## Configuration

The development source uses [one complete TOML profile](docs/configuration.md). It rejects missing
policy before opening an archive. For an existing archive, profile initialization requires an
explicit budget, retention and capture policy; it does not infer those values from environment
variables. Migrate all service/client callers together before an installed upgrade.

Published releases through 0.2.1 use the following environment variables. Their behavior stays
with those immutable release artifacts:

| Variable | Default | Meaning |
|---|---|---|
| `RLDYOUR_CLIPBOARD_RETENTION_DAYS` | `7` | Unpinned lifetime; `0` disables age cleanup |
| `RLDYOUR_CLIPBOARD_BUDGET` | `5368709120` | Disk bytes; `0` disables budget eviction |
| `RLDYOUR_CLIPBOARD_HOME` | Platform data directory | Relocates archive and socket together |
| `RLDYOUR_CLIPBOARD_CAPTURE` | enabled | `0` disables native capture for isolated tests |

| Platform | Archive |
|---|---|
| Linux | `~/.local/share/rldyour-clipboard` |
| macOS | `~/Library/Application Support/rldyour-clipboard` |
| Windows | `%LOCALAPPDATA%\rldyour-clipboard` |

SQLite holds metadata and full-text search; `blobs/` holds immutable content by
digest. An exclusive archive lock prevents a second daemon touching the same
store. A pin flushes its content and commits with SQLite FULL synchronization;
POSIX additionally flushes blob directory entries. Windows flushes file buffers
and WAL without a portable directory-flush guarantee. Storage hardware and
filesystem behavior still determine recovery after a power failure.
Linux normally uses a runtime socket managed by systemd; macOS uses a
launchd listener and starts capture at login. A manager-owned daemon idle-exits
only when no native watcher is active. Keep the archive on a local filesystem.

## Protocol and CLI

[The protocol specification](docs/protocol.md) describes version 1. Optional
`watch:false`, retention metadata and pagination `more` are compatible additions.
The [Rust SDK](crates/sdk) consumes the same typed messages as the daemon. It supports
filtered history, favorites, statistics, management, streamed recording/retrieval and a separate
notification subscription. A caller supplies its own transport or uses `LocalOperation` with
an explicit pathname, absolute deadline and cancellation handle on Windows, macOS and Linux.
Payload readers and destinations retain their own blocking policy. The SDK owns no archive or
collector. [Integration](docs/integration.md) defines the product boundary.

The new [native CLI](crates/cli) belongs to this unreleased source migration. Build it on an
execution host with `cargo build --locked -p nddev-clipboard` and select the daemon's complete
[TOML profile](docs/configuration.md):

```sh
nddev-clipboard --config /absolute/clipboard.toml list --favorites
nddev-clipboard --config /absolute/clipboard.toml pin 42
nddev-clipboard --config /absolute/clipboard.toml unpin 42
nddev-clipboard --config /absolute/clipboard.toml get 42 --mime image/png > restored.png
nddev-clipboard --config /absolute/clipboard.toml watch --seconds 30
```

Control output is escaped JSON. Pages include an exact decimal `key`; pass it back as the ID.
Follow `more` and use the last key as `--before`, because frame-size bounds can trim a page below
the requested limit. `get` streams raw output. Clear preserves favorites; explicit remove can
delete a favorite. A watch emits its validated Hello first and then native archive events for
its finite observation lifetime, with no reconnect or replay promise.

The legacy Python API and wheel remain for existing release callers while packaging, CI and
installation migrate. Their retained `fetch_to()` streams output; `Client.fetch()` returns bytes.
They are not the native Windows client, and the new Rust command is not yet a released asset.

## Validation

```sh
just check
```

This checks source only and expects a verified `clipboard-tools` artifact for the current source
fingerprint, installed under `.tools/bin`, plus the pinned UI dependencies. On an execution host
or runner, `just build-tools` prepares that verifier; `just test` compiles and exercises the Rust
workspace, and `just test-browser` builds and exercises the isolated common window.

Process fixtures use a short private temporary root and a complete profile with native capture
disabled. Never run fixtures against a user's archive or OS clipboard. Native installation,
capture, restoration and resource acceptance remain separate from browser checks. The Windows
[occupied-path preservation regression](https://github.com/NDDev-Technocracy-OpenNetwork/nddev-clipboard/issues/15)
currently blocks release; its check remains enabled. Legacy installers and delivery workflows
must finish migration before promotion.
See [quality notes](docs/quality.md) for limits and research sources.

## License and attribution

Original code uses [Apache-2.0](LICENSE), with attribution in [NOTICE](NOTICE).
Dependencies and the desktop platforms retain their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md). Previously issued releases retain their
original license files and immutable artifacts.

Created by Danil Silantyev, NDDev — <https://nddev.ai>.
Please name NDDev and link this repository when crediting the product.
