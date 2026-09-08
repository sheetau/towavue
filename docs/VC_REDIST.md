# VC runtime prerequisite review

2026-09-08: the owner confirmed individual development with Visual Studio Community. This closes the usage-category question, not distribution approval. No installer has been executed or license accepted on the owner's behalf.

## Fixed candidate

[vc-redist-inputs.json](vc-redist-inputs.json) records the official x64 package, original terms and observed toolchain. The Microsoft download and stable Community installation contain identical `vc_redist.x64.exe` files: version **14.51.36247.0**, 18,731,856 bytes, SHA256 `843068991daaa1f73ad9f6239bce4d0f6a07a51f18c37ea2a867e9beca71295c`. Both have valid Microsoft Authenticode signatures. The moving latest URL is discovery only; verification uses the fixed identity.

Use this package version as a conservative prerequisite floor. Toolset directory `14.51.36231`, linker resource `14.51.36256.0`, CRT header `14.51.36244.0` and redistributed runtime `14.51.36247.0` are distinct observations, not interchangeable version numbers. This is not a derivation of the oldest API-compatible runtime. Microsoft requires a runtime sufficiently recent for the build tools and matching the app architecture. [Latest supported runtime](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170).

## Terms and separation

Community's original terms permit individuals to develop their own applications. Distributable Code conditions remain applicable, including substantive app functionality, protective downstream terms and indemnity obligations; they do not become the app's MIT OR Apache-2.0 license. The complete original DOCX text and notes were read without alteration; document rendering was unavailable because LibreOffice is absent. [Community terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-community/).

The stable Visual Studio REDIST list includes the unmodified VC redistributable for properly licensed Community users; preview software is excluded. Keep Microsoft components and terms separate, and do not imply Microsoft endorsement. This review is conditional, not legal clearance for every distribution circumstance. [Visual Studio 2026 REDIST list](https://learn.microsoft.com/en-us/visualstudio/releases/2026/redistribution).

## Read-only verification

```powershell
.\scripts\get-vc-redist-status.ps1
.\scripts\get-vc-redist-status.ps1 -PackagePath 'path/to/vc_redist.x64.exe'
.\scripts\test-vc-redist-status.ps1 -PackagePath 'path/to/vc_redist.x64.exe'
```

The reader never downloads, launches or installs a package. An explicit package must match bytes, hash, version and valid Microsoft signature. It opens HKLM's `SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64` in **Registry64**, including from 32-bit PowerShell. It checks the installed flag, numeric version fields and matching four-part version string.

| Registration | Decision |
|---|---|
| Absent, explicitly uninstalled, or older v14 | Installation required |
| Same or newer v14 | Skip prerequisite |
| Malformed, contradictory, or another ABI major | Stop and diagnose |

Tests cover 26 snapshots, unsupported minimum ABI, missing/corrupt packages, input preservation and unchanged live inspection. Without `PackagePath`, package/signature tests explicitly skip; CI exercises that mode. Both 64-bit and 32-bit PowerShell observe the development host's installed `14.51.36247.0`. Registration does not prove DLL health or clean-machine compatibility.

## Planned installer behavior — not implemented or tested

Use the unchanged package's full UI with `/install /norestart` when needed, allowing the user to review and accept its terms. No `/quiet`, `/passive`, implicit assent or forced restart. Recheck the registry after installation; never remove the shared runtime when uninstalling towavue. [Microsoft deployment guidance](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170).

For future isolated tests, success must also pass the post-check; reboot-required success must retain that state and avoid automatic app launch. A different-version result requires a compatible-registration recheck, not unconditional success. Cancellation, concurrent installation and other failures must stop with diagnostics. Windows Installer documents codes 0, 3010, 1638, 1602 and 1618, but the VC bootstrapper's actual return values, HRESULT wrapping, UAC cancellation and restart behavior still need observation. [MSI error codes](https://learn.microsoft.com/en-us/windows/win32/msi/error-codes).

Remaining gates: native material review and final source/notice delivery, prerequisite/end-user terms integration, assisted Setup.exe, isolated supported-Windows installation/update/removal, and existing H1 quality/owner acceptance. No binary is committed or added to a distribution kit by these scripts.
