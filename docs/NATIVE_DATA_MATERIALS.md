# External Unicode data material delivery

The offline data kit supplies the 22 already-reviewed external data files and three original PCRE2 scripts that were previously outside the source catalog. It does not update Unicode versions, execute generators or rebuild DLLs.

The source of truth remains [the PCRE2/libxml2 evidence](native-unicode-table-audit.json) and [the font/line-break evidence](native-font-data-audit.json). No second hand-maintained list of their hashes is introduced. The collector reads those fixed records and binds the PCRE2/libxml2 archive identities to the companion [source supplement](native-source-supplements.json).

## Preparing and verifying

Use a cache with these exact relative locations:

- pcre2/Unicode.tables/: thirteen data files named by pcre2.data.
- pcre2/: GenerateUcd.py, GenerateCommon.py and FetchUcd.sh.
- libxml2/: Blocks-4.0.1.txt and UnicodeData-4.0.1.txt.
- libunibreak/: the seven version-prefixed filenames in libunibreak.inputs.

The originals are already retained locally from the earlier audits. When recovering them elsewhere, use their individually pinned upstream URLs/hashes, not moving latest-data endpoints. The collector performs no downloads.

```powershell
.\scripts\prepare-native-data-materials.ps1 -CacheDirectory 'vendor/msys2/native-data-20260909' -SourceMaterialsDirectory 'target/distribution/native-source-supplements-v16' -OutputDirectory 'target/distribution/native-data-materials-v1'
.\scripts\test-native-data-materials.ps1 -CacheDirectory 'vendor/msys2/native-data-20260909' -SourceMaterialsDirectory 'target/distribution/native-source-supplements-v16'
```

Choose a fresh output directory. All 25 external inputs, the original PCRE2/Unicode notices and source-inventory binding are checked before output creation. Links within input roots, unsafe paths, duplicates and input/output overlap are rejected. Existing output is not overwritten. INPUTS.json is written only after copied files verify, with relative filenames and no machine-specific paths.

The resulting 31 files total 7628780 bytes, tree SHA256 `7daf7f0a72cf71eced5ce46a336f91dd584e872e700a240836d20f114e201cf5`. README.txt explains the unchanged versions, generator locations in the companion source kit and the limits of the previous table reproductions. Audit JSON scope statements about older catalogs remain historical evidence, not a claim that these inputs are still absent from the new kit.

Tests cover independent expected file hashes, arbitrary cwd/repeat, 27 missing/corrupt pairs, missing README, six source/inventory mapping failures, links and overlap, failure after copying begins without a completion marker, and preservation of original inputs/previous output.

## Remaining release work

This kit closes external-input delivery into the local catalog, not public source access. The catalog still needs the final application's release binding and a user-facing source/notice entry point. Keep the complete library sources and their separate code notices alongside this companion; do not present the data kit as complete corresponding source for the libraries. Final runtime adoption, installer/lifecycle and quality/owner acceptance gates remain separate.
