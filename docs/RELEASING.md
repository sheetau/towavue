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

The builder runs `test-local-setup.ps1 -InputManifest <generated-manifest>` on its exact production result. This checks all payload and source bytes, original notice mappings, links, install/delete paths, source-archive mutations, invalid input refusal and read-only Setup/prerequisite/registration entry points. It does not install the application or VC runtime. Actual installed update/recovery, final media behavior and supported clean-machine evidence remain separate gates. No GitHub draft is created by the local assembly command.

## Build and upload a draft

After the final qualification gates in STATUS are satisfied, run `scripts/publish-release.ps1` with the same arguments as the builder. The command uses the authenticated GitHub CLI, requires the clean `main` branch and the `sheetau/towavue` origin, builds/verifies the artifacts, pushes the source without force, creates the exact version tag and uploads an unpublished stable draft. The committed notes come from `docs/releases/<version>.md`. It never publishes the release.

Use `-CheckOnly` to build and perform read-only GitHub preflight without a push, tag or draft change. For an already completed build, use:

```powershell
.\scripts\publish-release.ps1 -PreparedDirectory 'target/release-1.0.0-attempt1' -CheckOnly
# After qualification, upload the same verified build:
.\scripts\publish-release.ps1 -PreparedDirectory 'target/release-1.0.0-attempt1'
```

The prepared build must match the current clean commit. Retrying that same command verifies the existing draft's source tag, ownership marker, stable channel, exact asset names, sizes and server SHA-256 digests. It uploads only missing assets. Identical uploaded files are kept; different files, unrelated/edited drafts and all published releases are refused. A known empty GitHub `starter` placeholder can be removed and retried only on the matching draft. There is no `--clobber`, forced tag update or published-asset replacement. If code or notes change, create and qualify a new build before attempting another draft; the command does not silently retarget an existing version.

An incomplete draft says not to publish it. Normal release notes replace that notice only after every asset is verified. `DRAFT.json` is the local receipt for that completed draft; it records the release ID, commit and remote asset digests. The owner then publishes the stable release as latest in GitHub. Installed apps discover it on their next startup, manual or periodic update check; GitHub publication is not an immediate push to running apps.

`test-release-publishing.ps1` exercises ownership, published/prerelease/tag refusal, missing-only retries, unchanged no-ops and empty-upload recovery without GitHub mutations. Add `-ArtifactDirectory <completed-build>` for signature/checksum/source-blob verification and seven actual local tampering/incomplete-asset controls. The production private key is not used by this test.

The implementation follows the documented [draft creation options](https://cli.github.com/manual/gh_release_create), [upload behavior](https://cli.github.com/manual/gh_release_upload) and [release asset state/size/digest fields](https://docs.github.com/en/rest/releases/assets?apiVersion=2022-11-28). REST requests select API version 2022-11-28. Final remote execution and draft visibility still need their own evidence; pure planner tests are not an uploaded release.
