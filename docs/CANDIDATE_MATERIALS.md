# Evaluated candidate: source and notice binding

The collector joins the evaluated application's source snapshot and the fixed thirteen-kit catalog into one movable offline directory. It checks the actual executable and all 94 runtime files, but copies only source/notice materials. This is not Setup.exe, runtime adoption, public delivery or a complete linked SBOM.

The current binding describes the [Help/palette material entry](DEVELOPMENT.md) build 195af870. The earlier app kit v1, catalog v10 and candidate materials v2 remain historical records for executable 03125262; they are not relabeled as the new build's sources. The installed-app path contract is `licenses/START-HERE.html`, selected in Explorer. It does not require every source archive to be installed locally.

## Fixed inputs

[candidate-material-inputs.json](candidate-material-inputs.json) pins catalog v11's inventory/completion marker and the application source ZIP. Candidate binary identities are read from the hash-checked catalog's existing app and FFmpeg manifests, not another handwritten list.

- Application: 10301952 bytes, SHA256 `195af8705605e678a9cbe97aa57a0cdb7cae5e34bf61ee6b60e528b933f93186`. Source snapshot `06588b672f0394fb7ac2c1832242c32f7688ca36` is the Help-entry checkpoint. This binding describes that recorded executable, not the later cache/configuration resilience changes in the current application source. Cargo manifests/lock and toolchain are unchanged. The association uses the recorded normal release build and source-tree comparison, not a bit-identical rebuild claim.
- Application source ZIP: 1290025 bytes, SHA256 `2eb46e64e6c858fc09b8370e98683db57c0a70c1989d91136265eb03a479d2a3`. All 224 regular members are compared against the commit's Git blob identities; missing, duplicate or changed members fail. Two Git archives match exactly. The collector checks the pinned ZIP bytes and Git commit comment. No working-tree or untracked files enter the archive. This is application source, not the subsequently developed installer.
- Runtime: 84 DLLs match the original package audit by name, size and hash; nine rebuilt FFmpeg files point to their source/build kit; scoped ZVBI points to its own patched-source kit, not the old package. Observed runtime import edges must resolve within the 94-file set. This does not re-inspect PE headers or prove every dynamic load or static dependency.
- Catalog: all 2875 original files, including its inventory/completion marker, must match. Twelve native kits remain unchanged; app kit v2 changes only its candidate INPUTS/EVIDENCE, retaining nine original material files byte-for-byte. Historical scope statements within the kits are preserved; the new binding is the current candidate list.

## Prepare

From the repository, create the pinned application archive in a fresh local directory:

```powershell
git -c core.autocrlf=false -c core.eol=lf archive --format=zip --prefix=towavue/ --output=path/to/towavue-source-06588b6.zip 06588b672f0394fb7ac2c1832242c32f7688ca36
```

Use a runtime input directory containing exactly the 94 names in `native-ffmpeg-materials-v1/RUNTIME.json`. A native build's full `prefix/bin` also has seven link libraries and ffplay.exe: it is not the selected runtime set. Copy the selected original files to a fresh staging directory and keep the build prefix unchanged. Do not add OS DLLs or the VC redistributable here.

```powershell
.\scripts\prepare-candidate-materials.ps1 -Executable 'path/to/towavue.exe' -RuntimeDirectory 'path/to/selected-runtime' -CatalogDirectory 'target/native-material-catalog-v11' -ApplicationSource 'path/to/towavue-source-06588b6.zip' -OutputDirectory 'target/candidate-materials-v3'
.\scripts\test-candidate-materials.ps1 -Executable 'path/to/towavue.exe' -RuntimeDirectory 'path/to/selected-runtime' -CatalogDirectory 'target/native-material-catalog-v11' -ApplicationSource 'path/to/towavue-source-06588b6.zip'
```

Both scripts work from another current directory when called by absolute path. Output must be fresh and separate from inputs. A bad input never triggers download, repair, build or installation. The full catalog is validated before output, and each copied file is checked again. `INPUTS.json` is the final completion marker; interrupted copies remain incomplete.

## Read the output

Open `START-HERE.html`. It has 113 relative links to the source ZIP, application licenses, thirteen component guides, package/catalog guidance and each binary's materials. `BINDING.json` records 95 evaluated file identities and preserves the observed import classifications without machine paths. `catalog/` retains all original archives, patches, licenses and build records. Move the entire directory together.

The source ZIP contains the committed project, not a vendored offline Cargo cache. Follow its pinned build instructions and the separate native material guides; building may require obtaining dependencies. The material hashes identify this evaluation candidate only. They do not add a startup hash allowlist or prevent compatible DLL replacement.

## Verification and remaining scope

The focused tests pass all 224 source Git blobs, exact catalog preservation, 95 real-binary bindings, 113 local links, repeat/arbitrary cwd, 102 missing/corrupt input pairs, extra files, stale source commit, overlap/junction rejection and failure after catalog copy without a completion marker. Original input hashes and the first complete output are checked again afterward. Evidence is `target/tmp/candidate-material-test-bc867c58377743f2961149c4a247f456`. The refreshed app-kit and catalog suites also pass. Three live ignores remain unexecuted and provide no new hardware evidence.

