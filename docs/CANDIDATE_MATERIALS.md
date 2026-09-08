# Evaluated candidate: source and notice binding

The collector joins the evaluated application's source snapshot and the fixed thirteen-kit catalog into one movable offline directory. It checks the actual executable and all 94 runtime files, but copies only source/notice materials. This is not Setup.exe, runtime adoption, public delivery or a complete linked SBOM.

## Fixed inputs

[candidate-material-inputs.json](candidate-material-inputs.json) pins catalog v10's inventory/completion marker and the application source ZIP. Candidate binary identities are read from the hash-checked catalog's existing app and FFmpeg manifests, not another handwritten list.

- Application: 10287104 bytes, SHA256 `03125262c28c0df0190b6d0dfe7b0efb1dfbceb55946c4af7e40e4af0aad43f3`. Its source snapshot is commit `1504beef672517b3ae9d1a750e70ef56435ef0b3`, as recorded in [the build evaluation](DEVELOPMENT.md). The workspace's application code, manifests, lock and toolchain have no changes from that snapshot at this checkpoint. This association is historical build evidence, not a bit-identical rebuild claim.
- Application source ZIP: 1086184 bytes, SHA256 `1378dc03aa67d07574683e80d0a908cc1d52c4f032256c56a4b4bc0f86e676ea`. All 172 regular members are compared against the commit's Git blob identities by the test; missing, duplicate or changed members fail. The collector checks the pinned ZIP bytes and its Git commit comment. No working-tree or untracked files enter the archive.
- Runtime: 84 DLLs match the original package audit by name, size and hash; nine rebuilt FFmpeg files point to their source/build kit; scoped ZVBI points to its own patched-source kit, not the old package. Observed runtime import edges must resolve within the 94-file set. This does not re-inspect PE headers or prove every dynamic load or static dependency.
- Catalog: all 2875 original files, including its own inventory and completion marker, must match. All thirteen kits remain unchanged. Historical scope statements within them are preserved; the new binding is the current candidate list.

## Prepare

From the repository, create the pinned application archive in a fresh local directory:

```powershell
git -c core.autocrlf=false -c core.eol=lf archive --format=zip --prefix=towavue/ --output=path/to/towavue-source-1504bee.zip 1504beef672517b3ae9d1a750e70ef56435ef0b3
```

Use a runtime input directory containing exactly the 94 names in `native-ffmpeg-materials-v1/RUNTIME.json`. A native build's full `prefix/bin` also has seven link libraries and ffplay.exe: it is not the selected runtime set. Copy the selected original files to a fresh staging directory and keep the build prefix unchanged. Do not add OS DLLs or the VC redistributable here.

```powershell
.\scripts\prepare-candidate-materials.ps1 -Executable 'path/to/towavue.exe' -RuntimeDirectory 'path/to/selected-runtime' -CatalogDirectory 'target/native-material-catalog-v10' -ApplicationSource 'path/to/towavue-source-1504bee.zip' -OutputDirectory 'target/candidate-materials-v2'
.\scripts\test-candidate-materials.ps1 -Executable 'path/to/towavue.exe' -RuntimeDirectory 'path/to/selected-runtime' -CatalogDirectory 'target/native-material-catalog-v10' -ApplicationSource 'path/to/towavue-source-1504bee.zip'
```

Both scripts work from another current directory when called by absolute path. Output must be fresh and separate from inputs. A bad input never triggers download, repair, build or installation. The full catalog is validated before output, and each copied file is checked again. `INPUTS.json` is the final completion marker; interrupted copies remain incomplete.

## Read the output

Open `START-HERE.html`. It has 113 relative links to the source ZIP, application licenses, thirteen component guides, package/catalog guidance and each binary's materials. `BINDING.json` records 95 evaluated file identities and preserves the observed import classifications without machine paths. `catalog/` retains all original archives, patches, licenses and build records. Move the entire directory together.

The source ZIP contains the committed project, not a vendored offline Cargo cache. Follow its pinned build instructions and the separate native material guides; building may require obtaining dependencies. The material hashes identify this evaluation candidate only. They do not add a startup hash allowlist or prevent compatible DLL replacement.

## Verification and remaining scope

The focused tests pass all 172 source Git blobs, exact catalog preservation, 95 real-binary bindings, 113 local links, repeat/arbitrary cwd, 102 missing/corrupt input pairs, extra files, stale source commit, overlap/junction rejection and failure after catalog copy without a completion marker. Original input hashes and the first complete output are checked again afterward. Final evidence is `target/tmp/candidate-material-test-33e7fb5f1f1a43be9be3b5f86397d3bf`; format, full-target Clippy and 270 workspace tests also pass. Three live ignores remain unexecuted and provide no new hardware evidence.

The final local `target/candidate-materials-v2` contains 2879 files/844031954 bytes and matches the test's first output file-for-file. `BINDING.json` SHA256 is `a8497a10dd1d3d32e2fe9f4a5f1feb9f9920cd55a45867d046c16e038faa5a14`; `START-HERE.html` is `45e16d008a40cc0a421667a97d1de1d41aca59cce84eec43de2029606160dce8`. Earlier v1 is retained as diagnostic output with the subsequently corrected HTML link construction; do not use it as the final guide.

The Browser skill's selected browser blocked file-URL navigation. No alternative browser or transport was used to bypass that restriction. Link/file validation is not rendering evidence; actual page rendering and application-menu access remain unverified. No application UI was changed in this checkpoint.

The specifically named native notice/data collection work is now represented in the local catalog. Keep documented historical reproduction/other-target limits, but do not reopen all permissive source as a universal rebuild gate. This directory still needs the final candidate's same-release delivery and a user-facing entry from the installed app. VC prerequisite lifecycle, assisted Setup.exe, supported-Windows installation/update/removal, final runtime quality and owner acceptance remain separate. Publication/signing and any distribution adoption are not implied by collection success.
