# Distribution licenses

NDDev Clipboard's original code is Apache-2.0. Its LICENSE and NOTICE accompany
source and binary distributions. Previous releases keep their own license files.
The dependency inventory and purpose are recorded in THIRD_PARTY_NOTICES.md;
distributions retain the applicable dependency texts and notices.

The GNOME integration runs inside the separately installed GNOME Shell. The
[GNOME review guidelines](https://gjs.guide/extensions/review-guidelines/review-guidelines.html#licensing)
require extensions to be distributed under GPL-compatible terms. Apache-2.0 is
[compatible with GPLv3](https://www.apache.org/licenses/GPL-compatibility.html);
combining it into a GPLv3 work does not remove the Apache notices. We distribute
our own extension source and do not bundle or modify GNOME Shell. Its license
does not become the daemon's license through their local IPC connection.

The native operating-system frameworks and independently installed desktop tools
retain their terms. Do not copy code or assets from another extension or application
without preserving its source, license and required attribution. The public
repository transfer does not change the license of third-party material.
