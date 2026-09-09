towavue local Setup sources

This archive contains the exact installer build sources and input manifests
recorded in SOURCES.json, plus towavue's original MIT and Apache-2.0 licenses.
It is not the application's Rust source archive or an approved public release.
Preserve the directory layout when extracting into a fresh local directory.

From that directory, run Windows PowerShell with the following external inputs:

  .\scripts\build-local-setup.ps1 -Executable 'path/to/towavue.exe' -RuntimeDirectory 'path/to/exact-94-file-runtime' -SourceCompanion 'path/to/matching-companion.zip' -VcRedist 'path/to/vc_redist.x64.exe' -NsisArchive 'path/to/nsis-3.12.zip' -OutputDirectory 'path/to/fresh-output'

docs/setup-inputs.json pins the application/native source companion and its
binary binding. Extract that companion separately to read BINDING.json and
the application/native build instructions. Supply the matching executable and
the exact 94 runtime files; do not use a whole compiler prefix/bin directory.
docs/nsis-inputs.json and docs/vc-redist-inputs.json identify the original
portable compiler and Microsoft prerequisite package by size/hash/version.
Those external binaries and archives are not included in this source ZIP.

The builder verifies inputs, generates the explicit payload include/inventory,
and invokes the portable NSIS compiler. No Git checkout, development FFmpeg
environment, source download or installed NSIS toolchain is required to package
the supplied matching binaries. The build does not install, sign or publish.
Actual application/VC installation and supported-Windows lifecycle tests remain
separate. Rebuilding this Setup is not rebuilding the application/native code,
and identical installer bytes across different tool/runtime environments are
not promised. Application and native sources/notices remain in their matching
companion; the shared Microsoft runtime retains its original terms.
