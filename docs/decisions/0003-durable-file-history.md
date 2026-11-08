# Durable file selections in the existing archive

Status: proposed. The product behavior is selected; this storage decision awaits discussion.
No new archive format, migration or native API permission is accepted by this document.

## Required behavior

A successful new file-history entry retains the selected files' bytes, including a photo copied
as a file. Moving or deleting the originals must not prevent restoration. Image pixels copied
from an application remain an image representation. Recent history, search and favorites use
the same product-owned archive and budget. NDS remains an external consumer.

Capture must publish a complete selection or report failure. Failure must leave the current OS
clipboard usable. Old reference-only records contain no saved file bytes: preserve their meaning
and never read their historical paths as an implicit backfill.

## Recommended storage

Store each complete file selection as one versioned, uncompressed TAR representation in the
existing content-addressed blob store. Keep the existing v1 streaming transport, entry/part
tables, blob reference accounting, pin durability and garbage collection. The product API owns
the representation identifier and typed manifest; generated clients consume that definition.

The representation contains a bounded manifest with relative names, hierarchy, lengths and
content identities, followed by ordinary file bodies under controlled ASCII object names.
Source-machine absolute paths, ownership, ACLs and executable behavior are not portable content.
Directory structure and empty files/directories belong to the selection. Capture does not follow
symlinks, reparse points or special files into other locations. Unsupported names and target
collisions produce explicit outcomes; never silently replace distinct names with lossy text.

Use the qualified `tar` library for standard framing and a small product-owned layer for the
manifest, deterministic ordering and limits. Raw entry iteration allows the decoder to reject
GNU/PAX extensions, links, devices, duplicates, unexpected members and length mismatches before
they become filesystem actions. Do not use unrestricted `unpack` or trust archive member paths.
File creation uses a new private destination, non-following operations and no-overwrite names.
Validate the whole selection and its streamed digests before publishing restored files.

The archive streams data through fixed-size buffers. Manifest size, entry count and depth are
bounded in the typed contract. File capture uses owned source handles and detects observable
source changes; ordinary file reads do not provide a filesystem-wide point-in-time snapshot.
Cancellation cannot promise to interrupt an arbitrary filesystem or network-share read.

The complete container, including framing and metadata, counts toward the existing archive
budget. Identical complete selections deduplicate. Files repeated inside different selections
do not share independent archive objects in this design. This is the principal storage tradeoff.
Do not introduce compression by default: copied photos and packaged files commonly already
contain compressed bytes, and restoration should not require another complete in-memory copy.

## Native restoration and ownership

Historical restoration always offers a copy. It must never replay an earlier cut/move action or
delete an original source. The archive remains immutable; a destination application must not
receive a writable hard link to the canonical blob.

The native adapter owns the exported selection for the actual transfer lifetime. File URLs and
Windows CF_HDROP refer to physical files; they are not retained file contents. Windows virtual
file streams and macOS file promises have separate native lifetime contracts. A pathname export
must remain valid for pending paste operations, including archive eviction and application
shutdown boundaries. Clipboard replacement alone does not prove that every receiver finished.
Temporary restoration storage must be bounded, observable and included in resource qualification;
an implementation may not conceal an unbounded second archive outside the configured policy.
Its exact native ownership strategy must pass platform qualification before this work can ship.

This decision does not expand ADR 0001's FFI scope. Research safe APIs first. Any additional direct
native operation needs a separate discussion. A reference-only fallback must not be reported as
a successfully saved durable selection.

## Alternatives

| Option | Benefit | Cost |
| --- | --- | --- |
| One TAR representation per selection (recommended) | Standard stream framing; existing archive ownership, atomic publication, pinning and GC | Deduplication is at complete-selection granularity |
| One CAS object per file plus a manifest | Reuses equal files across different selections; efficient individual-object transfer | New reference/index model, migration and GC; an older daemon does not retain manifest-only references |
| Small project-owned binary stream format | Minimal framing and direct control of limits | Another persistent codec to specify, maintain and qualify instead of reusing standard framing |

The first option needs roughly 300–500 lines of product-specific codec/manifest code, excluding
tests, filesystem traversal and native restoration. A small owned codec would be of a similar
order but would add framing responsibility. Per-file CAS also changes the store and migration
model; implementation size needs a separate design. These are engineering estimates, not measured
results. Keep the product-specific implementation here; extract shared code only when another
real consumer needs the same contract.

## Acceptance before delivery

- Exercise actual native capture and paste of one file, several Unicode files, directories,
  empty files, photo files and image pixels on Windows, macOS ARM/Intel and supported Linux paths.
- Move or delete synthetic originals after capture, restart the product and restore exact bytes.
- Keep old reference-only entries honest and verify that old archive content remains accessible.
- Run the unmodified published-baseline upgrade/return fixture; add durable-selection preservation
  through that round trip. Old software is not expected to restore the new representation.
- Verify interrupted capture, changed/unavailable sources, storage pressure, full favorites,
  malformed containers, collisions, redirects, interrupted restore and restart recovery.
- Measure peak memory, elapsed time and total disk use for large files and file trees. Prove
  export cleanup without breaking an active native transfer or deleting an unrelated path.

## Sources

- [Windows Shell clipboard formats](https://learn.microsoft.com/en-us/windows/win32/shell/clipboard)
- [Windows data-transfer scenarios](https://learn.microsoft.com/en-us/windows/win32/shell/datascenarios)
- [Apple file promise provider](https://developer.apple.com/documentation/appkit/nsfilepromiseprovider)
- [TAR raw entry iteration](https://docs.rs/tar/0.4.46/tar/struct.Entries.html#method.raw)

These sources describe platform and library mechanics. The proposed storage layout and its
tradeoffs are product design conclusions; they do not establish native product acceptance.
