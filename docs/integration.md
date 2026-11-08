# Product integration

NDDev Clipboard owns its capture backends, archive, retention, pinning and local protocol.
It runs without an NDS account, replica or collector. The native clients and command-line
client use the same archive interface.

The Rust SDK consumes the contract types directly and streams representations. Its generic
client accepts a caller-owned connection. `LocalOperation` also offers a pathname AF_UNIX
connector with one caller-selected absolute deadline and a cancellation handle. The same deadline
covers connect, negotiation and later requests; arriving bytes do not extend it. Native I/O waits
use readiness notifications and explicit cancellation wakeups. External payload readers and
destinations retain their own blocking policy. The SDK starts no service or collector. See its
[runtime and ownership contract](../crates/sdk/README.md).

Native clients resolve the selected profile through the product's Rust configuration reader;
see [configuration](configuration.md).

## Local contract

[Protocol version 1](protocol.md) is the existing supported interface. Control frames and
streamed content have separate bounds. A read-only adapter connects with role `ui` and
`watch:false`, pages metadata and fetches one explicitly selected representation. The NDS
adapter applies its own 1 MiB fetch limit; that is not the product's archive-entry limit.

The transport is a private local socket, including the Windows AF_UNIX implementation.
The product does not open a TCP listener. An embedding application supplies an explicitly
selected local endpoint and its own operation lifetime; it does not start a second daemon
or borrow another caller's authority.

## State and effects

The archive remains product-owned. Metadata, pinned entries and content-addressed blobs
keep their existing format and meaning. An adapter does not copy the archive into a
replica or synchronize OS clipboard actions as desired state. Reading an archive entry
does not grant permission to restore it to the current clipboard.

Cancellation ends the adapter's owned work. It cannot retract a restore already handed
to the OS. Consumers retain the initiating actor and local permission through selection,
fetch and delivery. Clipboard content, search terms, paths and native responses are not
telemetry payloads.

The existing socket name, GNOME extension UUID, GSettings schema and platform archive
locations are stored-data and installation identities. A product name or repository
transfer does not silently replace them. Upgrade and removal preserve the archive.

## Qualification

Use a fresh private synthetic archive and disable native capture for protocol tests.
Never inspect or export a user's clipboard. Verify ordinary paging, pinned content,
large streamed representations, interrupted publication and restart on the real daemon.
Each consumer separately qualifies its supported protocol, lifecycle and data limits.
Published versions and provider artifacts remain immutable.
