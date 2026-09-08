Native runtime source supplements (2026-09-08)

This is a preparation/audit bundle, not an approved release or complete
corresponding-source bundle. It covers 13 package owners lacking regular
documents under the audited package share/licenses directory, plus XZ,
FreeType and gettext-runtime with mixed-license scope or secondary notices,
and eight further library-source-first owners: libiconv, FriBidi, Game Music
Emu, mpg123, libbluray, Graphite2, GLib and libplacebo, plus libsoxr and SRT
and Little CMS, rav1e, libdovi, shaderc, SPIRV-Cross, Vulkan Loader,
PCRE2 and libxml2 (34 owners total).
Chromaprint, OpenAL, ZVBI and GCC runtimes have separate material bundles.

PCRE2's original BSD exception and separate SLJIT license remain distinct.
Its recipe enables JIT; the wineditline patch concerns the unbundled test
tool. Preserve actual generated Unicode tables without claiming that the
missing maint generator or historical data inputs have been reproduced.
libxml2's dictionary/list notices include different authors and terms from
the root introduction. Keep html5lib test notices with the source archive;
do not label those tests as DLL code. The recipe's external W3C test suite
is not included or executed. Unicode data attribution remains separate.

INPUTS.json identifies unchanged source archives, patch/template inputs and
selected original documents. Each package directory contains its exact
PKGBUILD, matched to the original package .BUILDINFO hash in the accompanying
native-runtime-package-audit.json and native-runtime-recipes.json. The audit
records all candidate DLL identities; this supplement generator does not
inspect or copy a live runtime, repeat package signature verification, or
prove bit-reproducible compilation. Upstream source signatures were not
verified; downloaded archive hashes match the package-build-matched recipes.
Libsoxr is the exception: its recipe pins a Git commit but skips an archive
checksum. Its locally generated tar hash is our archive pin, not an upstream
recipe checksum. Obtain the exact recipe repository/commit, run the recorded
archive_command with core.autocrlf=false, and place that tar in the libsoxr
cache directory before collection. Even Download will not fetch it as HTTP.
The audited tar's 137 regular files match their Git blob IDs, with one safe
internal helper link retained only in the archive; no submodules are present.
Source
signatures marked SKIP by the GMP/libssh/XZ/FreeType/gettext recipes are not
included.

The additional Unicode notice is a tracked original from the pinned upstream
tree named in INPUTS.json, not a FriBidi archive member or recipe input.
Its bytes and Git blob ID are verified independently. The generator checks
and copies this local notice without downloading or replacing it.

Keep original archives and recipes together. Patches are not applied by this
generator: in particular, ZeroMQ's recipe applies its commit patch in REVERSE.
Theora also edits export lists; Snappy copies a pkg-config template. Exact
prepare/build/package functions remain in each PKGBUILD. Do not execute an
upstream recipe blindly. Build infrastructure, transitive build inputs and
binary reproduction remain separate work.

License scope observations (not blanket distribution approval):

- Shaderc retains source/build originals and upstream dependency notices.
  Its recipe de-vendors glslang/SPIRV-Tools and its MinGW defaults request
  static libgcc/libstdc++. The actual build records GCC 16.1.0-5, glslang
  16.3.0-1, SPIRV-Tools 3~1.4.357.0-1 and SPIRV-Headers 2~1.4.357.0-1.
  These exact static/header inputs remain a separate review; neither the
  package's root Apache text nor old bundled notices close that scope.
- SPIRV-Cross preserves Apache/MIT alternatives, copyright-bearing headers,
  generated SPIR-V headers, Khronos terms and .reuse/dep5. Keep differing
  source annotations intact. Source-only test/reference/document licenses
  do not imply that those files are part of the runtime payload.
- Vulkan Loader's Windows source list includes cJSON and dirent_on_windows.
  Preserve their complete original MIT and HPND-Kevlin-Henney notices,
  including the additional Khronos/Valve/LunarG copyrights, plus REUSE
  mapping and generated-header scope. Its root Apache notice is not the
  complete notice set. Vulkan-Headers 1~1.4.357.0-1 from the original build
  record and other static/header inputs remain separate work. The patch
  adjusts the pkg-config import-library suffix; this collector does not
  apply it or regenerate loader code.
