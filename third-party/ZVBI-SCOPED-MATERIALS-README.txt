Scoped ZVBI source materials
===========================

These materials describe the specific experimental ZVBI DLL identified in
SCOPED-INPUTS.json, not the original MSYS2 package DLL. No DLL, executable,
static archive, import library, captured broadcast or extracted subtitle is
included. Distribution approval remains false.

sources/zvbi-0.2.45-git.tar is the complete unmodified preferred source,
byte-identical to the original PKGBUILD's VCS archive checksum. Its full
copyright/license notices remain intact, including the GPL-marked files
excluded from compilation. COPYING.md, NEWS and README.md are also supplied
separately without edits. Do not describe every file in the source archive
as LGPL-only, or overwrite its per-file notices.

Apply third-party/patches/zvbi-no-program-id.patch exactly once. It includes
the original MSYS2 no-undefined adjustment. It changes seven
build/call-site/header files without changing license notices. The excluded
pdc/packet-830 sources and their license headers remain unchanged. Program-ID and
broadcast-time APIs/events are unavailable, not success stubs. This is not
a general-purpose, full-API replacement for ZVBI.

The supplied scripts/build-zvbi-native-experiment.ps1 verifies the original
archive and patch, prepares source in a fresh directory, and invokes native
Autotools/GCC. Keep this directory layout. Example from this directory:

  .\scripts\build-zvbi-native-experiment.ps1 -MsysRoot 'C:/tvbuild/msys64' `
      -SourceArchive '.\sources\zvbi-0.2.45-git.tar' `
      -BuildDirectory 'C:/tvbuild/fresh-zvbi'

Use the pinned 330-package MSYS2 snapshot described by the two input lists
under docs. These lists identify dependencies; this bundle does not include
the MSYS2 packages or bootstrap/install workflow. Obtain that workflow from
the toolchain_workflow_revision in SCOPED-INPUTS.json and verify the environment
before building. WSL is neither used nor required.

generated/libzvbi.h is the actual regenerated public header. EVIDENCE.json
compares all 224 original input files in the observed build source against
a fresh archive-plus-patch replay, and binds the six prefix input hashes to
the FFmpeg build record and staged DLL. Uncompiled original files are also
covered by this comparison; do not call all 224 files compiled sources.
The old package metadata under docs/native-zvbi-inputs.json identifies the
upstream source/recipe history, not the scoped DLL as an MSYS2 binary.

Autotools-generated files, system headers, compiler inputs and transitive
runtime material closure still require their separate audit. Hash agreement
with observed inputs is not a cryptographic build attestation or a claim
that a rebuild at another path/time produces identical DLL bytes. Maintain
the functional, final-candidate performance, installer and target-Windows
gates before adoption or publication.