The local `target/candidate-materials-v3` contains 2879 files/844235795 bytes and matches the complete test output and actual-app trial copy file-for-file. `BINDING.json` SHA256 is `c68cad3486e730fafdcc32ce79936a85f2d65c4206c2dd87ac4067252541569a`; `START-HERE.html` is `cc6278f7648b0c1b9b34137b3b536a24e94da49ba2f58301e32e4f4feb355266`. Older v1 is diagnostic output with a corrected-later HTML link issue; v2 is the valid older-application binding. Neither replaces v3.

The Browser skill's selected browser blocked file-URL navigation. No alternative browser or transport was used to bypass that restriction. Link/file validation is not rendering evidence; actual page rendering remains unverified. The real v3 tree was copied to the exe-adjacent `licenses` directory of a separate normal-app trial without FFMPEG_DIR/development PATH. Help selected the exact actual guide in Explorer. The initial one-call UI helper missed menu visibility within five seconds; subsequent reads found it open, and the already-open Help path completed. This is file-placement/selection evidence, not HTML execution, screen-reader latency acceptance or Setup installation. The app and exact owned Explorer folder closed normally.

The specifically named native notice/data collection work is represented in the local catalog and the Help-entry app/source binding is refreshed. Keep historical reproduction/other-target limits, but do not reopen all permissive source as a universal rebuild gate. Same-release delivery and actual installer placement remain necessary. VC prerequisite lifecycle, actual Setup.exe, supported-Windows installation/update/removal, final runtime quality and owner acceptance remain separate. Publication/signing and distribution adoption are not implied by collection success.

## Separate development preparation after the resilience fixes

On 2026-09-09, prepare a separate, unqualified development set at `target/tmp/candidate-source-c277ceb-20260909`; do not replace the pins above. Executable `f91b4498172f71b734cb4a0c564c213d45a7696e9c27707d1769f4be8a064265` (10319872 bytes) contains both resilience fixes. Its recorded build source matches commit `c277ceb9513c3842163e499c2a89cb109d988a42`, including unchanged Cargo/toolchain inputs.

`towavue-source-c277ceb.zip` is 1416448 bytes/SHA256 `ca58895273df30b126ef5a11af8e1534a5bd921b82cbc4dc8f2abc83dca96f92`. All 251 regular members equal their committed Git blobs; a second Git archive is byte-identical. This full repository snapshot also contains the current installer sources, but is not a rebuilt native-source companion or proof that an existing Setup delivers them. It does not contain a vendored Cargo cache.

The separate `viewer space & tools` directory contains that app plus the exact 94 selected runtime files. Reinspect all 95 PE files: every runtime hash and imported-module classification matches the existing runtime inventory. Adjacent ffmpeg/ffprobe generate and inspect a 32x16 PNG from an unrelated cwd with FFMPEG_DIR absent and PATH limited to Windows System32. This proves the helper probe on this host, not normal-app startup, arbitrary dynamic loads, supported-Windows qualification or VC installation. No app window, installer, browser or media playback is launched by this preparation.

`REVIEW.json` (8034 bytes/SHA256 `afa3212dbdfbc084b12ae14ab8caeab778df552e7b35aef9bb7cd43635dda35b`) records the source/runtime checks with `distribution_approved=false`. The actual Setup builder rejects this new exe with the old companion before creating output. Old app/Setup/companion hashes remain unchanged. Qualify the new normal window, warning/focus, playback/export and performance before adopting/rebinding the final app kit, catalog, companion and Setup. Do not copy the old guide beside this exe and imply matching materials.

## Portable source companion

Package a directory only after the candidate checks above pass:

```powershell
.\scripts\pack-candidate-materials.ps1 -MaterialsDirectory 'target/candidate-materials-v3' -ArchivePath 'path/to/new-sources.zip'
.\scripts\test-candidate-material-archive.ps1
```

The packer verifies lossless archival, not candidate legitimacy or license approval. It rejects missing completion files, existing/overlapping destinations and reparse points. It names ZIP entries with forward slashes in ordinal order, includes hidden files and uses fixed wrapper timestamps; all decompressed sizes/hashes and original inputs are checked before the final filename appears. A failure retains a distinctly named partial file, never an apparently complete archive. Compression bytes are not promised identical across different .NET implementations.

The local companion `target/distribution/towavue-sources-195af870-c68cad34.zip` has 2879 files, 509515254 bytes and SHA256 `1fa92321d044ffa380a76caf342a33a4507ca80aca0f620740b75859a02b5d20`. Every entry matches v3, including the 224-file application source ZIP and all native originals. A complete repeat with the final packer has the same SHA256. The initial direct legacy .NET CreateFromDirectory trial emitted 2875 backslash paths and was rejected; that `.partial` is diagnostic, not the companion. Cold PowerShell also required explicitly loading System.IO.Compression; the standalone test covers it.

This approximately 510 MB companion is separate from the application's approximately 1.3 MB source ZIP. It is a local app/native material artifact, not a published release, an installer or a requirement to install 844 MB of source files. The final Setup must still provide the local notices and matching same-release companion access, plus its own installer/prerequisite materials.
