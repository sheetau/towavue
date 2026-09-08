# PCRE2 and libxml2 Unicode table evidence

2026-09-08: regenerate the two previously unidentified data-derived tables in fresh, ignored audit directories. Both match their original release sources after **CRLF-to-LF normalization only**. This establishes the data/generator correspondence, not a DLL rebuild or distribution approval. Application and runtime binaries are untouched.

[The fixed inventory](native-unicode-table-audit.json) records original URLs, sizes, SHA256 values, PCRE2 Git blobs, generator identities and both output comparisons. The source archives are already bound to the package build recipes in [native-source-supplements.json](native-source-supplements.json).

## PCRE2 10.48

The release archive omits `maint`, but tag `pcre2-10.48` resolves through annotated tag object `42821dd6a614c496ff0959f91bc779f57886d7e9` to commit `7978954dbd2efc6f2196869290553cf1871b4ce6`. The three original scripts match that commit's Git blobs. Every one of the thirteen files fetched individually from the official Unicode 17.0.0 data directories also matches the corresponding committed blob.

Use the unchanged `GenerateUcd.py` and `GenerateCommon.py` beside `Unicode.tables/`, with data paths from the inventory. From that fresh directory, run:

```powershell
python -B -X utf8 GenerateUcd.py pcre2_ucd.c
```

The inspected `FetchUcd.sh` is **not executed**: its removal/retrieval behavior is unnecessary for this check. Its thirteen-file inventory includes UnicodeData.txt, which this particular generator no longer consumes. Do not describe all thirteen as directly read by GenerateUcd.py.

The generated file is 365200 bytes on Windows; the original `src/pcre2_ucd.c` is 359377 bytes. Converting only CRLF to LF gives the original SHA256 `cfa8bcb3ad316e09ebfdf3e4b07d708a9d4e53795cc7c0bf04ce5156d7695adc`. This is a full-file comparison, not selected table samples.

## libxml2 2.15.4

The unchanged release members `codegen/genUnicode.py` and `codegen/rangetab.py` use the official `Blocks-4.0.1.txt` and `UnicodeData-4.0.1.txt`. Place the two data files in a fresh working directory and the two scripts in its `codegen/` child, then run:

```powershell
python -B -X utf8 codegen/genUnicode.py
```

The generator reports 125 block descriptions and 15100 characters generating 36 categories. Its 71515-byte output becomes the original 70042-byte `codegen/unicode.inc` after CRLF-to-LF conversion, SHA256 `fa6edc16172c03d05719bfdb899a338fdbc3764c7848f0bdab2a3ed81603b95e`. The actual library consumer is `xmlregexp.c`. Do not replace these historical inputs with Unicode 16 or 17 merely because newer data is available.

Both runs used Python 3.14.2 with bytecode-cache creation disabled. No source archive member, actual build directory or installed runtime was overwritten. Repeating in separate fresh directories must yield the recorded output hashes before normalization and the original hashes afterward.

## Notice scope and remaining delivery

The [original Unicode License V3](../third-party/unicode-data-20260908/LICENSE.txt) is retained byte-for-byte from [Unicode's license endpoint](https://www.unicode.org/license.txt), as observed on 2026-09-08; it is not a file from either library release. Unicode's [terms, definitions and section 3.3](https://www.unicode.org/copyright.html) cover computer data files under Public with V3 unless specific release/file/documentation terms indicate otherwise. This is the basis for retaining the current notice alongside the original data attribution, not for rewriting older copyright statements or extending data permissions to code-chart PDFs or fonts.

The 17.0.0 data headers retain their 2025 Unicode attribution and terms references. Blocks 4.0.1 retains its 1991-2004 copyright and historical terms URL; UnicodeData 4.0.1 itself has no leading notice. Preserve these original headers with the data materials. The 2026 license does not turn the input data into a 2026 version. FriBidi's separately pinned Unicode 16 notice remains unchanged, as do PCRE2/SLJIT and libxml2's distinct code notices.

The [external data material kit](NATIVE_DATA_MATERIALS.md) now collects these exact data/generator inputs, original notices and unchanged evidence into the local catalog. Supplement v13 and successors include the current Unicode notice; older fixed kits remain unchanged. [Fontconfig, HarfBuzz and libunibreak references](NATIVE_FONT_DATA_AUDIT.md) are identified separately. The named notice cases have subsequent reviews; final release binding/source access and installer/quality gates remain open.
