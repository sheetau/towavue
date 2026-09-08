# Assisted installer lifecycle fixture

This is a local safety slice, **not the towavue application installer**. It contains three harmless documents, an ownership record and an uninstaller. It does not contain the app, FFmpeg or the VC redistributable, register an application, create shortcuts, change associations, update an existing installation or publish anything. The compiler rejects invocation without the fixture-only define.

## Build and focused verification

Obtain the portable `nsis-3.12.zip` from the fixed URL in [nsis-inputs.json](nsis-inputs.json), then run:

```powershell
.\scripts\test-setup-fixture.ps1 -NsisArchive 'path/to/nsis-3.12.zip'
```

The script checks bytes/SHA256 before extraction or execution, checks archive paths and version, compiles with `/NOCONFIG /WX`, and tests only fresh `target/tmp/setup-fixture-<id>` directories. It does not download or install the compiler. CI downloads the same pinned archive in a separate Windows Server 2022 fixture job; that is not Windows 10/11 compatibility evidence. No generated executable or toolchain goes into Git.

Use `curl.exe --fail --location` for this SourceForge URL. The initial CI download failed the identity check; a local `Invoke-WebRequest` reproduction saved an HTML page instead of the ZIP. Keep the original hash check, never accept that response or update the pin to match it. CI records actual download size/hash before verification.

The official project lists 3.12 as the released version. The existing local Electron-builder cache reports 3.04; it is not used. The archive digest is locally measured, not an independently authenticated publisher checksum or a reproducible-compiler claim. The generated executable uses Unicode and zlib with the standard System plug-in; the original package `COPYING` is retained in its documents. Other compression choices and third-party plug-ins are not introduced. [Official download](https://nsis.sourceforge.io/Download), [compiler options](https://nsis.sourceforge.io/Docs/Chapter3.html), [license reference](https://nsis.sourceforge.io/Docs/AppendixI.html).

## Implemented boundary

- Normal user execution, Welcome, editable destination, progress, Finish and uninstall confirmation. The default is an owned scratch location, not a real application installation directory.
- Reject an occupied directory, drive root, UNC/device path, existing file in the ancestor chain, or reparse point in that chain. Recheck before extraction, including silent fixture tests. Existing empty directories and new nested directories are supported.
- Preserve an explicit `/D` argument from the original command line. NSIS can otherwise replace an invalid destination with the default before `.onInit`; a rejected choice must not silently install elsewhere.
- Canonicalize nonexistent directories with the Windows API, normalize a trailing separator and preserve Japanese names with a UTF-16LE INI marker. The marker binds this fixture and the actual destination, not just a familiar filename.
- Check root/nested directory and marker before deleting. Remove only the five explicit owned files, then remove directories only when empty. Extra files at either level remain. No wildcard, recursive removal, scheduled reboot deletion, shared runtime removal or settings cleanup.
- Report incomplete installation/deletion as failure. A locked payload retains the uninstaller/marker so removal can be retried. A failure to remove the last marker is also a failure, not reported success. No automatic rollback or power-loss atomicity claim.

These are accidental-damage guards, not authentication or protection against hostile concurrent filesystem changes. Final app updates, replaced DLL ownership and registered shortcuts need their own lifecycle design and tests. The NSIS documentation explicitly warns against recursive removal of a user-selected installation directory. [Scripting reference](https://nsis.sourceforge.io/Docs/Chapter4.html#rmdir).

## Observed tests and limitations

The focused suite checks exact installed bytes, Japanese/space/ampersand/dollar paths, empty/new nested/trailing-separator paths, occupied folders, ancestor files, junction refusal, missing/corrupt/moved markers, locked-file failure/retry, preservation of extra user files, empty-directory removal and input preservation. A read-only `/CHECKONLY /CHECKPATH=...` exits before target writes for broad-path tests. The uninstaller driver is copied to trial scratch and uses NSIS `_?=` to keep one waited process; the normal self-copy parent/child lifecycle is not yet covered.

Initial tests exposed nonexistent-path handling, NSIS invalid-destination fallback and ANSI loss in the Japanese marker; all now have regression coverage. Native hidden-window controls were inspected through owned PID/start-time/window handles: Welcome, destination, Cancel/No/Yes and no destination creation on cancellation. Rendering that hidden window returned a blank image, so this is **not visual QA** or screen-reader acceptance. No browser or HTML document was opened.

Next: bind the final application/source/notices, integrate the prerequisite's original user-consent UI and per-user application registration, and validate actual Setup installation/update/removal in isolated supported Windows. Keep [VC runtime gates](VC_REDIST.md), [candidate material bindings](CANDIDATE_MATERIALS.md), final playback quality and owner acceptance separate. This fixture does not approve runtime distribution or satisfy the launch gate.
