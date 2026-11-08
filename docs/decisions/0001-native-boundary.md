# Safe native adapters and the remaining FFI boundary

Status: accepted, 2026-11-08, after discussion of the narrower safe-first approach.
The direction is safe APIs first; a crate boundary alone does not justify keeping an
unsafe operation. The [desktop investigation](../research/desktop-clipboard.md) records the
transport alternatives and native API candidates.

Clipboard needs native clipboard notifications, platform-managed socket activation and private
filesystem operations. These are existing product capabilities. The standalone configuration
work also exposed a Windows defect: a refused AF_UNIX connection does not prove that an occupied
path is a socket. The native Windows regression test demonstrated deletion of an ordinary file.

Keeping AF_UNIX and protocol v1 is the proposed transport choice. It does not require a new
transport protocol, a TCP service or retaining the existing application-owned callbacks.
Transport selection and permission to introduce a native exception are separate decisions.

## Remove avoidable unsafe operations first

- Private file opens, directory synchronization and archive locks already use safe Rust APIs.
  They are not reasons for an FFI exception.
- Prefer a qualified safe Windows clipboard monitor and scoped format access over the current
  application-owned window callback and lifetime transmute. The investigated `clipboard-win`
  API is a candidate, not an adopted dependency. Check privacy markers and collect formats in
  one clipboard transaction; preserve cancellation and shutdown ownership.
- Use safe Foundation data access when qualifying the macOS adapter. The existing unchecked
  NSData borrow is not included in this candidate exception. Compare the safe path's real
  memory and latency with the baseline before proposing any separate optimization exception.
- Linux X11 already uses a safe client API. GNOME Wayland remains a compositor adapter;
  neither requires adding application-owned Rust unsafe for this migration.
- The shared history window and tray follow the common desktop application stack. They do not
  justify retaining a custom unsafe GUI callback implementation.

New dependencies must be selected, licensed and qualified before adoption. In particular,
Windows private-directory ownership and access control still need qualification; do not treat
inherited permissions on every custom path as a proved private boundary.

## Approved direct operations

Direct operations are limited to inherited systemd/launchd listener acquisition and a read-only
Windows file attribute tag query, inside `nddev-clipboard-platform`. Safe standard-library
metadata does not expose the Windows reparse tag; the investigated safe file wrapper also lacks
that query. Acquiring an owned listener from a service manager's raw descriptor likewise needs a
native ownership boundary. Use ordinary safe Rust for all surrounding validation and lifecycle.

- Validate inherited listener type and configured local path, set close-on-exec, and preserve
  handle ownership on every failure.
- Query `GetFileInformationByHandleEx(FileAttributeTagInfo)` through a non-following owned
  file handle. Remove a stale Windows endpoint only after a positive
  `IO_REPARSE_TAG_AF_UNIX` classification and a refused connection probe. Preserve ordinary
  files, directories, symlinks, unrelated reparse points and live listeners.

The proposed crate has a safe Rust interface and depends on no archive, database, service
composition, module implementation or NDS. Raw pointers and unowned handles do not cross its
public boundary. Scoped standard-library handle borrows retain their checked lifetimes.
The API, SDK, ordinary daemon logic and build tooling forbid unsafe Rust.

This is not permission for a general FFI facility, arbitrary native execution, another
collector, retaining every legacy unsafe block or changing archive/wire formats. The exact
scope does not expand to new native operations without another discussion. Native qualification
is a release requirement; acceptance of the approach does not substitute for that evidence.

- [Rust Windows metadata API](https://doc.rust-lang.org/std/os/windows/fs/trait.MetadataExt.html)
- [Windows attribute-tag query](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getfileinformationbyhandleex)

Qualification must cover private file/redirect boundaries and socket ownership on every
supported OS, ordinary-file preservation and stale/live Windows AF_UNIX endpoints, actual
inherited listeners on Linux/macOS, synthetic native capture on isolated runners, and the
existing protocol, durability and native client suites. Each unsafe block must state and test
its ownership, size and lifetime assumptions. User clipboard data is never a fixture.
