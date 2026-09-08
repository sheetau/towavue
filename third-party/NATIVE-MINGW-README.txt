Native MinGW CRT/header/winpthreads materials (evaluation only)

This kit separates the current native build's 14.0.0.r353.g6df76fa52
packages from shaderc's recorded 14.0.0.r220.gd999af622 inputs. It does
not replace the GCC or Microsoft Visual C++ runtime materials.
No compiler, DLL, static library or executable binary is copied here.

Eight original MSYS2 package signatures were verified locally with the
existing keyring. The offline collector checks hashes, not signatures.
Both unchanged Git source archives reproduce the original recipe VCS
checksums. No commit signature, patch application or rebuild is claimed.
Six recipes and four original patch copies describe the two versions.

Keep the complete original runtime notices, including Zope, getopt,
gdtoa, math, musl-derived and Wine-derived notices. The MinGW-only notice
file distinguishes tools/profiling from ordinary application runtime
code. Do not assign every source archive component to the final binary.
The original Cephes FIXME is preserved, not replaced with a new license
or described as resolved permission. Per-file scope remains significant.
Winpthreads retains both its MIT and Lockless BSD-derived original text.
The GNU LGPL 2.1 text supplements the original references; it does not
replace individual copyright notices or determine final linked scope.

Six selected installed headers per version match original source bytes,
including intrinsic implementations and pthread.h. Generated _mingw.h
is retained separately from its source template. These selected headers
are not a full compiler dependency graph or a final linked-code SBOM.

The source archives retain unrelated source-only components unchanged.
Remaining historical/static/header scope and final source-access/notice
assembly are separate work. This kit does not authorize distribution.
