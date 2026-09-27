# VC runtime prerequisite review

2026-09-08: the owner confirmed individual development with Visual Studio Community. This closes the usage-category question, not distribution approval. No installer has been executed or license accepted on the owner's behalf.

## Fixed candidate

[vc-redist-inputs.json](vc-redist-inputs.json) records the official x64 package, original terms and observed toolchain. The Microsoft download and stable Community installation contain identical `vc_redist.x64.exe` files: version **14.51.36247.0**, 18,731,856 bytes, SHA256 `843068991daaa1f73ad9f6239bce4d0f6a07a51f18c37ea2a867e9beca71295c`. Both have valid Microsoft Authenticode signatures. The moving latest URL is discovery only; verification uses the fixed identity.

Use this package version as a conservative prerequisite floor. Toolset directory `14.51.36231`, linker resource `14.51.36256.0`, CRT header `14.51.36244.0` and redistributed runtime `14.51.36247.0` are distinct observations, not interchangeable version numbers. This is not a derivation of the oldest API-compatible runtime. Microsoft requires a runtime sufficiently recent for the build tools and matching the app architecture. [Latest supported runtime](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist?view=msvc-170).

## Terms and separation

Community's original terms permit individuals to develop their own applications. Distributable Code conditions remain applicable, including substantive app functionality, protective downstream terms and indemnity obligations; they do not become the app's Apache-2.0 license. The complete original DOCX text and notes were read without alteration; document rendering was unavailable because LibreOffice is absent. [Community terms](https://visualstudio.microsoft.com/license-terms/vs2026-ga-community/).

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

## Setup integration (1.0.3 onward)

The owner authorized completing prerequisite installation inside towavue Setup, with Windows UAC approval when needed, on 2026-09-27. This supersedes the earlier full-Microsoft-UI policy for future releases; the existing v1.0.2 draft/tag/assets remain unchanged.

Run the verified, unchanged package with `/install /quiet /norestart` and `RunAs`. Setup's Welcome page identifies the Microsoft component and applicable separate terms; its existing progress/details and Finish/error pages own installation feedback. Only UAC requires a separate system prompt. There is no additional prerequisite confirmation or Microsoft installer window. [Microsoft command-line options](https://learn.microsoft.com/en-us/cpp/windows/redistributing-visual-cpp-files?view=msvc-170#command-line-options-for-the-redistributable-packages).

The wrapper skips satisfied state, refuses unknown state, waits for the elevated quiet package and requires both a satisfied post-check and actual exit 0/3010. UAC rejection, cancellation, concurrent installation and other failures stop Setup before application publication, retaining diagnostic output. Result 3010 is preserved for Finish/the update helper; neither Windows nor the app restarts automatically after that result. Normal automatic updates may install a missing/older prerequisite through the same UAC-only path. Fresh silent installations and ordinary update ownership/version/recovery restrictions are unchanged. Uninstall never removes the shared runtime.

Run `scripts/test-setup-prerequisite.ps1 -NsisArchive <verified archive>` for 16 isolated wrapper cases and seven native NSIS flows. The wrapper cases substitute only the reader and child process, asserting quiet/no-restart arguments, elevation, hidden child UI, waiting and post-checks. The NSIS cases compile the unchanged production prerequisite function with an inert package and synthetic helper, covering skip, install, failure, restart and automatic-update handoff without changing the real runtime or showing windows. They do not certify actual UAC, clean-machine installation or native reboot behavior. The owner host's installed runtime remains untouched; those physical checks require a suitable separate environment.
