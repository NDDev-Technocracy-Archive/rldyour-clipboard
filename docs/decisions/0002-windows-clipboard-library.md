# Safe Windows clipboard access and its dependency license

Status: proposed, awaiting owner discussion and approval.

## Problem and recommendation

The existing Windows capture adapter owns a native window procedure and extends
a borrowed recorder to a static lifetime. ADR 0001 does not allow retaining that
code. Prefer a scoped safe library over a larger application-owned FFI exception.

Select `clipboard-win` 5.4.3 with its `std` and `monitor` features for Windows.
Its monitor stays on its creating thread. The safe shutdown handle may cross to
the lifecycle owner; no application callback, static recorder or lifetime
transmute is needed. Check privacy markers and read all representations under
one clipboard guard. Keep AF_UNIX, protocol v1 and the archive owner unchanged.

## License review and exact scope

The candidate and its two resolved dependencies use Boost Software License 1.0:

| Package | Qualified version | License |
| --- | --- | --- |
| clipboard-win | 5.4.3 | BSL-1.0 |
| error-code | 3.4.0 | BSL-1.0 |
| windows-win | 3.0.0 | BSL-1.0 |

The [license text and explanation](https://www.boost.org/users/license.html)
permit use, modification and distribution, including closed-source derivatives.
Source copies and derivatives retain the applicable copyright and license text;
the text provides an exception for machine-executable object code. It does not
require disclosing product source or our changes. Our original code remains
Apache-2.0. Preserve the dependency license texts and notices in distributions
even where the binary exception applies.

Approval would permit these three pinned BSL-1.0 dependencies only, not an
unrestricted license allowlist. Add the qualified technology lines through the
dependency-governance process before product promotion. Do not merge, release
or add these dependencies to the product workspace while this ADR is proposed.

## Alternatives considered

- A project-owned native clipboard boundary: avoids another library but adds
  direct operations and lifecycle code beyond ADR 0001. It needs a separately
  approved native scope and tests for every unsafe operation.
- WinSafe: a general safe Win32 wrapper with clipboard access and GUI events.
  It remains an alternative, but its equivalent monitor, transfer and shutdown
  path has not been qualified here.
- Arboard: convenient text/image/HTML/file APIs, but its documented interface
  opens the Windows clipboard per operation. It does not expose the required
  single transaction for privacy inspection and several representations.
- Clipboard-rs uses clipboard-win in its Windows backend; it does not remove
  the BSL dependency or this review.

## Evidence and remaining acceptance

The isolated, non-distributed candidate passed [Windows run 42487615566](https://github.com/NDDev-Technocracy-OpenNetwork/nddev-clipboard/actions/runs/42487615566)
at source `6b5d78099092c47c79d2ec3a17283f064625e9a3`. Both console and Windows
subsystem binaries exercised a real notification, one locked multi-format read,
five-language Unicode, HTML, two Unicode file paths, a 3,145,765-byte binary
representation and shutdown from blocked receive. The native allocation matched
the requested binary length. No user desktop, archive or installed service was used.

This proves the library mechanics, not the completed product. Before delivery,
the production adapter must prove real privacy exclusions, native images/files,
content budget, read failures, re-copy behavior and lifecycle teardown. Durable
file bytes and the tray/window remain separate implementation work.

- [Upstream source inspected](https://github.com/DoumanAsh/clipboard-win/tree/ed27008ae5d1f10cfbb266de039f0fb35fd05a56)
- [WinSafe clipboard interface](https://docs.rs/winsafe/latest/winsafe/struct.HCLIPBOARD.html)
- [Arboard platform behavior](https://docs.rs/arboard/latest/arboard/struct.Clipboard.html)
