# Release assembly

The initial release targets Windows 11 x64, version 1.0.0. EXE and Setup have no Authenticode signature by owner decision. Update metadata is independently signed with the existing RSA-4096 key. Public publication is reserved to the owner. See [STATUS](STATUS.md) for the remaining qualification and draft-publishing work.

## Build exact local assets

Use a clean committed checkout, the pinned Rust toolchain, Windows SDK/LLVM and a populated locked Cargo cache. Select the redistributable native FFmpeg build from [NATIVE_FFMPEG_BUILD](NATIVE_FFMPEG_BUILD.md), with headers/import libraries and its original runtime. The BtbN development runtime is excluded. Preserve the twelve native source/notice kits from the accepted catalog; the old application kit is not reused.

The existing signing key must be available to the current Windows user. Assembly reads it from LocalApplicationData/towavue-release/update-signing-key.dpapi and checks it against the public key in the checkout. It never generates or replaces that identity. Keep private files outside the checkout and release output.

Run from PowerShell, replacing the local input paths:

```powershell
.\scripts\build-release-artifacts.ps1 `
  -FfmpegPrefix 'path/to/selected-native-prefix' `
  -NativeMaterialsDirectory 'path/to/retained-native-materials' `
  -VcRedist 'path/to/vc_redist.x64.exe' `
  -NsisArchive 'path/to/nsis-3.12.zip' `
  -OutputDirectory 'target/release-1.0.0-attempt1'
```

The output must be new, with an existing parent. The command builds the Windows x64 Release application offline, snapshots it, regenerates current Rust notices and exact Git source, verifies/copies native materials and the selected 94-file runtime, creates the source companion, compiles production Setup and runs non-installing package checks. It signs the exact Setup's canonical metadata and writes checksums. `RELEASE.json` is written last; its absence means the attempt is incomplete. Failed attempts are retained for diagnosis; use a fresh output name for a retry.

`assets/` contains Setup, the matching source companion, `towavue-update-v1.txt`, `towavue-update-v1.sig` and `SHA256SUMS.txt`. `application/`, `materials-build/` and `setup-build/` retain the input/output evidence and must not be uploaded wholesale. No private key is staged. Checksums cover the other four assets; the final release receipt also records the checksum file itself.

Optional parameters select an existing Cargo target directory, package/recipe directories, supplemental Rust notice directory and official Rust runtime archive cache. Defaults match the preparation scripts in [DEVELOPMENT](DEVELOPMENT.md). No dependency, source archive, prerequisite or compiler is downloaded by this command. `LIBCLANG_PATH` follows the development build setup.

The material collector can also run independently with `prepare-release-materials.ps1 -Executable ... -FfmpegPrefix ... -NativeMaterialsDirectory ... -OutputDirectory ...`. Its caller must supply the application built from the clean current commit. Full release assembly establishes that association by building and snapshotting the executable itself. Neither path claims bit-identical compiler reproduction.

## Production and historical inputs

Generated manifests bind each version's app/source/notices while the committed native kit digests remain fixed. The app, catalog, candidate and Setup collectors accept an explicit `-InputManifest`; their default manifests still describe the older evaluation candidate. Do not rewrite historical hashes to make a new executable pass. Production Setup requires schema 2, matching executable and source-binding versions, a committed source identity and the versioned companion name. It generates the production registration namespace and schema-2 installed inventory.

The installed guide links directly to the matching companion on the versioned GitHub release. The companion retains original native guides and their historical scope statements; those records do not certify current installed behavior. The installer-source ZIP embeds the actual generated Setup manifest and instructions for reproducing the package separately.

## Verification

`scripts/test-release-materials.ps1` checks source archive equality against committed Git blobs, altered/missing/duplicate/extra source entries, dirty source, canonical version/path constraints, hashes, output overlap and reparse refusal. `test-candidate-material-archive.ps1` checks lossless deterministic ZIP packaging and interrupted-output protection. `test-release-signing.ps1` uses an isolated test key, never the production key.

The builder runs `test-local-setup.ps1 -InputManifest <generated-manifest>` on its exact production result. This checks all payload and source bytes, original notice mappings, links, install/delete paths, source-archive mutations, invalid input refusal and read-only Setup/prerequisite/registration entry points. It does not install the application or VC runtime. Actual installed update/recovery, final media behavior and supported clean-machine evidence remain separate gates. No GitHub draft is created by this local assembly command yet.
