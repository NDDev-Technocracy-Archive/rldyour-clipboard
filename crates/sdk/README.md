# Rust client

`nddev-clipboard-sdk` uses the public `nddev-clipboard-api` v1 messages. It owns no archive,
credentials, service or collector. A caller can give it an owned `Read + Write` connection, or use
`LocalOperation` to connect to the configured pathname AF_UNIX endpoint on Windows, macOS or Linux.
The generic `Client::new` leaves transport policy with its caller.

`LocalOperation` consumes one absolute `Instant` deadline. It covers connection, negotiation,
requests and streamed socket I/O without renewing the budget when bytes arrive. Its cancellation
handle shuts down only that operation's connected socket; cancellation during connect is checked
before and after the deadline-bounded connect. The handle holds a weak socket reference, so keeping
it does not retain a dropped connection or affect a replacement. No collector, listener or
environment-dependent endpoint is created.

Connected I/O uses the safe `async-io` readiness adapter, its absolute timer and an explicit
cancellation waker. The current calling thread processes events through `block_on`; the library
also owns a shared fallback I/O thread. There is no thread per connection or NDS runtime. On
Windows, shutting down a socket alone did not interrupt a pending blocking receive in native
qualification; readiness waits retain the original cancellation regression without timed polling.
The adapter owns registration and removal of OS I/O sources; this SDK adds no unsafe code.

Payloads use fixed-size chunks with deadline/cancellation checks between reads and writes, including
already buffered incoming bytes. Caller-supplied payload readers and destinations still own their
own blocking behavior; the SDK cannot interrupt an arbitrary external reader or writer. A timeout
or cancellation closes the local connection and prevents reuse. It does not prove that a previously
sent mutation was undone; reconcile the resulting archive state before repeating uncertain work.

The client negotiates without broadcasts and serializes requests. `ListQuery` selects the provider's
limit, cursor, text, kind and pinned filters; omitted fields retain provider defaults. The client
lists history and favorites, pins/removes entries, clears unpinned history, reads shared typed
archive statistics, records multiple streamed representations, aborts drafts and streams fetched data
into a caller-owned writer. Control frames have a fixed bound; payloads do not pass through a
complete in-memory SDK buffer. The SDK keeps exact 64-bit entry IDs in Rust.

`fetch_reader` lends one representation directly to a streaming decoder. The mutable borrow
keeps other requests off that connection, and the reader stops at the declared payload boundary
even when later frames have already arrived. Fully reading it permits reuse; `finish` verifies
completion and returns its metadata. Dropping an incomplete reader or finishing early closes the
connection. Decoding does not require a second complete payload buffer or temporary archive.

`part_writer` lends the sending side to an encoder with an exact declared length. It never
writes beyond that boundary. `finish` is mandatory: it flushes the body and consumes the part's
matching acknowledgement. Dropping the writer without it closes the connection even after all
body bytes were written. A matching remote refusal consumes its reply and permits reuse.
File-selection encoding can therefore stream straight to the daemon without an intermediate TAR.

`part_chunks_from` accepts a reader whose final size is unknown and emits bounded v1 chunks.
Only a clean input EOF sends the successful terminator and consumes the matching acknowledgement.
An input error, overflow, transport failure or deadline closes the connection and leaves the
draft unpublished. Known-length and unknown-length delivery retain the same content identity.

`thumbnail_to` streams straight RGBA pixels into a caller-owned writer using the shared
`ThumbnailInfo` contract. It checks nonzero geometry, tight row packing and the exact byte count
before copying. A truncated payload or failed destination closes the connection; no leftover
pixel bytes can become a later response. The original stored image remains a separate fetch.

Truncated data, I/O failures and unexpected frames invalidate the connection. Drop that client
so the daemon releases any draft. A matching remote request error preserves the connection.
Protocol errors contain no copied content or remote diagnostic message.

`LocalOperation::subscribe` uses a dedicated connection with the same absolute deadline and
cancellation policy. `Subscription::greeting` exposes its validated v1 Hello and current provider
policy. `next_event` accepts only Added, Updated, Removed and Cleared frames; an unexpected reply
invalidates the subscription. It exposes no mutation/request methods and cannot interleave a
broadcast with a request acknowledgement. A successful greeting establishes notification readiness:
the provider owns the subscription before publishing Hello and orders later frames behind it.
There is no replay or reconnect guarantee. After disconnect, reconcile a new archive listing.

Qualification includes malformed/truncated streams, real pathname sockets with a stalled greeting
or continual partial progress, cancellation, and actual daemon processes with private synthetic
archives on the native runners. This does not qualify clipboard restoration, native UI or
installation; their platform acceptance remains separate.
