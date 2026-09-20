towavue application licenses and Rust notices

For towavue 1.0.3 and later, LICENSE-APACHE and NOTICE apply to the application.
Earlier versions retain their MIT OR Apache-2.0 terms; their application kit
contains both license texts. Third-party components retain their own terms
and original notices, independently of the application's license selection.

RUST-THIRD-PARTY-NOTICES.txt contains the exact Windows x64 normal/build
dependency closure recorded in docs/rust-license-inputs.json and Cargo.lock,
including original font, inline, Unicode and upstream supplemental notices.
INPUTS.json and EVIDENCE.json record the package count and file identities.
Build dependencies are not necessarily linked into the executable. Windows
fallback fonts are read at runtime and are not copied or downloaded here.

TOWAVUE-RUST-RUNTIME-NOTICES.zip contains the selected MSVC Rust standard
library, license dictionary and compiler-builtins/libm originals. Read its
README.txt first. The full SPDX dictionary does not mean every license applies.
Rust inside native rav1e/libdovi and the native compiler/runtime materials are
separate catalog kits. No excluded BtbN GNU runtime materials are selected.

The companion's application source ZIP is a committed Git snapshot containing
Cargo.lock, the pinned toolchain, patched dependencies and build instructions.
Obtain ordinary Cargo dependencies from their recorded sources; this is not
an offline Cargo cache or a claim of bit-identical executable reproduction.
The separate native kits retain sources, patches and build inputs. Compatible
FFmpeg DLL replacement is not blocked by a startup hash allowlist.

Source and matching Setup are delivered together under the versioned release:
https://github.com/sheetau/towavue/releases
Draft assets become public when the owner publishes the release. BINDING.json
identifies the exact executable, runtime and application source commit.

To regenerate these materials, use scripts/prepare-release-materials.ps1 with
the selected native material directory and the matching built executable.
The collector checks originals and never installs, signs or publishes them.
The Microsoft Visual C++ prerequisite is separate and retains its own terms;
Rust's MSVC target name does not grant Microsoft redistribution permission.
