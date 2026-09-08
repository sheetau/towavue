Native OpenAL Soft library materials
===================================

This directory covers only the pinned libopenal-1.dll identified in INPUTS.json.
It is not the full FFmpeg source/notice/runtime distribution. No DLL, executable,
device driver or separately installed HRTF data is included. The source archive
does retain the default HRTF dataset. Generation is not publication.

The original source archive, exact package PKGBUILD, four patches and package
metadata preserve the source/build inputs. The package recipe hash identifies
commit 3da1e4e9f763ce05ebf2c0e244dcf3e66f44bb66, not the later recipe revision
that removed mingw32 from the architecture list. Retain .BUILDINFO's original
build environment rather than substituting the current local compiler versions.

The package-wide GPL-2.0-or-later metadata is preserved, not rewritten. The
library sources explicitly use Library GPL version 2 or later. COPYING supplies
that license; the copied buffer source preserves the library copyright notice.
PFFFT, fmt and GSL carry separate notices included here. The unmodified source
archive preserves the remaining source notices and the GPL utility sources.
COPYING.GPLv2 supplies the full license text for those utility sources; it does
not assign the GPL to the separately scoped OpenAL library DLL.

The default HRTF dataset is derived from KEMAR measurements by Bill Gardner and
Keith Martin, copyright 1994 MIT Media Laboratory. The provider permits research
and commercial use with attribution to the authors. Preserve this attribution
for the data embedded in the DLL, not only for a separate data-file install.
Provider: https://sound.media.mit.edu/resources/KEMAR.html
OpenAL's docs/hrtf.txt records this dataset origin and is included here.

The GPL SOFA-support and makemhr sources are separate utility targets in CMake,
not sources of the OpenAL library target. The package includes makemhr.exe, but
the candidate FFmpeg runtime imports only libopenal-1.dll from this package.
Do not bundle the entire package or describe its utilities as LGPL libraries.
None of the four patches changes source notices or the library/tool boundary.
The old filename patch applies with upstream patch's fuzz 1/offset 373; preserve
the original patches and verify the resulting files when reproducing a build.

The actual DLL hash matches the pinned package, but a bit-reproducible package
rebuild has not been demonstrated. GCC/C++ runtime and all other FFmpeg inputs
still require separate materials and distribution checks.
