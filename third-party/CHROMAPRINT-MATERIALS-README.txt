Native Chromaprint corresponding materials
=========================================

This directory covers only the pinned MSYS2 Chromaprint 1.6.1-1 DLL identified
in INPUTS.json. It is not the full FFmpeg source/notice/runtime distribution.
No binary is included, and generation does not authorize publication.

The unchanged source archive and PKGBUILD preserve the source and build recipe.
The package's .BUILDINFO records the recipe hash and original build environment;
.PKGINFO records its identity. These records are not a bit-reproducible rebuild
or the complete source distribution of the compiler and runtime dependencies.
The prepared runtime DLL must match the pinned package bytes.

The recipe selects bundled KissFFT for both static and shared builds. The
source's LICENSE.md contains the Chromaprint MIT notice and explains its LGPL
resampling code. Preserve the KissFFT COPYING copyright holder notice together
with the referenced LICENSES/BSD-3-Clause text. The copied resampling sources
retain their individual notices; COPYING.LGPLv2.1 supplies the full LGPL text.
The complete source archive preserves all other source and third-party notices.

The original package does not contain license documents. Do not substitute
the previous FFTW-backed FFmpeg notices for these candidate-specific materials.
GCC runtime and other native dependencies require their own corresponding
materials and distribution review.
