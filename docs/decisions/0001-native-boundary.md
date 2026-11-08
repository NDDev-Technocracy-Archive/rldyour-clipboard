# Native platform boundary

Status: under discussion. The initial proposal is not approved. Complete the
[desktop investigation](../research/desktop-clipboard.md) and discuss transport, native API reuse
and the remaining exception scope before implementation or promotion.

Clipboard needs native clipboard notifications, platform-managed socket activation and private
filesystem operations. These are existing product capabilities. The standalone configuration
work also exposed a Windows defect: a refused AF_UNIX connection does not prove that an occupied
path is a socket. The native Windows regression test demonstrated deletion of an ordinary file.

Place all direct native calls in one `nddev-clipboard-platform` crate. Its public interface is
safe Rust, using owned handles and scoped callback ownership. The API, daemon, clients' shared
rules and build tooling forbid unsafe Rust. The platform crate must not depend on the archive,
database, service composition or NDS. It may depend on the public product contract.

The permitted native surface is limited to:

- Inherited systemd/launchd AF_UNIX listeners. Private file opens, directory synchronization and
  archive locks already use safe Rust APIs and do not need an exception. Validate inherited listener type and configured local path;
  set close-on-exec and preserve ownership on every failure.
- Existing AppKit pasteboard reads, with autorelease-scoped borrowed bytes passed synchronously
  to a safe recorder interface.
- Existing Windows clipboard listener/window, format enumeration and bounded locked handles.
  Keep callback state owned by its message thread; eliminate the existing lifetime transmute.
- A read-only `GetFileInformationByHandleEx(FileAttributeTagInfo)` query through a non-following
  owned file handle. A stale Windows endpoint may be removed only when its actual tag is
  `IO_REPARSE_TAG_AF_UNIX` and a connection probe reports refusal. Ordinary files, directories,
  symlinks and unrelated reparse points are retained.

No public unsafe function, raw pointer or borrowed native handle crosses the crate boundary.
This decision does not permit a general FFI facility, arbitrary native execution, a second
collector, a TCP listener or changed archive/wire formats.

Qualification must cover private file/redirect boundaries and socket ownership on every
supported OS, ordinary-file preservation and stale/live Windows AF_UNIX endpoints, actual
inherited listeners on Linux/macOS, synthetic native capture on isolated runners, and the
existing protocol, durability and native client suites. Each unsafe block must state and test
its ownership, size and lifetime assumptions. User clipboard data is never a fixture.
