# Optional FFmpeg preroll extension

`metadata-preroll.patch` applies to FFmpeg commit
`e47273f4d9227152dcbf543cebaf9e2430ddbcc4`. The manifest pins the upstream
archive, patch and all ten before/after source hashes. Existing upstream
license headers remain in place. Retain the upstream license files with the
patched checkout. This directory contains source changes only.

The extension reduces D3D11 pixel reconstruction for eligible HEVC and AV1
preroll pictures while retaining normal parsing, output order, packet timing
and reference bookkeeping. It advertises codec mask 5 through the versioned
`avcodec_towavue_preroll_v1_*` API. H.264 and VP9 keep ordinary decoding.
The Rust runtime discovers exports on its actual loaded codec module;
unmodified FFmpeg remains supported and uses the ordinary path.

Apply only to a fresh extraction of the pinned source before configuring it:

```powershell
.\scripts\apply-ffmpeg-preroll.ps1 -SourceDirectory <extracted-source> -MsysRoot <pinned-msys-root> -ValidateOnly
.\scripts\apply-ffmpeg-preroll.ps1 -SourceDirectory <extracted-source> -MsysRoot <pinned-msys-root>
```

The helper checks every original file, dry-runs with zero fuzz, applies once,
and verifies every resulting hash. It refuses changed and already-patched
inputs. It does not configure, compile, install or select native libraries.
For a complete build, the ordinary native builder has an explicit
`-SeekOptimized` profile. Supply the same pinned MSYS environment and six
source-built dependency prefixes as the verified native build, including
the limited ZVBI prefix. The output directory must be new and use an ASCII
path without spaces.

```powershell
.\scripts\build-ffmpeg-native.ps1 -SeekOptimized `
    -MsysRoot $msysRoot -BuildDirectory $buildDirectory `
    -Aribb24Prefix $aribb24Prefix -LcevcPrefix $lcevcPrefix `
    -LibristPrefix $libristPrefix -Uavs3dPrefix $uavs3dPrefix `
    -VvencPrefix $vvencPrefix -ZvbiPrefix $zvbiPrefix
```

This profile applies the patch before configuration, retains all configured
features, then selects the qualified static-preferred external link policy.
FFmpeg's own libraries remain shared. `aribcaption`, `hwy`, `zmq` and
`placebo` retain shared linkage because of their installed consumer contracts
or symbol/export conflicts. Required static-consumer declarations, C++/OpenMP
archives, archive groups and the Windows AVRT import library are explicit.
`static-runtime.json` pins the expected 29-module closure and import edges;
the builder verifies and stages only its 19 external DLLs. It records every
selected archive hash in `static-link.json`. No package, feature or installed
header is modified. Without the switch, the original native profile remains.

After a successful build, compare the isolated helpers and feature lists:

```powershell
.\scripts\test-ffmpeg-native-build.ps1 -BuildDirectory $buildDirectory `
    -MsysRoot $msysRoot -ReferenceExecutable $referenceFfmpeg
```

Select the resulting prefix through `FFMPEG_DIR` and its `bin` on the process
PATH when building or running the app. Keep one coherent native/helper
closure; do not mix DLLs from different prefixes. Build output and its
provenance remain local, and the profile does not approve distribution.

For correctness, run the runtime's explicit capability control with mask 5
on the patched library and mask 0 on stock FFmpeg. Run the HEVC/AV1 preroll,
continuation, cancellation, held-frame/reuse, device and fallback controls
documented in [DEVELOPMENT](../../docs/DEVELOPMENT.md). A matching source hash or successful
build is not a content or performance test. Staging, static dependency
selection and distribution approval are separate from this patch.
