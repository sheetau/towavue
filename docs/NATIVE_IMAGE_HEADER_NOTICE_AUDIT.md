# libpng, libwebp and OpenCL Headers notices

2026-09-09: finish the three specifically named notice/header cases before final source-access assembly. This is not a new runtime, codec restriction, whole-toolchain rebuild or distribution approval.

## libpng 1.6.58-1

The original 1070096-byte release archive matches the build-record-bound recipe SHA256 `28eb403f51f0f7405249132cecfe82ea5c0ef97f1b32c5a65828814ae0d34775`. Two SourceForge download URLs returned HTML and were rejected; the Netix mirror supplied the matching original. Keep the actual mirror URL in the source inventory, not a new archive made from the Git tag.

Preserve the original [PNG license and historical notices](https://github.com/pnggroup/libpng/blob/v1.6.58/LICENSE), AUTHORS, exact APNG patch and recipe. The patch is a separate modified-source input; neither patch application nor rebuild is claimed. Do not relabel the patched package as an untouched upstream build or invent patch authors.

Makefile.am separates core/x86 filter sources from contributed programs. The recipe explicitly builds pngminus conversion executables, but these are not the candidate's libpng16-16.dll. Retain their Willem van Schaik MIT notice and source headers. Greg Roelofs's examples instead offer BSD-like advertising/GPL alternatives; keep LICENSE and COPYING without treating those examples as linked library code or flattening their choice. Separate CI/pngexif MIT originals remain with source extras.

Retain the x86 filter's Mike Klein/Matt Sarett attribution and libpng terms, dfn.awk/header-generation inputs, and config.guess/config.sub/ltmain.sh with their GNU exceptions. Source/build-tool terms are not automatically DLL output terms. The complete archive remains unchanged, including unselected notices and other-target sources.

## libwebp 1.6.0-1

The 3833512-byte source archive matches recipe SHA256 `93a852c2b3efafee3723efd4636de855b46f9fe1efddd607e1f42f60fc8f2136`; the original 572-byte CMake install-location patch also matches. Neither patch is applied by the collector.

The package includes COPYING, but library headers expressly reference separate AUTHORS and [PATENTS](https://github.com/webmproject/libwebp/blob/v1.6.0/PATENTS). Preserve all three originals for WebP and SharpYUV. Google's grant has its own scope and termination conditions; retaining it does not establish clearance of all possible third-party patents.

Original CMake/header/source selection covers the WebP, demux, mux and SharpYUV library family. The static/shared recipe configurations differ: shared builds retain several default command-line tools, while disabling animation utilities, the viewer and extras. Do not label every package dependency as part of every DLL. ARM NEON files name libvpx adaptations; retain them as other-target source context, not an x86-64 NEON path. No extra standalone NOTICE/license file was found in this archive.

These two owners add four original source/patch inputs (4953375 bytes) and 33 selected original documents to the [44-owner supplement](native-source-supplements.json). The two archives contain 669/364 safe regular-file/directory entries and no links. Source signatures are not newly verified.

## OpenCL Headers 2~2026.05.29-1

OpenCL ICD Loader's original .BUILDINFO explicitly records this external package. The original package archive is 51817 bytes, SHA256 `511d129166c913df6220c3c2016397681e7888d396164696cbb9987472f95bc5`. Its detached signature verifies with existing MSYS2 key `5F944B027F7FE2091985AA2EFA11531AA0AA7F57`. Initial gpgv calls reject the armored key file; dearmor the unchanged existing key material into the isolated audit directory and verify successfully, without importing keys or changing trust/package state.

The package's .BUILDINFO binds recipe SHA256 `56b52d94bc5b3399d6d30d51e43ff9de7887667349f50de0f9621c418372749b`. That recipe binds the 89035-byte source archive, SHA256 `d9e6c48357de5002da11ce45de600e0c3ffe6ab4f628a3b9fe2b38603161658a`. Package/source inventories contain 35/84 safe regular-file/directory entries, no links. All eighteen installed CL headers are byte-identical to the source headers.

Preserve the [original Apache text](https://github.com/KhronosGroup/OpenCL-Headers/blob/v2026.05.29/LICENSE), README, CMake selection and all eighteen headers with their actual Khronos copyrights, including differing 2008/2018/2019 start years. No standalone NOTICE exists; do not fill the root license's appendix placeholders. The recipe installs existing headers and disables testing; it does not build a vendor driver.

The existing [static/API-header material collector](native-shader-inputs.json) now includes this tenth input, bound to its loader consumer. It retains both installed/source header bytes and metadata, not a new runtime DLL or binary package payload. This closes the identified external-header material gap, not proof of historical loader compilation or reproduction of its separate dispatch-generation environment. No OpenCL acceleration path is added to towavue.

## Next gate

Source-kit tests pass 675 exact files, 156 missing/corrupt input pairs, additional/package-notice failures, VCS/link/cwd/repeat and preservation. Header-kit tests pass 225 files, 46 input pairs, nine mapping failures and incomplete-extraction marker protection. The complete catalog suite also passes; all 2844 final files match tested output. M0 format, full-target Clippy and 270 tests pass; three live ignores remain unexecuted. Candidate exe and all 94 runtime files are unchanged.

Next assemble the final release's source/notice access and check the remaining explicit material limits. These three cases do not justify reopening every permissive library's full source or rebuilding every build tool. Runtime adoption, assisted Setup.exe, isolated prerequisite/Windows lifecycle and final-candidate quality/owner acceptance remain independent gates.
