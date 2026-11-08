# Rust client

`nddev-clipboard-sdk` uses the public `nddev-clipboard-api` v1 messages. It owns no archive,
credentials, service or collector. A caller can give it an owned `Read + Write` connection, or use
`LocalOperation` to connect to the configured pathname AF_UNIX endpoint on Windows, macOS or Linux.
The generic `Client::new` leaves transport policy with its caller.

`LocalOperation` consumes one absolute `Instant` deadline. It covers connection, negotiation,
requests and streamed socket I/O without renewing the budget when bytes arrive. Its cancellation
handle shuts down only that operation's connected socket; cancellation during connect is checked
before and after the deadline-bounded connect. The handle holds a weak socket reference, so keeping
it does not retain a dropped connection or affect a replacement. No worker, retry loop, collector,
listener, global runtime or environment-dependent endpoint is created.

Payloads use fixed-size chunks with deadline/cancellation checks between reads and writes, including
already buffered incoming bytes. Caller-supplied payload readers and destinations still own their
own blocking behavior; the SDK cannot interrupt an arbitrary external reader or writer. A timeout
or cancellation closes the local connection and prevents reuse. It does not prove that a previously
sent mutation was undone; reconcile the resulting archive state before repeating uncertain work.

The client negotiates without broadcasts and serializes requests. It lists history and favorites,
pins entries, records multiple streamed representations, aborts drafts and streams fetched data
into a caller-owned writer. Control frames have a fixed bound; payloads do not pass through a
complete in-memory SDK buffer. The SDK keeps exact 64-bit entry IDs in Rust.

Truncated data, I/O failures and unexpected frames invalidate the connection. Drop that client
so the daemon releases any draft. A matching remote request error preserves the connection.
Protocol errors contain no copied content or remote diagnostic message.

Qualification includes malformed/truncated streams, real pathname sockets with a stalled greeting
or continual partial progress, cancellation, and actual daemon processes with private synthetic
archives on the native runners. This does not qualify clipboard restoration, native UI or
installation; their platform acceptance remains separate.
