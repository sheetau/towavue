towavue - Rust runtime notice materials

1.98.0-x86_64-pc-windows-msvc: the toolchain used to build towavue.
1.97.1-x86_64-pc-windows-gnu: the compiler version observed in librav1e.a
from the fixed FFmpeg build image. See INPUTS.json for exact identities.

COPYRIGHT-library.html is the unmodified official standard-library report.
The root COPYRIGHT, LICENSE-APACHE and LICENSE-MIT are also retained.
licenses/ preserves the release's complete SPDX text dictionary; inclusion
does not mean every license in that dictionary applies to this application.
The report includes other platforms and build dependencies and is not a
list of components proven to be linked into the Windows executable.

compiler-builtins/LICENSE.txt and compiler-builtins/libm/LICENSE.txt come
from the matching official rust-src component. They are kept separately
because the standard-library HTML does not identify compiler-builtins.
Their AND/OR terms and LLVM exception are preserved, not replaced with
the standard library's MIT OR Apache-2.0 declaration. Individual source
notices and any additional linked native runtime materials remain part
of the distribution review.

INPUTS.json identifies archive URLs, SHA256 checksums, original entry
paths and byte identities. No compiler executable or library is included.
The Rust Cargo dependency bundle, native FFmpeg/GCC and Microsoft runtime
materials are separate. This ZIP is not an installer or a full source
bundle and does not establish complete redistribution compliance.
