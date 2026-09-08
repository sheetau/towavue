# Local application Setup evaluation

This builds an **unpublished evaluation installer**, not a release. It contains the recorded normal app, its exact 94-file media runtime, local original notices, a path-bound uninstaller and the original Microsoft prerequisite package. Nothing is installed by the build or inspection scripts. Registration, shortcuts, updates and supported-Windows lifecycle qualification remain pending.

## Build and inspect

Use the fixed candidate inputs described in [candidate materials](CANDIDATE_MATERIALS.md), [source companion pins](setup-inputs.json), [NSIS inputs](nsis-inputs.json) and [VC prerequisite review](VC_REDIST.md):

```powershell
.\scripts\build-local-setup.ps1 `
  -Executable 'path/to/towavue.exe' `
  -RuntimeDirectory 'path/to/exact-94-file-runtime' `
  -SourceCompanion 'path/to/towavue-sources-195af870-c68cad34.zip' `
  -VcRedist 'path/to/vc_redist.x64.exe' `
  -NsisArchive 'path/to/nsis-3.12.zip' `
  -OutputDirectory 'path/to/fresh-setup-build'

.\scripts\test-local-setup.ps1 `
  -BuildDirectory 'path/to/fresh-setup-build' `
  -SourceCompanion 'path/to/towavue-sources-195af870-c68cad34.zip' `
  -NsisArchive 'path/to/nsis-3.12.zip'

.\scripts\test-setup-prerequisite.ps1
.\scripts\test-setup-fixture.ps1 -NsisArchive 'path/to/nsis-3.12.zip'
```

The builder checks pinned archives, Microsoft package identity/signature and all 95 actual binary bindings before staging. It uses the original portable compiler with `/NOCONFIG /WX`; it does not download, install a toolchain, sign or publish. `BUILD.json` is written only after compilation and final input/staging checks. A directory or executable without that record is incomplete, not a deliverable. Retain failed output for inspection and use a fresh directory for another build.

The current app is 10,301,952 bytes/SHA256 `195af8705605e678a9cbe97aa57a0cdb7cae5e34bf61ee6b60e528b933f93186`. It has not been rebuilt for the installer. The source companion remains 509,515,254 bytes/SHA256 `1fa92321d044ffa380a76caf342a33a4507ca80aca0f620740b75859a02b5d20`. That archive binds app/native sources, not these later installer sources. Installer-source delivery is still a release gate.

## Implemented behavior

- Unicode/zlib, normal-user execution, Welcome, editable destination, progress and Finish. The default is the current user's `Programs/towavue-evaluation` under LocalAppData; a writable empty dedicated local directory can be selected. No automatic application launch or reboot.
- Application mode requires native AMD64 and OS build 19045 or later. This check does not qualify an OS or hardware configuration. Silent application installation is refused; silent fixture tests remain supported.
- Shared path checks reject roots, relative/drive-relative paths, UNC/device paths, occupied folders, files in the ancestor chain and reparse points. The app also checks the longest generated payload path against the traditional Windows path limit. Check again after prerequisite interaction, before target writes.
- Generate explicit installation/deletion lists from the verified staging inventory, not recursive extraction/deletion or a second handwritten runtime list. Check every owned directory and payload-bound marker before removing anything. Delete only owned names; remove directories only when empty. Extra user files and media/settings outside the installation are not removed. A locked-file failure retains retry state; no reboot deletion or automatic rollback.
- Run the existing strict Registry64 prerequisite reader through Windows PowerShell's absolute system path, including under 32-bit NSIS. `ExecutionPolicy Bypass` is process-scoped for the bundled reviewed scripts, not a machine setting change. Unknown state stops. Satisfied state skips installation.
- When required, ask before opening the unchanged Microsoft package's full UI with `/install /norestart` and normal UAC elevation. The user controls Microsoft terms acceptance. Wait for that process and recheck registration; only actual exit 0 or 3010 with satisfied registration proceeds. Other codes, including a different-version result, conservatively stop. Preserve 3010 in the Setup exit result and explain manual restart; never restart or launch towavue automatically. Never remove the shared runtime.

The wrapper uses PowerShell `Start-Process -Verb RunAs` to retain the actual child result rather than assuming that a waited shell launch exposes an exit code. Synthetic tests are not evidence of Microsoft's actual UAC, cancellation, HRESULT or reboot behavior. [NSIS execution reference](https://nsis.sourceforge.io/Docs/Chapter4.html#execshellwait).

## Installed notices and separate sources

The 2,610 non-source-archive originals include all retained text, headers, data/build records and the Rust runtime notice ZIP. They keep their exact bytes under short numbered names in `licenses/companion-records`; the guide and `INSTALLED-FILES.json` map each name to its full original companion path. Short names avoid imposing historical source directory depths on the install location. No license text is edited.

`licenses/START-HERE.html` offers app licenses, 13 component guides and an expandable complete original-file list. It identifies the separate companion by filename, bytes and hash, explicitly states that this local evaluation has no public download URL, and explains that original READMEs/inventories refer to the complete extracted companion. Original catalog `FILES.json` is **not** the installed inventory. The full 844 MB source tree is not installed; the matching companion must be delivered alongside any future public Setup.

Help selects this guide in Explorer. The build/test tools do not open HTML, launch a browser or claim rendered visual QA. No public source URL is invented.

## Verification and open gates

Inspection compares all 95 staged binaries, every selected original against the fixed companion ZIP, all guide links, the generated install/delete lists and build-source hashes. It exercises eight invalid input/output cases, five non-installing Setup probes and the packaged prerequisite's read-only inspection in 32-bit and 64-bit Windows PowerShell. It does not execute the real install section.

Sixteen child-process synthetic cases cover skip/required/unknown, original launch arguments, waiting, success, 3010, failed post-checks, cancellation, busy/different-version/wrapped failure and simulated UAC/reader failure. They use the unchanged wrapper, a private fake reader/launcher and a nonexistent fake package. CI runs these and the harmless shared lifecycle fixture, not the real application installer.

The empty-working-directory probe exposed a shared safety bug: NSIS stores a drive-root assignment as `C:`, and Windows then resolves it against the process working directory. Previous occupied-cwd tests happened to reject the result. Reject roots and drive-relative input **before** resolution; the fixture now tests empty cwd too. No actual root installation or deletion was attempted.

Remaining: isolated Windows 10/11 actual install/prerequisite/self-copy uninstall, failure recovery, application registration/shortcuts/update design and tests, actual guide rendering, same-release installer/source delivery, final playback/physical/owner acceptance, runtime adoption and publication approval. This evaluation is not a completed or recommended end-user installer.
