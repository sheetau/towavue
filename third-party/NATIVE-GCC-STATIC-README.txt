GCC 16.1 native static/header materials (evaluation only)

This kit preserves the exact MSYS2 GCC/GCC-libs 16.1.0-5 provenance recorded
by shaderc, original GNU sources, recipe and fourteen patch/build inputs.
It is separate from the GCC 16.2 runtime DLL materials. No compiler, DLL or
static library binary is copied into the output, installed or rebuilt.
The full source archive retains unrelated components under their own terms.

Both original package signatures were verified locally with the existing
MSYS2 keyring. The collector checks input hashes, not signatures. The GNU
source signature is retained but has not been verified; the source matches
the recipe checksum. No original patch or source is modified.

Preserve COPYING3 and COPYING.RUNTIME together. The GCC runtime exception
is conditional and applies to files bearing its notice; do not replace it
with a blanket GPL-only label or a blanket distribution approval.
The selected standard headers retain Hewlett-Packard, Silicon Graphics and
Boost-derived attribution as well as GCC notices. The Boost 1.0 license text
is an original archive member under libphobos, not a libphobos linkage claim.
Keep libbacktrace and PSTL's original compound text; PSTL refers to a credits
file absent from its GCC include directory. Source-only components are not
automatically assigned to the final DLL. The manual and FDL text are retained
as documentation materials, not licensed as runtime code.

Ten selected installed standard headers match their original sources.
The generated target c++config.h and original template are retained separately.
Static archive member identities/counts are observations, not a link map.
Final linked/static/MinGW/CRT/intrinsic scope, notices and source-access
assembly remain separate work. This kit does not authorize distribution.
