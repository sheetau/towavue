# OpenSSL, OpenCL and libva notice scope

2026-09-08: inspect the original OpenSSL 3.6.4-1, OpenCL ICD Loader 2026.05.29-1 and libva 2.24.1-1 sources named by the fixed, build-record-bound MSYS2 recipes. This completes their previously named root author/NOTICE checks, not an exhaustive linked-code inventory or distribution approval.

The [40-owner supplement inventory](native-source-supplements.json) binds three original source archives and six patch/source inputs, 55449638 bytes, to the recipe hashes. It adds 62 selected original documents and source files. The archive inventories have 6179/121/138 safe regular-file/directory entries and no links. Source signatures, patch application, library rebuilds and driver installation are not performed. In particular, OpenSSL's recipe still records its `.asc` and key; a recipe SHA256 comparison is not signature verification.

## OpenSSL

The archive has no standalone NOTICE file. Its [README copyright](https://raw.githubusercontent.com/openssl/openssl/openssl-3.6.4/README.md) identifies the OpenSSL Project Authors and Eric A. Young/Tim J. Hudson; AUTHORS.md explains its copyright-holder list. Retain these originals with Apache-2.0, without inventing a missing NOTICE or substituting an older OpenSSL license.

MSYS2's relocation patch explicitly adds `pathtools.c` to libcrypto's source list and calls it from `crypto/defaults.c`. Preserve Ray Donnelly's original 2014 CC0/no-warranty declaration in both pathtools files and the recipe patch. These are not original OpenSSL archive members and must not be attributed to Apache-2.0 solely because they are used there.

Selected AES/ARIA/ChaCha source and x86-64 generators preserve the original public-domain credits, disclaimers and OpenSSL notices. The OpenSSL-provided SHA1 generator identifies Andy Polyakov and describes alternative CRYPTOGAMS sourcing while explicitly carrying Apache-2.0 here; do not replace this exact source's license with a differently distributed CRYPTOGAMS copy. Keep assembly build selections and conditional C fallbacks distinct: retaining both is not evidence that both implementations were linked.

The full source includes Perl Text::Template 1.56 under its original GPL-1.0-or-later/Artistic alternative. Retain its complete original LICENSE and the build-tool integration reference. This is a source/build tool, not evidence that its terms automatically license generated DLL output. No Perl package is installed. OpenSSL's cryptography/export warning remains in the retained README; no export, patent, security-certification or FIPS-validation conclusion is drawn here.

## OpenCL ICD Loader

The source inventory has no standalone NOTICE. The root Apache appendix retains its original unfilled example, which is not an author declaration. The Windows/common C and header inputs in CMakeLists.txt carry Khronos attribution; [adapter.h](https://raw.githubusercontent.com/KhronosGroup/OpenCL-ICD-Loader/v2026.05.29/loader/windows/adapter.h) additionally names Valve and LunarG. Preserve all selected Windows/common originals and their build selection. The separate layer-info executable and disabled test targets are not called DLL code.

Its original `.BUILDINFO` identifies `mingw-w64-x86_64-opencl-headers-2~2026.05.29-1-any`. The CMake build uses external OpenCL Headers when no local header tree is present. The [2026-09-09 follow-up](NATIVE_IMAGE_HEADER_NOTICE_AUDIT.md) now preserves that exact signed package's provenance, original source and all eighteen matching installed/source headers in the static/API-header kit. Historical loader compilation and its separate dispatch-generation environment are not newly reproduced. The loader notice does not license an external vendor implementation, and no registry/driver/runtime changes or new towavue decode path are made.

## libva

The original COPYING is permission/disclaimer text without a standalone copyright declaration. Do not fill it with an inferred owner. The source's actual core files identify Intel; Windows compatibility code identifies Microsoft, and [va_win32.c](https://raw.githubusercontent.com/intel/libva/2.24.1/va/win32/va_win32.c) also identifies Emil Velikov. Preserve their complete original headers, including differing disclaimer wording, with the unmodified COPYING.

The audited package supplies both libva.dll and libva_win32.dll. The original Meson source lists identify the four core C files and the separate Windows library, so the Windows notice is relevant rather than a Linux-only attribution guess. Retain both recipe patches and the actual core/Windows source references. No external VA driver is included or licensed by this review.

The originals from this review are collected in supplement v14 and its successors; older fixed kits remain unchanged. Subsequent codec and image/header reviews complete the specifically named notice cases. Next join final notices and required source access. Retaining these sources is not a universal whole-toolchain rebuild requirement for permissive libraries. Final-candidate quality, assisted Setup.exe, isolated prerequisite/supported-Windows lifecycle and owner acceptance gates remain open.
