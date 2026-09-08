External Unicode data and PCRE2 generator materials (2026-09-09)

This is an offline companion to native-source-supplements-v16, not an
approved release or a substitute for the corresponding library sources.
The original input bytes and version-specific attribution are unchanged.
INPUTS.json records every copied file and the companion source inventory
hash. The two original audit JSON files retain upstream URLs, Git provenance
where verified, generator/source identities and the previous reproduction
observations. Their references to older catalog/audit storage describe the
earlier observation; this kit now supplies the external inputs themselves.

pcre2/ contains thirteen Unicode 17.0.0 files in Unicode.tables/ and the
unchanged GenerateUcd.py, GenerateCommon.py and FetchUcd.sh from the exact
PCRE2 10.48 commit recorded in native-unicode-table-audit.json. The original
PCRE2 LICENCE.md accompanies the scripts. FetchUcd.sh is preserved as
provenance only: the collector never runs its download/removal operations.
GenerateUcd.py does not directly consume every file in that fetch inventory.

libxml2/ contains Blocks-4.0.1.txt and UnicodeData-4.0.1.txt. Its unchanged
codegen/genUnicode.py and codegen/rangetab.py are in the companion kit at:
mingw-w64-x86_64-libxml2/libxml2-2.15.4/codegen/

libunibreak/ contains the seven individually pinned inputs. The line-break
table uses Unicode 15.0; the other five reproduced tables use 15.1, including
emoji 15.1. The unchanged generators, sed files and template remain at:
mingw-w64-x86_64-libunibreak/libunibreak-libunibreak_7_0/src/

The previous reproduction checks used fresh working directories, original
generators, Python 3.14.2 with -B -X utf8 and only the documented LF handling.
They establish data/table correspondence, not identical rebuilt DLLs.
Copy required inputs to a fresh working directory before reproducing;
do not write generator output over these originals or the source kit.

UNICODE-LICENSE.txt is the unchanged Unicode V3 notice retrieved 2026-09-08.
Keep the older input headers and their own attribution/terms references.
It does not change the input versions or replace separate code/font/test
licenses. FriBidi's Unicode 16 materials are already in the source supplement
and retain their independently pinned notice. Fontconfig's CaseFolding data
and HarfBuzz's generated tables/Microsoft override data remain with their
source archives; this kit does not claim to reproduce HarfBuzz's packTab or
grouped-UCDXML environment.

These local materials still need the final application's same-release
delivery and user-facing notice/source entry point. No public download,
patent clearance, installation or runtime adoption is asserted here.
