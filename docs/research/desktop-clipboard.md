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

Both choices use local IPC and require no TCP port. No new transport has been selected.

## Capture and restoration

`clipboard-win` provides a safe, message-driven monitor, a shutdown handle, raw representations
and file-list reading/writing. Its owned monitor can replace application-owned callback lifetime
machinery. Qualified use must hold one clipboard transaction while checking privacy markers
and collecting its representations; independently reopening it per format can combine different
copies. Inspection used commit `ed27008ae5d1f10cfbb266de039f0fb35fd05a56`, published crate 5.4.3,
under BSL-1.0. Adoption is not yet approved or qualified.

- [Monitor and shutdown](https://docs.rs/clipboard-win/latest/clipboard_win/monitor/struct.Monitor.html)
- [Formats and file-list setters](https://github.com/DoumanAsh/clipboard-win/blob/ed27008ae5d1f10cfbb266de039f0fb35fd05a56/src/formats.rs)

`clipboard-rs` exposes text, HTML, RTF, images, files and custom formats. Its inspected Windows
implementation uses `clipboard-win`. Its macOS watcher still checks `NSPasteboard.changeCount`
on an interval. A cross-platform API does not remove platform lifecycle, permission or memory
requirements. Inspection used commit `7b598dfaf7f2e1f9d9d0a441e114dc91e4663b31`; it is not adopted.

- [Clipboard-rs source](https://github.com/ChurchTao/clipboard-rs/tree/7b598dfaf7f2e1f9d9d0a441e114dc91e4663b31/src/platform)
- [Apple change count](https://developer.apple.com/documentation/appkit/nspasteboard/changecount)
- [Apple pasteboard access behavior](https://developer.apple.com/documentation/AppKit/NSPasteboard?changes=_5)

The current macOS path avoids another full-size payload allocation by borrowing NSData only
inside its owning autorelease scope. The existing Foundation crate also supplies safe
`NSData::to_vec`, which copies its bytes. Compare actual native memory and latency before
choosing a replacement; do not label moving an unchecked borrow behind another package a fix.

GNOME selection access remains a compositor integration. Mutter exposes selection source MIME
lists and asynchronous reads. Qualify against the supported shell versions and native clipboard
applications rather than treating an X11-only or another compositor's test as GNOME evidence.

- [Mutter selection source](https://gnome.pages.gitlab.gnome.org/mutter/meta/class.SelectionSource.html)

## Product acceptance

A shared Tauri history window and Rust application logic follow the existing desktop stack.
The public OpenNetwork design package supplies visual assets and tokens. A GNOME panel entry
and platform capture adapters remain part of the product where required by the OS.

- Tray/panel entry, one history window, recent/favorite views, search, paging and keyboard use.
- Text/HTML/RTF, copied image bytes and copied file lists restore into actual native applications.
- Image content and an image copied as a file retain the right native representation.
- Unicode/multiple files, missing or moved source paths, large content and cancellation have
  explicit behavior. File-reference versus durable file-content retention requires a product
  decision before changing the existing data model.
- Pins, byte-exact payloads and existing archives survive restarts and upgrades. Capture runs
  once per profile, and changing client views does not start another collector.
- Settings and schemas have one typed Rust source; clients do not parse private settings again.
- Five UI languages, right-to-left layout, accessible controls and theme checks accompany the UI.
- Qualify native surfaces on isolated execution hosts/runners with synthetic data. Existing user
  clipboard data and installed services are never fixtures.
