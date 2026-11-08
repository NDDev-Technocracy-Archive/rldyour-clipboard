# Windows clipboard candidate qualification

This isolated package evaluates clipboard-win 5.4.3. It is excluded from the
product workspace and distribution. Production has not adopted this dependency
or its BSL-1.0 license. An approval decision follows native evidence and review.

The console and Windows-subsystem binaries exercise the same real native
monitor: registration, one multi-format copy, Unicode, HTML, binary data, file
paths, and shutdown while waiting. Only an isolated CI desktop may run them;
they replace that desktop's clipboard with their own synthetic data. Neither
binary is an application installer or accesses an archive.

Keep the candidate's source license and notices if it is accepted. Remove the
isolated package after its cases have moved to the product's native acceptance.
