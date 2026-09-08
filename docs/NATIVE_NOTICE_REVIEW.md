# Native package notice review

2026-09-08: read and hash-check the original notices of the 35 runtime owners outside the existing 32-owner supplement and separate Chromaprint, OpenAL, GCC, scoped-ZVBI and MinGW material work. This is a **notice-level review**, not a complete linked-code inventory or distribution approval. An ordinary root license is not evidence that every embedded file has that license.

The exact versions, original paths and hashes are in [native-runtime-package-audit.json](native-runtime-package-audit.json), SHA256 `085583aceda22c85c0a4f8d5f8d63c35e2e825e349cfe396fa5b8037a630abb1`. The [catalog](NATIVE_MATERIAL_CATALOG.md) retains the unchanged texts under `packages/<package>/mingw64/share/licenses/`. Table names omit `mingw-w64-x86_64-`. Do not replace these originals with this summary or rewrite historical package metadata to match it.

| Owner | What the original notice actually records |
|---|---|
| aom | BSD-2-Clause and separate AOM patent license |
| brotli | Brotli Authors' MIT text |
| bzip2 | Origin/alteration/non-endorsement conditions; acknowledgment is optional |
| crypto++ | Compilation Boost license, individual public-domain statements and separate ARM CRYPTOGAMS notice |
| dav1d | VideoLAN/authors' BSD text and separate AOM patent license |
| expat | Thai Open Source/Clark Cooper and Expat maintainers' MIT notices |
| fontconfig | Multiple inherited notices; Unicode CaseFolding refers to external terms |
| harfbuzz | Old MIT, named contributors and explicit references to subdirectory COPYING files |
| highway | Apache/BSD choice, plus CC0 for the named random header; all three texts are present |
| kvazaar | Tampere/ITU/ISO/IEC/contributor BSD notice, including non-endorsement |
| libaribcaption | magicxqq MIT notice |
| libjxl | JPEG XL Project Authors' BSD notice; not an embedded-component inventory |
| libogg | Xiph BSD notice, including non-endorsement |
| libopenmpt | OpenMPT contributors and Olivier Lapicque BSD notice; not the full embedded-code list |
| libpng | Current PNG license and historical notices; separate contrib/generated files are mentioned |
| libsodium | Frank Denis ISC notice |
| libunibreak | Named authors' zlib-style notice; this text does not separately identify Unicode data terms |
| libva | Permission/disclaimer text refers to Precision Insight; standalone copyright attribution needs checking |
| libvpl | Intel MIT notice; no license to an external driver inferred |
| libwebp | Google BSD notice; patent/data/embedded notices need separate scope checking |
| libxml2 | Root explicitly points to different dict.c/list.c notices; supplemented below |
| lilv | Actual retained text is ISC; keep it even though package label permits alternatives |
| opencl-icd | Apache text only, including unfilled appendix placeholders; author/NOTICE scope needs checking |
| openh264 | Cisco BSD notice; this is not the Cisco-distributed binary patent arrangement |
| openjpeg2 | Multiple contributor BSD notices and explicit reservation of other rights |
| openssl | Apache text; check actual accompanying attribution/NOTICE rather than infer completeness |
| opus | Named contributor BSD text and three patent-license references; preserve those references |
| pcre2 | BSD-3-Clause WITH PCRE2-exception, plus explicit separate SLJIT license; supplemented below |
| serd | David Robillard ISC notice |
| sord | David Robillard ISC notice |
| sratom | David Robillard ISC notice |
| svt-av1 | BSD-3-Clause-Clear and separate AOM patent license; do not merge their grants |
| vmaf | Original is BSD-2-Clause-Patent, more specific than the package's BSD label |
| zix | David Robillard ISC notice |
| zlib | Gailly/Adler notice with origin/alteration conditions |

## Specific supplements completed

PCRE2 10.48-3's recipe enables JIT. Its original root notice expressly points to `deps/sljit/LICENSE`, a BSD-2-Clause text not present in the package notice directory. Preserve that text, the x86 implementation and build selection alongside PCRE2's original exception. This does not assign the BSD-3-Clause exception to SLJIT. [Upstream PCRE2 notice](https://github.com/PCRE2Project/pcre2/blob/pcre2-10.48/LICENCE.md).

libxml2 2.15.4-1's library source list includes `dict.c` and `list.c`. Preserve their original permission/disclaimer blocks, including Gary Pennington's attribution, separately from the root Copyright. Its full source also contains html5lib test notices; retain them without calling the tests DLL code. The external W3C XML test archive is neither included nor executed: the exact recipe still identifies it for anyone reproducing the package tests. [Original source archive](https://download.gnome.org/sources/libxml2/2.15/libxml2-2.15.4.tar.xz).

Both archives and five patch inputs match the build-record-bound recipe checksums. Archive inventories contain 505/4524 safe regular-file/directory entries. The [source supplement](native-source-supplements.json) retains 23 selected originals for these two owners, source archives and all five patches. Source signatures, patch application, rebuild and final binary selection are not newly verified here.

[OpenSSL/OpenCL/libva author and NOTICE review](NATIVE_PLATFORM_NOTICE_AUDIT.md) now retains the previously missing source-level attribution, OpenSSL's CC0 relocation inputs and separate source/build-tool terms. No standalone NOTICE exists in either inspected Apache project archive; preserve actual authors without filling example placeholders. The 40-owner supplement adds their original sources, patches and 62 selected files. External OpenCL Headers/generation, exhaustive embedded-code coverage and final delivery remain separate.

## Bounded follow-up

1. The named Unicode references are now identified: [fontconfig/HarfBuzz/libunibreak review](NATIVE_FONT_DATA_AUDIT.md) retains actual 17.0 and mixed 15.0/15.1 evidence, Microsoft's separate USE MIT text and source-test notices; all six libunibreak tables reproduce. [PCRE2/libxml2 reproduction](NATIVE_UNICODE_TABLE_AUDIT.md) establishes exact 17.0.0/4.0.1 inputs. Supplement v13 includes the current Unicode notice alongside FriBidi's unchanged pinned notice. The [external data kit](NATIVE_DATA_MATERIALS.md) now supplies 22 data files and three PCRE2 scripts in the local catalog; none of these source comparisons is a DLL rebuild or exhaustive HarfBuzz generator-environment check.
2. [libjxl/libopenmpt embedded-code review](NATIVE_CODEC_EMBEDDED_AUDIT.md) retains the xorshift MIT, Vector Class Apache and TinyFFT BSD-2-Clause references and actual consumers. [libpng/libwebp/OpenCL Headers review](NATIVE_IMAGE_HEADER_NOTICE_AUDIT.md) completes the remaining named cases: original PNG source-extra/build-tool terms, WebP PATENTS/AUTHORS and exact Khronos headers. The 44-owner supplement and ten-input API-header kit distinguish disabled/other-target/source-extra code from runtime owners. Do not turn these completed cases into an unbounded audit of every permissive source.
3. Connect the resulting notices and required source materials to the approved release, then implement assisted Setup.exe and isolated lifecycle checks. Preserve patent/trademark conditions separately; notice retention is not patent clearance.

No universal source-download or whole-toolchain rebuild requirement is added for the other permissive libraries. Keep their verified original notices; investigate further when source/recipe evidence identifies a specific extra condition or input. Existing app/native material kits, runtime quality and owner acceptance gates remain independent.
