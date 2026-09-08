# Font and line-break data attribution

2026-09-08: inspect the exact fontconfig 2.18.3-1, HarfBuzz 14.4.0-1 and libunibreak 7.0-1 sources named by the already build-record-bound MSYS2 recipes. This resolves the three named Unicode reference checks; it is not an exhaustive linked-code inventory, a DLL rebuild or release approval.

[Fixed evidence](native-font-data-audit.json) records source/data identities and six libunibreak reproductions. [The 37-owner supplement](native-source-supplements.json) adds three unchanged source archives, five recipe patch/template inputs and 52 selected original documents, including generator inputs and distinct notices. Every archive/build input matches its recipe SHA256. Archive inventories contain 745/3980/69 entries. HarfBuzz's only link, `CLAUDE.md -> AGENTS.md`, remains inside the original archive and is not extracted. No package build, test, download/update target or installation hook is executed.

## What each library actually uses

| Library | Source evidence | Retained attribution |
|---|---|---|
| fontconfig | `fc-case/meson.build` uses the shipped CaseFolding.txt, generator and template; data is byte-identical to official Unicode 17.0.0 | Original combined COPYING, the 2025 data header and separate Unicode notice |
| HarfBuzz | UCD, emoji and USE table headers identify Unicode 17.0.0; UCD is the default callback path unless disabled | Root Old MIT, Microsoft MIT for the three `ms-use` inputs, Unicode data notice and original generated headers |
| libunibreak | Line table uses 15.0; word, grapheme, Indic conjunct, East Asian width and emoji tables use 15.1 | Original author-specific zlib-style LICENCE, unchanged generated headers and separate Unicode notice |

HarfBuzz's USE generator explicitly consumes Microsoft's additional Indic property inputs. Preserve their [original MIT text](https://raw.githubusercontent.com/harfbuzz/harfbuzz/14.4.0/src/ms-use/COPYING); the root Old MIT notice is not a replacement. Its test tree also contains SIL OFL, Adobe Apache and Unicode Apache notices. These apply to source-test materials, not a claim that test fonts are installed with towavue or that all those fonts have one license. In particular, a Unicode-authored Apache test notice must not be relabeled as Unicode V3 data.

The HarfBuzz UCD generator uses packTab and grouped UCDXML. Their exact generation environment is not reproduced here. Keep the original generated source and header evidence without treating the generic `latest` URL in a generator as a pinned input, or inferring that availability of GLib makes the UCD table unused.

## libunibreak reproduction

Retrieve only the seven versioned official text files listed in the evidence, not the moving `UNIDATA` URLs in Makefile.am. The five Python generators accept explicit input paths; East Asian width also takes its separate LineBreak 15.1 path. Use the unchanged release scripts with Python 3.14.2, `-B -X utf8`. The sixth, line-break, follows the original `extract_data.sed`, `expand_single.sed`, Python generator and template sequence with LineBreak 15.0.

All six complete outputs match the original release tables when captured stdout is joined with LF and a final LF and source CRLF is normalized to LF. No table entries, version headers, whitespace other than line endings, or generator code are changed. This confirms the mixed-version source input, not the behavior or identity of a rebuilt DLL. Original library source directories are not output targets.

## Notice and delivery boundaries

The current original [Unicode License V3](../third-party/unicode-data-20260908/LICENSE.txt) is now copied by the source supplement alongside FriBidi's separately pinned Unicode 16 notice. Keep older data headers unchanged. The [Unicode terms](https://www.unicode.org/copyright.html) define computer-data scope and preserve release-specific exceptions; this does not grant font/code-chart or patent rights. Original source-test Apache/OFL terms remain separate.

The external libunibreak data inputs now join the PCRE2/libxml2 inputs in the [external data material kit](NATIVE_DATA_MATERIALS.md), using the unchanged evidence's URLs/hashes. Fontconfig's actual data and HarfBuzz/libunibreak generated tables and code notices remain inside the preserved source archives and selected documents. The Unicode notice is included in supplement v13 and successors; older fixed kits remain unchanged.

The named embedded-code/NOTICE cases have subsequent checks in [the package review](NATIVE_NOTICE_REVIEW.md). Next bind final notices/source access to the release. Do not turn this bounded check into a universal rebuild requirement for permissive libraries. Installer, runtime quality, supported-Windows lifecycle and owner acceptance gates remain open.
