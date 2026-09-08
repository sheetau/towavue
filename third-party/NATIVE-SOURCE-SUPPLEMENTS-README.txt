Native runtime source supplements (2026-09-08)

This is a preparation/audit bundle, not an approved release or complete
corresponding-source bundle. It covers 13 package owners lacking regular
documents under the audited package share/licenses directory, plus XZ and
FreeType with mixed-license scope or secondary notices. Chromaprint, OpenAL
and ZVBI have separate material bundles.

INPUTS.json identifies unchanged source archives, patch/template inputs and
selected original documents. Each package directory contains its exact
PKGBUILD, matched to the original package .BUILDINFO hash in the accompanying
native-runtime-package-audit.json and native-runtime-recipes.json. The audit
records all candidate DLL identities; this supplement generator does not
inspect or copy a live runtime, repeat package signature verification, or
prove bit-reproducible compilation. Upstream source signatures were not
verified; archive hashes match the package-build-matched recipes. Source
signatures marked SKIP by the GMP/libssh/XZ/FreeType recipes are not included.

Keep original archives and recipes together. Patches are not applied by this
generator: in particular, ZeroMQ's recipe applies its commit patch in REVERSE.
Theora also edits export lists; Snappy copies a pkg-config template. Exact
prepare/build/package functions remain in each PKGBUILD. Do not execute an
upstream recipe blindly. Build infrastructure, transitive build inputs and
binary reproduction remain separate work.

License scope observations (not blanket distribution approval):

- XZ 5.8.3 COPYING identifies liblzma as 0BSD, separately from LGPL getopt
  in CLI tools and GPL helper scripts. The candidate stages liblzma-5.dll,
  not those tools. Preserve all COPYING files and the unchanged full archive;
  do not label every file in that archive 0BSD. The selected Makefile and
  library/common source files provide scope evidence, not compiler closure.
  https://github.com/tukaani-project/xz/blob/v5.8.3/COPYING
- FreeType 2.14.3 LICENSE.TXT offers FTL or GPL for the main project; the
  planned runtime route is FTL. The candidate media runtime is based in part
  on the work of the FreeType Team (https://freetype.org). Retain this credit
  in the distribution documentation, and retain FTL.TXT unchanged. Source
  changes must be identified: the two original MSYS2 patches enable gxvalid
  and otvalid modules and subpixel rendering. Their exact bytes and the
  recipe remain together with the unmodified source archive.
  BDF/PCF/hash notices, gzip's zlib notice and all four HarfBuzz-derived file
  notices are also preserved. The HarfBuzz script-list copyright years differ
  from the other three; do not collapse them into one generic MIT notice.
  The recipe enables available dependencies. Meson prefers system zlib and
  the audited DLL imports zlib1.dll, but no complete static/header closure
  is claimed. The bundled gzip header also remains source material.
  https://github.com/freetype/freetype/blob/VER-2-14-3/LICENSE.TXT
- GMP 6.3.0 gmp-h.in offers LGPL version 3-or-later or GPL version 2-or-later.
  The planned library route is LGPL, consistent with the version3 FFmpeg
  candidate. Keep COPYING.LESSERv3 and COPYINGv3 together; preserve the other
  original license texts, authors and the source header as well. A top-level
  GPL COPYING file does not by itself identify the library as GPL-only.
  https://gmplib.org/manual/Copying
- LZ4 1.10.0 LICENSE distinguishes the BSD-2-Clause lib directory from GPL
  tools/tests. lib/Makefile takes library sources from that directory; retain
  its full original text and lib/lz4.h for scope evidence. All selected tool
  and example notices are also retained with the full source archive; their
  inclusion is not a claim that those programs are in the runtime DLL.
  https://github.com/lz4/lz4/blob/v1.10.0/LICENSE
- LAME is used as a separate library. Its original LICENSE and COPYING are
  retained, together with mpglib attribution. Project: https://lame.sourceforge.io/
- libvpx includes separate third-party notices and WebM patent-grant texts;
  Theora also supplies a separate technology statement. Preserve these texts
  without interpreting them as clearance of every third-party patent.
- opencore-amr's opencore/NOTICE contains additional upstream contributions;
  do not replace it with only the top-level Apache license. Applicability of
  its inherited material to the built AMR subset still needs review.
- Snappy COPYING distinguishes library terms from benchmark-data terms
  (including CC-BY attribution). Its recipe disables tests and benchmarks.
  The complete archive still includes those data, so source-archive release
  obligations and linked-library scope must be reviewed separately.
- ZeroMQ's MPL text and bundled wepoll/Unity notices are retained without
  claiming that every bundled test dependency is linked into its DLL.

Selected documents are an explicit fixed list, not proof of notice closure.
Individual source headers, embedded data and static/header dependencies can
carry additional terms. Original encodings and bytes are preserved (including
non-UTF-8 author names). Retaining notices for source-only tools or tests does
not assign those licenses to every runtime DLL.

Still required before release: finish individual source/data scope review,
remaining native packages and static dependencies, complete build/staging
reproduction, release media/performance tests, and installer lifecycle trials
on the supported target Windows environment. No installation, OS setting,
product DLL replacement, publication or license purchase is authorized by
successful generation of these materials.
