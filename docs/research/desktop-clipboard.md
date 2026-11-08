# Desktop Clipboard research

This is investigation evidence, not an accepted architecture decision. Native exceptions and
dependency additions remain under discussion.

The product is a tray/panel utility with a small history window, recent copies, search,
favorites and restoration of text, images and files on Windows, macOS and Linux. It must work
without NDS; NDS consumes the product contract for its separately selected integration.

## Existing behavior and reproduction

The archive and local protocol already run on all three desktop operating systems. GNOME and
AppKit provide existing interfaces. Windows currently has background capture only, and its
CF_HDROP file capture is incomplete. File entries currently retain references, while copied
image data is stored as a blob. These are distinct restoration scenarios.

The newly enabled Windows filesystem regression demonstrates that a refused AF_UNIX connection
can cause the existing daemon to unlink an ordinary file at the requested endpoint. The Unix
test had checked the actual socket type; Windows had no equivalent check. The failure is in
the cleanup policy, not evidence that Windows lacks a native Unix socket implementation.

Evidence: [native run](https://github.com/NDDev-Technocracy-OpenNetwork/nddev-clipboard/actions/runs/42471588587),
Windows test `net::tests::a_regular_file_is_never_removed`. Linux and macOS ARM/Intel passed
that run; Windows reported 79 passed and one failed before reaching the process lifecycle suite.
Development has not replaced any installed service, archive or release.

## Local transport choices

| Choice | Benefit | Work and constraints |
| --- | --- | --- |
| Keep native AF_UNIX and wire v1 on all platforms | Existing clients and bounded byte-stream behavior remain applicable | Positively classify stale Windows endpoints with the official file attribute tag API; enforce private placement and ownership |
| Windows Named Pipes; Unix sockets on Linux/macOS | Kernel-owned Windows endpoint lifetime, native access controls, no socket-file reclamation | Migrate Windows clients through an explicit endpoint contract; qualify ACLs, server identity, cancellation and a whole-operation deadline |

Microsoft documents AF_UNIX as a native Windows feature. Its socket file is an NTFS reparse
point. `GetFileInformationByHandleEx(FileAttributeTagInfo)` exposes its actual tag through an
owned, non-following file handle. A refused connection alone is not a type check.

- [Windows AF_UNIX](https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/)
- [File attribute tag query](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex)
- [Named pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)

Named Pipes need explicit private access rights and remote-client rejection. The default DACL
can allow Everyone and anonymous read access. Microsoft also notes that generic write access
includes permission to create pipe instances, and documents logon-SID isolation.

The inspected `interprocess` source (commit `885d96bd89a94fa03e7f813c3e3017ebdd8f9a38`)
offers safe security-descriptor construction and optional asynchronous transport. Its synchronous
Windows local socket explicitly returns `Unsupported` for send/receive timeouts. A type-alias
replacement would break the daemon's bounded-I/O contract. An asynchronous variant requires
real cancellation/deadline and abrupt-restart qualification; no performance advantage is assumed.

- [Safe security descriptor construction](https://github.com/kotauskas/interprocess/blob/885d96bd89a94fa03e7f813c3e3017ebdd8f9a38/src/os/windows/security_descriptor/owned.rs)
- [Synchronous timeout limitation](https://github.com/kotauskas/interprocess/blob/885d96bd89a94fa03e7f813c3e3017ebdd8f9a38/src/os/windows/named_pipe/local_socket/stream.rs)
- [Tokio pipe options](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html)

Both choices use local IPC and require no TCP port. ADR 0001 retains the existing
AF_UNIX/v1 transport. The systemd/launchd activation and Windows occupied-path
boundary passed native qualification on Linux, Windows and both macOS architectures.

## Capture and restoration

`clipboard-win` provides a safe, message-driven monitor, a shutdown handle, raw representations
and file-list reading/writing. Its owned monitor can replace application-owned callback lifetime
machinery. Qualified use must hold one clipboard transaction while checking privacy markers
and collecting its representations; independently reopening it per format can combine different
copies. Inspection used commit `ed27008ae5d1f10cfbb266de039f0fb35fd05a56`, published crate 5.4.3,
under BSL-1.0. The isolated console/Windows-subsystem probe now passes native
notification, one locked multi-format read, Unicode file-list and shutdown cases;
[ADR 0002](../decisions/0002-windows-clipboard-library.md) proposes its adoption
and reviews all three resolved BSL-1.0 packages. The owner approved that exact
library/license scope on 2026-11-08. The production adapter is now being qualified.

- [Monitor and shutdown](https://docs.rs/clipboard-win/latest/clipboard_win/monitor/struct.Monitor.html)
- [Formats and file-list setters](https://github.com/DoumanAsh/clipboard-win/blob/ed27008ae5d1f10cfbb266de039f0fb35fd05a56/src/formats.rs)

`clipboard-rs` exposes text, HTML, RTF, images, files and custom formats. Its inspected Windows
implementation uses `clipboard-win`. Its macOS watcher still checks `NSPasteboard.changeCount`
on an interval. A cross-platform API does not remove platform lifecycle, permission or memory
requirements. Inspection used commit `7b598dfaf7f2e1f9d9d0a441e114dc91e4663b31`; it is not adopted.

- [Clipboard-rs source](https://github.com/ChurchTao/clipboard-rs/tree/7b598dfaf7f2e1f9d9d0a441e114dc91e4663b31/src/platform)
- [Apple change count](https://developer.apple.com/documentation/appkit/nspasteboard/changecount)
- [Apple pasteboard access behavior](https://developer.apple.com/documentation/AppKit/NSPasteboard?changes=_5)

The previous macOS path borrowed NSData through an unchecked application call.
The candidate replaces it with an immutable Foundation snapshot and safe bounded
range copies into the archive writer. `NSData::to_vec` on the entire object would
allocate a full Rust payload; `NSData::iter` also makes that copy internally.
Native byte-accuracy, privacy exclusions and one-shot 128 MiB resource evidence
passed on both architectures in [run 42487506891](https://github.com/NDDev-Technocracy-OpenNetwork/nddev-clipboard/actions/runs/42487506891).
The measured reader also hashes every byte and checks it against the fixture.

| macOS architecture | Safe range reader peak RSS | Safe whole-copy peak RSS | Range / whole-copy elapsed |
| --- | --- | --- | --- |
| ARM64 | 140,427,264 bytes | 274,546,688 bytes | 74,480 / 77,294 microseconds |
| Intel | 136,454,144 bytes | 270,585,856 bytes | 662,362 / 694,121 microseconds |

These are one-shot immutable NSData reader measurements, not tray-process RSS,
continuous capture throughput or a comparison against the removed unchecked
borrow. The range path avoids a second whole Rust payload allocation while
keeping application Rust safe. Fixture size and byte checks are identical.

GNOME selection access remains a compositor integration. Mutter exposes selection source MIME
lists and asynchronous reads. Qualify against the supported shell versions and native clipboard
applications rather than treating an X11-only or another compositor's test as GNOME evidence.

- [Mutter selection source](https://gnome.pages.gitlab.gnome.org/mutter/meta/class.SelectionSource.html)

## Selected file-history behavior

The selected product behavior is durable file content, within the archive's configured budget.
New copies should retain their bytes so restoration survives moving or deleting the originals.
A copied photo file and image pixels still restore as their respective native representations.
The storage encoding, migration and platform implementation are not yet qualified.

Publish a successful durable record only after all selected content is complete. Handle large
files with bounded streaming, cancellation and a visible refusal when the budget cannot hold
them. Existing OS clipboard content must remain usable if archival fails. Favorites preserve
their archived content under the existing pin policy; they do not grant an unlimited budget.

Existing reference-only records have no saved file bytes. Do not claim that deleted originals
can be recovered, silently manufacture content or scan historical paths as an implicit backfill.
Restoration exports must keep the immutable archive intact and retain their owned materialization
until the native clipboard transfer finishes. No data migration or installed archive has changed.

## Product acceptance

A shared Tauri history window and Rust application logic follow the existing desktop stack.
The public OpenNetwork design package supplies visual assets and tokens. A GNOME panel entry
and platform capture adapters remain part of the product where required by the OS.

- Tray/panel entry, one history window, recent/favorite views, search, paging and keyboard use.
- Text/HTML/RTF, copied image bytes and copied file lists restore into actual native applications.
- Image content and an image copied as a file retain the right native representation.
- Unicode/multiple files, missing or moved source paths, large content and cancellation have
  explicit behavior. Qualify the selected durable file-content behavior, storage encoding and
  migration before changing the existing data model.
- Pins, byte-exact payloads and existing archives survive restarts and upgrades. Capture runs
  once per profile, and changing client views does not start another collector.
- Settings and schemas have one typed Rust source; clients do not parse private settings again.
- Five UI languages, right-to-left layout, accessible controls and theme checks accompany the UI.
- Qualify native surfaces on isolated execution hosts/runners with synthetic data. Existing user
  clipboard data and installed services are never fixtures.
