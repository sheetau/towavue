GCC runtime materials (16.2.0-3, 2026-09-08)

This is an audit/preparation bundle, not an approved release or complete
compiler-input closure. It covers the candidate's libgcc_s_seh-1.dll,
libgomp-1.dll and libstdc++-6.dll, checked in that order. It contains no
runtime binaries or original binary package archive. The package is used
only to retain its original .PKGINFO, .BUILDINFO and four license documents.
Do not add libatomic, libquadmath, other runtimes or the compiler to the
product merely because they occur in the original split-package recipe.

INPUTS.json pins the original GCC 16.2.0 source archive, exact package,
matched PKGBUILD, all seventeen patches, gdbinit, three candidate DLLs and
selected original source documents. The package's .BUILDINFO identifies
the recipe hash; all source/patch/gdbinit hashes match that recipe. The
GCC source signature is not verified and its SKIP-checksummed .sig input
is not included. Full source member/type audit found 158985 regular files
and 6094 directories, with no symlinks or unsafe member names. Only named
regular documents are extracted; the original source archive is unchanged.

The original GCC Runtime Library Exception 3.1 is an additional permission
for the files that explicitly carry it. The selected libgcc SEH/arithmetic,
libgomp and libstdc++ exception/allocation source files preserve those
notices. Keep COPYING3 and COPYING.RUNTIME together and retain the package
README without rewording it. This does not relicense the whole source
archive or transfer the compiler package's GPL label to towavue.
https://github.com/gcc-mirror/gcc/blob/releases/gcc-16.2.0/COPYING.RUNTIME

The Exception's Independent Module and Eligible Compilation Process
conditions must be assessed against actual compilation. It does not mean
all GCC-related files are exempt or that separately shipped runtime DLLs
need no corresponding-source materials. The recorded package build uses
profiled bootstrap, POSIX threads, shared/static libraries, libgomp and
libstdc++ backtrace support. No recipe, Autoreconf, bootstrap or patch
application is executed by this collector, and no independent GCC rebuild
or universal compilation-process eligibility is claimed.

Original downstream modifications remain in their exact patches. In
particular, the libgomp patch changes Windows printf-format attributes in
libgomp.h; compiler code-generation, relocation and other language changes
remain in the complete patch set. gdbinit is a compiler-package input, not
a runtime DLL. The recipe's prepare/build/split-package functions remain
the source of build instructions; do not execute them blindly.

Additional scope is not collapsed into the GCC exception: libbacktrace's
BSD-style notice and PSTL's Apache/LLVM-exception text are retained. Their
presence here does not establish every linked source/header dependency.
The libstdc++ manual distinguishes documentation terms, including GFDL,
from code terms; the full archive preserves those materials as well.
The package's original LGPL text relates to its broader runtime package
scope, including libquadmath, not proof that the three selected DLLs are
LGPL. Do not label the complete GCC source archive with one runtime license.

Remaining work includes final source-access instructions, compiler/MinGW
headers and static/embedded inputs, per-file/compilation scope, other
native notices and data, final-candidate release tests and supported-Windows
installer lifecycle. Generation does not authorize runtime replacement,
OS changes, signing, publication, purchases or upstream contact.