- rav1e 0.8.1 and libdovi 3.4.0 retain their original Cargo manifests/locks
  and package notices. rav1e also preserves all x86 assembly source notices,
  the ISC x86inc header, IVF license and original non-UTF-8 PATENTS bytes.
  Do not apply its root BSD label to every assembly header or dependency.
  libdovi's library lock is dolby_vision/Cargo.lock, not the outer CLI lock.
  Embedded crate/runtime materials are a separate inventory; these two
  archives alone do not cover them. The package build records use Rust
  1.87.0 and 1.97.0, not towavue's Rust 1.98.0 or the excluded BtbN runtime.
- Little CMS 2.19.1 retains its MIT core LICENSE separately from GPL fast
  float/threaded plugin sources in the full archive. Keep the original
  plugin license/header, core/header/build descriptions, authors and
  iccjpeg utility notice. The recipe builds core and fast_float as distinct
  targets but adds both to lcms2.pc, propagating the plugin link argument.
  The current FFmpeg configuration has CONFIG_LCMS2=0; LCMS is used through
  libplacebo and libjxl_cms instead. No existing feature flag is changed.
  Isolated avcodec/avfilter link replay scans the fast_float import library
  but selects no member and no static fast_float implementation archive.
  Both .text and .rdata sections match the staged candidate exactly.
  Source inspection of the two LCMS clients finds core context creation
  without plugin activation. This narrows the earlier link-argument
  concern; it is not a blanket static/header closure or release approval.
  Do not describe the full original source archive as MIT-only.
  https://github.com/mm2/Little-CMS/blob/lcms2.19.1/meson.build
- libsoxr 0.1.3 LICENCE permits LGPL 2.1-or-later and specifically points to
  embedded PFFFT terms. The matched recipe enables PFFFT and OpenMP on x64,
  disables AVFFT, and builds both shared/static libraries and LSR bindings.
  The candidate stages libsoxr.dll only. Preserve original NCAR/UCAR/Pommier
  attribution in LICENSE-PFFFT (a separate recipe input/package notice),
  pffft.c/h, Ooura's fft4g.c notice, LGPL text, authors and build descriptions.
  GPL lsr-tests source remains in the original archive, not a runtime claim.
  Both original patches stay unchanged. Git object/byte binding is not a
  new build, signature check or proof of every compiler/static input.
- SRT 1.5.7 retains its MPL 2.0 LICENSE, original source and sole Windows
  compatibility-header patch. srtcore/core.h also contains the University
  of Illinois notice; preserve it rather than substituting MPL alone.
  The exact recipe selects OpenSSL and builds shared/static variants.
  Its internal srt-ffplay link is not extracted. Complete source access
  instructions, per-file attribution and linked dependency scope remain
  necessary before distribution.
  https://github.com/Haivision/srt/blob/v1.5.7/LICENSE
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
  names UnicodeData, ArabicShaping, mirroring and bracket inputs. All four
  data files match the Unicode upstream tree pinned in INPUTS.json. Retain
  its original Unicode License V3 in unicode-16.0.0/LICENSE.txt, separately
  from LGPL, and the original data attribution in the source archive.
  The candidate DLL also identifies Unicode 16.0.0. The upstream ReadMe at
  this revision is a release template, not identical to FriBidi's expanded
  ReadMe; no equality is claimed for it or the entire upstream tree.
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
  bdj_jar, not native BD-J support. Its default embed_udfread=true makes the
  bundled libudfread 1.2.0 target static; the build inventory has no external
  libudfread, and the candidate contains its UDF diagnostics without a UDF
  DLL import. Preserve the four original LGPL implementation files and
  build selection evidence. Native BD-J sources are unconditional and use
  the bundled JNI headers when jdk_home is empty, as in the matched recipe.
  Keep both JNI headers' MPL/GPL/LGPL alternatives unmodified; the LGPL
  option is available without relabeling them as LGPL-only. No JAR, JVM or
  ASM binary is staged. This is not an independent full DLL rebuild.
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
  do not replace it with only the top-level Apache license. Its build lists
  select the AMR-NB/AMR-WB codec subset and small OSCL wrappers, not the
  broader OpenCORE MPEG video/AAC/Windows Media implementations described
  by that notice. The actual codec headers explicitly state permission
  from the 3GPP copyright holders under the preceding Apache terms.
  Preserve representative original headers, build lists and the separate
  patent disclaimer; this does not establish patent clearance.
- Snappy COPYING distinguishes library terms from benchmark-data terms
  (including CC-BY attribution). Its recipe disables tests and benchmarks.
  The original target lists and all three recipe patches keep test data in
  a separate test-support target, not the snappy library's four C++ files.
  The complete source archive still includes differently licensed data.
  Keep COPYING's data notices with that archive and do not describe its
  entire contents as BSD-only or copy test media into the runtime payload.
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
