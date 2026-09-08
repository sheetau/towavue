towavue scoped native FFmpeg source and build materials
INCOMPLETE REVIEW MATERIALS - NOT AN APPROVED RELEASE

Contents
--------
sources/ retains the original FFmpeg archive and five unchanged Git source
archives. patches/ retains all six applied patches in their original bytes.
notices/ contains selected original notices and attribution-bearing sources;
the full archives retain all other files, notices and source-only tools/tests.
INPUTS.json fixes revisions, archive/patch hashes, selected members, builder
inputs and the source-tree hashes verified against the actual candidate sources.
BUILD.json records the observed 87 configure flags, 330 installed packages and
130 selected prefix files. RUNTIME.json identifies the observed 94 candidate
runtime files and imports. Neither document contains machine-specific paths.
EVIDENCE.json is written last after source replay and live input checks.

The five Git tar files were generated at the recorded commit with:
  git -c core.autocrlf=false -c core.eol=lf -c core.abbrev=no archive
      --format=tar --prefix=COMPONENT/ --output=COMPONENT.tar COMMIT
These are locally fixed archive hashes, not upstream recipe checksum or
signature claims. FFmpeg's original source archive is retained unchanged.
All six archives contain only regular files and directories, with no submodule
or symlink entries. Source-tree verification covers original regular files after
the listed patches, including uncompiled files, not generated/system-header or
historical static-input closure. No new library build occurs in the collector.

Changes and original terms
--------------------------
ARIB applies 12.patch, 13.patch, 17.patch, then aribb24-version.patch. The first
three are unchanged upstream patches from the recorded BtbN recipe revision;
the last changes configure.ac's package version from 1.0.3 to 1.0.4 for the
FFmpeg LGPL version gate. Original source/header terms and COPYING are retained:
COPYING contains LGPLv3, while individual headers include LGPL-2.1-or-later.
Do not describe the entire source as LGPL-2.1-only.

LCEVC applies lcevc-static-link-order.patch to put its private pkg-config link
inputs after its own libraries. Retain both COPYING and LICENSE.md, including
the BSD-3-Clause-Clear terms and V-Nova's additional original scope statement.
The full source also contains separate tools/dependency notices; retaining
those does not assert every tool was incorporated in the runtime.

librist has no source patch. The native builder uses bundled MbedTLS 3.6.6 and
bundled cJSON, not the separately investigated MbedTLS 4.2 input in the older
recipe provenance. Keep librist's BSD terms, MbedTLS's original terms and
alternative-license headers, and cJSON's original MIT copyright notice.

uavs3d applies uavs3d-cdecl-guard.patch to guard the Windows calling-convention
macro. Keep its original BSD copyright holders. VVenC has no source patch;
keep its Clear BSD, authors, inherited VTM, SIMDe and JSON notices. The original
Clear BSD texts explicitly withhold patent rights; this kit is not patent
clearance. FFmpeg's original LGPL/GPL texts describe distinct source scopes,
not permission to enable GPL/nonfree components in this candidate.

Native rebuild entry points
---------------------------
Use an isolated Windows/MSYS2 installation with the pinned toolchain/media
packages in docs/msys2-*-inputs.json. The builders never use WSL. BUILD.json's
installed-package list is an observation, not an automatic installer. Compiler
bootstrap and package downloads are not included in this offline source kit.

The five dependency builders require genuine Git checkouts at INPUTS.json's
commits, not just extracted tar files: HEAD is checked and some build version
files depend on Git. Obtain those commits from the recorded repositories and
use process-local core.autocrlf=false/core.eol=lf for checkout. Apply only the
listed patches, in order, with git apply --unidiff-zero. Do not create fake
commits to bypass the builders. Archives preserve the source independently,
but an offline rebuild from these tar files alone has not been demonstrated.

Each source builder takes -MsysRoot, -SourceDirectory and a fresh
-BuildDirectory (ASCII paths without spaces):
  scripts/build-aribb24-native.ps1
  scripts/build-lcevc-native.ps1
  scripts/build-librist-native.ps1
  scripts/build-video-codec-native.ps1 -Codec uavs3d
  scripts/build-video-codec-native.ps1 -Codec vvenc
Each produces a prefix subdirectory and runs its own C API smoke test.
Obtain the separately retained scoped-ZVBI kit and follow its README to build
that sixth prefix. The original package's ZVBI DLL is not a substitute.

Then invoke scripts/build-ffmpeg-native.ps1 with -MsysRoot, a fresh
-BuildDirectory, -SourceArchive pointing to sources/ffmpeg.tar.gz, and the
six explicit -Aribb24Prefix, -LcevcPrefix, -LibristPrefix, -Uavs3dPrefix,
-VvencPrefix and -ZvbiPrefix parameters. Retain scripts/ and docs/ together:
the included builders resolve their fixed supporting files relative to them.
The FFmpeg builder executes feature, package, prefix and dependency checks.
The optional historical image-config comparison explicitly skips when absent.

BUILD.json replaces each selected prefix with @PREFIX:PACKAGE@ and the FFmpeg
output prefix with @FFMPEG_PREFIX@. Substitute your actual paths when reading
the flags; do not execute placeholder strings literally. Path normalization is
explicitly separate from raw build-record hashes in INPUTS.json. RUNTIME.json
removes only paths, preserving file hashes and import names/kinds.

Remaining delivery work
-----------------------
Other native libraries, scoped ZVBI and toolchain/runtime sources and notices
are separate kits in the native material catalog. This kit does not include
their binaries, towavue's own Rust/font/MSVC notices, Visual C++ prerequisites
or an installer. Remaining individual static/header/data scope, final source
access through the approved release, candidate quality, supported-Windows
installation/update/removal and owner acceptance are separate open gates.
No published endpoint, reproducible binary identity or distribution approval
is asserted. Original archives and licenses must not be relabeled wholesale.
