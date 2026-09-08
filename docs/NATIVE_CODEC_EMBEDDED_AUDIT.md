# libjxl and libopenmpt embedded-code notices

2026-09-08: resolve the two named embedded-code cases in [NATIVE_NOTICE_REVIEW.md](NATIVE_NOTICE_REVIEW.md). This is a source/notice review of the fixed candidate's three libjxl DLLs and libopenmpt-0.dll, not a rebuild, exhaustive linked SBOM or distribution approval.

## Fixed sources and build selection

[native-source-supplements.json](native-source-supplements.json) retains both original archives, their build-record-bound recipes and 32 selected original files. Neither recipe has patch inputs.

| Source | Bytes | SHA256 | Archive inventory |
|---|---:|---|---|
| libjxl 0.12.0 | 1698757 | `03e9be69a30be4011f559da75328b6d7cea8ad921fabfbd551ce10bf45cdc992` | 934 entries, including 12 internal benchmark-script links |
| libopenmpt 0.8.9 autotools | 1765691 | `d7ce84fd05d686c4bcf66af40eae857afa371442db60eeda3f874bd6cf6fc318` | 638 regular-file/directory entries, no links |

Archive paths were checked for traversal/absolute paths and unsupported types. Libjxl's links point to iqa_wrapper.sh inside tools/benchmark/metrics; they remain in the unchanged archive and are never materialized by the collector. No recipe prepare/check command, dependency fetch or source-signature verification was performed.

Libjxl's static and shared recipe configurations disable SKCMS, SJPEG, bundled PNG, tests and benchmarks, and force system Brotli, Highway and LCMS2. The shared configuration enables plugins and leaves tools at their true default; do not describe all package tools as disabled. The three candidate DLLs are distinct from package tools/plugins and their extra image dependencies. Root LICENSE/AUTHORS, CMake source lists and third_party selection remain together.

Libopenmpt's recipe builds shared/static with examples disabled. Its configure.ac selects external zlib, mpg123, Ogg, Vorbis and Vorbisfile; no recipe --without option changes that selection. Makefile.am records the library source/link lists separately from command-line tools. README explicitly says the autotools archive omits full OpenMPT include/ and contrib/ directories. Conditional credits in common/version.cpp are not proof that every named optional library is linked.

## Actual adaptations and retained originals

| Original consumer evidence | Separate notice |
|---|---|
| libjxl dec_noise.cc includes xorshift128plus-inl.h, which expressly identifies the MIT adaptation | Vincenzo Pupillo's original MIT text from [vpxyz/xorshift](https://github.com/vpxyz/xorshift/blob/5dc8bfcde1b2652bd36841343d02a270d8452d43/LICENSE) |
| libjxl enc_xyb.cc uses CubeRootAndAdd; fast_math-inl.h identifies modification from Apache-licensed vectormath_exp.h | Agner Fog's original [Vector Class Apache-2.0 text](https://github.com/vectorclass/version2/blob/c6bfcba4313ba70521ebc8fb6b2e03180d58f928/LICENSE), including its actual 2012-2019 attribution |
| libopenmpt Paula.cpp uses TinyFFT; Makefile.am includes both sources and TinyFFT's headers identify the adaptation | Ryuhei Mori's original 2017 [TinyFFT BSD-2-Clause text](https://github.com/ryuhei-mori/tinyfft/blob/1d1be6ca7027324631024fc444b8868d779e37b2/LICENSE) |

The three unchanged reference notices are tracked under third-party/codec-embedded and copied into supplement codec-embedded/. Their SHA256/size and upstream Git blob IDs are fixed in the manifest; local bytes match those blobs. The reference commits establish notice provenance, **not the historical code revision adapted by these libraries**. Preserve the original adaptation/modification statements; do not substitute current upstream implementation code or rewrite the libraries' BSD notices.

Libopenmpt's src/mpt headers retain their BSL-1.0 OR BSD-3-Clause choice. Preserve the Boost text and original root BSD text; src/mpt/LICENSE.BSD-3-Clause.txt has the same text after CRLF-to-LF normalization, but is not byte-identical, and remains unchanged inside the full archive. This is not a claim that all of libopenmpt is Boost-licensed.

Preserve Opal's Shayde/Reality public-domain origin and JP Cimalando fixes, OPL.cpp's named Schism Tracker contributor/relicensing statement, and ITCompression's GreaseMonkey/Ben Russell public-domain origin alongside OpenMPT's modifications. Load_ams.cpp's Velvet Studio source-origin statement is retained too. No new relicensing is performed or inferred by towavue.

Libjxl's APNG zlib-style notice and HEVC benchmark/configuration BSD notice remain available with source extras. They are not evidence that a HEVC software codec is linked into libjxl.dll. Likewise, source/build-tool Apache headers are not silently assigned the root BSD label. Original patent/other-rights reservations remain unchanged.

## Verification and remaining scope

The existing offline collector now covers 42 owners and five additional notices. It checks recipe/archive/document bindings and every notice before completion. Regression tests include missing/corrupt additional notices, missing/different/duplicate package notices for both new owners, repeat/arbitrary-cwd output, link exclusion and preservation of inputs/previous outputs.

This closes the specifically identified notice omissions above, not final release delivery. The catalog must select the verified new supplement; external OpenCL Headers and other explicitly named source conditions remain separate follow-up. No universal full-source download or toolchain rebuild requirement is introduced for otherwise reviewed permissive libraries. Runtime adoption, assisted Setup.exe, prerequisite/target-Windows lifecycle and final-candidate quality gates remain open.
