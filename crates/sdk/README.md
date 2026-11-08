# Rust client

`nddev-clipboard-sdk` uses the public `nddev-clipboard-api` v1 messages. It owns no archive,
credentials, service, collector or transport selection. A caller gives it an owned `Read + Write`
connection and applies its whole-operation deadline and cancellation at that transport boundary.
The SDK itself does not promise a connect timeout or cancellation for an arbitrary reader.

The client negotiates without broadcasts and serializes requests. It lists history and favorites,
pins entries, records multiple streamed representations, aborts drafts and streams fetched data
into a caller-owned writer. Control frames have a fixed bound; payloads do not pass through a
complete in-memory SDK buffer. The SDK keeps exact 64-bit entry IDs in Rust.

Truncated data, I/O failures and unexpected frames invalidate the connection. Drop that client
so the daemon releases any draft. A matching remote request error preserves the connection.
Protocol errors contain no copied content or remote diagnostic message.

Qualification includes malformed/truncated peer streams and actual daemon processes with private
synthetic archives on the native runners. This does not qualify clipboard restoration, native UI,
transport connection deadlines or installation; their platform acceptance remains separate.
