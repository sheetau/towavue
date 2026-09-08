Native runtime source supplements (2026-09-08)

This is a preparation/audit bundle, not an approved release or complete
corresponding-source bundle. It covers 13 package owners lacking regular
documents under the audited package share/licenses directory, plus XZ,
FreeType and gettext-runtime with mixed-license scope or secondary notices,
and eight further library-source-first owners: libiconv, FriBidi, Game Music
Emu, mpg123, libbluray, Graphite2, GLib and libplacebo (24 owners total).
Chromaprint, OpenAL and ZVBI have separate material bundles.

INPUTS.json identifies unchanged source archives, patch/template inputs and
selected original documents. Each package directory contains its exact
PKGBUILD, matched to the original package .BUILDINFO hash in the accompanying
native-runtime-package-audit.json and native-runtime-recipes.json. The audit
records all candidate DLL identities; this supplement generator does not
inspect or copy a live runtime, repeat package signature verification, or
prove bit-reproducible compilation. Upstream source signatures were not
verified; archive hashes match the package-build-matched recipes. Source
signatures marked SKIP by the GMP/libssh/XZ/FreeType/gettext recipes are not
included.

Keep original archives and recipes together. Patches are not applied by this
generator: in particular, ZeroMQ's recipe applies its commit patch in REVERSE.
Theora also edits export lists; Snappy copies a pkg-config template. Exact
prepare/build/package functions remain in each PKGBUILD. Do not execute an
upstream recipe blindly. Build infrastructure, transitive build inputs and
binary reproduction remain separate work.

License scope observations (not blanket distribution approval):

- The eight additional packages retain their original source archives and
  all 16 checksum-bound patches/templates/hooks/scripts with exact recipes.
  Package notices map to original source bytes; libiconv's two identical
  COPYING.LIB files retain distinct package identities. GLib's COPYING is
  an internal symlink: its regular LICENSES/LGPL-2.1-or-later.txt target is
  selected instead. Both GLib links and seven earlier zimg links remain in
  the full archives, not materialized in this bundle. Redundant copies of
  identical license text remain in their original archives.
- libiconv's README distinguishes LGPL libraries/headers from GPL programs
  and documentation. The candidate stages libiconv-2.dll, not iconv.exe.
  The full source archive must not be described as exclusively LGPL.
- FriBidi includes Unicode 16.0.0 data. Its selected generator build file
  names UnicodeData, ArabicShaping, mirroring and bracket inputs. Retain
  those original data in the archive and the selected Unicode notices;
  final data attribution and applicable Unicode terms still need closure.
- Game Music Emu's original CMake defaults to the LGPL-2.1-or-later Nuked
  YM2612 implementation, and its matched recipe does not override that
  choice. MAME is a separate GPL alternative in the full source archive;
  retaining license.gpl2.txt is not evidence that MAME is linked. Preserve
  the emulator source headers and MIT gme/ext/LICENSE as well. This is
  recipe/source evidence, not independent DLL compiler-input closure.
  https://github.com/libgme/game-music-emu/blob/0.6.5/CMakeLists.txt
- mpg123's COPYING supplies project attribution and LGPL 2.1 terms, with
  exceptions for individually marked files. AUTHORS and the original patent
  discussion are retained; this is not independent patent clearance.
- libbluray retains bundled libudfread and ASM notices. Its recipe disables
  bdj_jar, not all native BD-J support; the selected build files distinguish
  these paths. Static libudfread selection still needs binary/build binding.
- Graphite2 LICENSE offers four alternatives for SIL-authored/copyrighted
  material unless otherwise specified; COPYING retains its own older
  wording. Keep both unchanged. Do not apply the MIT alternative blindly
  to third-party code, test fonts, bindings or the entire source archive.
  The original Debian copyright inventory and site license are retained
  as scope clues, not a current complete notice inventory.
  https://github.com/silnrsi/graphite/blob/1.3.15/LICENSE
- GLib's original LICENSES directory includes several different terms;
  their presence does not assign every license to libglib-2.0-0.dll.
  Keep the original commit patch, two downstream patches, two hook
  templates and Python helper without executing them during collection.
- libplacebo's recipe applies its pkg-config patch at the install prefix,
  not to the source tree. Static/header/generated inputs remain open,
  including xxhash, fast_float, Vulkan headers and glad output. Its
  fast_float build dependency is distinct from Little CMS's same-named
  plugin; do not infer the latter's license from the former's name.
- Gettext 1.0 gettext-runtime/COPYING distinguishes the LGPL libintl library
  and headers from GPL programs and documentation. The candidate stages only
  libintl-8.dll from this package, not the tools or libasprintf. Keep the full
  unmodified archive, all six original patches and the matched recipe. One
  patch changes intl/libgnuintl.in.h's Windows printf-format attribute; the
  other five target programs/tests or libasprintf. Autogen and configure also
  affect the build, so this patch list is not compiler-input closure.
  Four package notices are retained with explicit source-to-package paths;
  identical LGPL text in intl and libasprintf is not a duplicate error.
  The full archive still contains GPL tools: do not label all of it LGPL.
  https://ftp.gnu.org/pub/gnu/gettext/gettext-1.0.tar.lz
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
