# File selections

`nddev-clipboard-files` implements the file-selection storage accepted in
[ADR 0003](../../docs/decisions/0003-durable-file-history.md). Its typed persistent
contract lives in [`nddev-clipboard-api::files`](../api/src/files.rs).

`PreparedSelection` validates ordered relative components, parent directories, counts,
lengths and metadata limits before source I/O. It calculates the complete TAR length
without reading file bodies. `write_to` opens each file when needed and hashes its bytes
while streaming them. The final digest table avoids a preliminary content read. Standard
TAR framing contains a manifest, controlled object names and the digest table; source-machine
absolute paths never become archive member names.

`decode` accepts a representation-bounded reader, including the SDK's `BlobReader`.
It validates raw TAR members and the manifest, streams file bodies to its receiver, verifies
the final digests and requires the exact zero trailer and representation boundary. It never
calls an unrestricted filesystem unpack function. Malformed, linked, extended, incomplete
or trailing data cannot report successful decoding. Directory entries have no body.

The receiver must keep its outputs private until the whole call succeeds and discard them
on failure. It owns target-specific filename validation, non-following/no-overwrite creation,
export lifetime, cancellation and native restoration. This codec does not make an arbitrary
file read interruptible, implement a filesystem transaction or grant native clipboard access.
Case collisions and names unsupported by a destination require explicit target handling.

Metadata has fixed format limits; file bodies use bounded chunks. No complete selection is
buffered by the codec. Tests use real synthetic files, remove the source before restoring,
verify exact output, inspect standard TAR interoperability on Unix, and exercise corruption,
incomplete transfers, invalid topology, links, extensions and oversized metadata.
Those checks do not establish native clipboard capture or paste support.
