# Clipboard history window

This source is the shared desktop history view under development. The isolated browser preview
uses synthetic records and never reads or writes an OS clipboard. Its successful checks do not
qualify native installation, capture, restoration or a tray implementation.

The API models are generated from the Rust contract through Hey API. Decimal `key` values carry
exact archive identifiers across the webview boundary; presentation must never use the numeric
legacy `id` to address a record. `package.json` is generated from the root Cargo version and
`package.template.json` by `clipboard-tools`.

`vendor/opennetwork` is the unmodified public design distribution at the revision and tree in
`assets.lock.json`. Retain its Apache NOTICE and every included asset license. Authored UI code
uses those tokens and the generated icon projections; no private design repository is an input.

The view covers recent copies, history, search, favorites, paging, exact record selection,
keyboard tabs, literal rendering of untrusted text, five languages and right-to-left layout.
`HistorySource` is its typed application boundary. Native integration must implement it through
the product contract and preserve cancellation, deadlines, local ownership and clipboard format
semantics before this replaces the existing desktop clients.

Run source checks with `bun run check`. Build and run browser tests only on an execution host or
runner: `bun run build:preview` and `bun run test`. Every test worker binds its temporary static
server to an OS-assigned loopback port and closes it after the suite.
