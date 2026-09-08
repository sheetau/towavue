towavue application license and Rust notice materials
INCOMPLETE REVIEW MATERIALS - NOT AN APPROVED RELEASE

LICENSE-MIT and LICENSE-APACHE are towavue's original license texts. The
application is offered under MIT OR Apache-2.0; retaining both texts does not
convert that choice into an AND requirement.

RUST-THIRD-PARTY-NOTICES.txt retains the current Cargo.lock's 146 normal/build
dependency entries for Windows x86-64, with original terms and attribution.
It includes the four embedded epaint_default_fonts notice files (Hack, OFL,
UFL and emoji-icon-font) and the recorded inline/Unicode/upstream supplements.
It is not a claim that every build dependency is linked into the executable.
Japanese fallback fonts are read from Windows at runtime and are not bundled
or downloaded by towavue. No Windows font files are included in these materials.

TOWAVUE-RUST-RUNTIME-NOTICES.zip contains only Rust 1.98.0 for
x86_64-pc-windows-msvc: eighteen original standard-library, license-dictionary
and compiler-builtins/libm documents, plus README.txt and source INPUTS.json.
Open that ZIP's README first. Original AND/OR and exception terms are retained.
The complete SPDX text dictionary does not mean every listed license applies.
The excluded BtbN FFmpeg build's Rust 1.97.1 GNU documents are not included.
Current native rav1e/libdovi Rust materials are in separate native catalog kits.

Cargo.lock, rust-toolchain.toml and docs/rust-license-inputs.json bind the
dependency notices to their exact inputs. INPUTS.json records the candidate
executable's identity and material hashes; the executable itself is not copied.
EVIDENCE.json is written only after validating the exact files and selected
runtime ZIP members. Collection does not establish a reproducible binary or
prove all historical/native/static inputs. The source project is maintained at
https://github.com/sheetau/towavue; this kit is not a newly published release.

Regenerate the two notice inputs with the repository scripts before collecting:
  scripts/prepare-rust-notices.ps1
  scripts/prepare-rust-runtime-notices.ps1 -Scope towavue
The first requires the pinned Cargo cache and recorded supplemental originals.
The second reads the matching official rustc/rust-src archives; -Download is
explicit and optional. The app collector itself is offline and never builds,
installs, repairs inputs or copies any compiler/DLL/executable/font binary.

The native source/notice catalog supplies FFmpeg, its source dependencies,
scoped ZVBI and separate native toolchain/runtime materials. Individual remaining
native scope, Microsoft Visual C++ redistribution/prerequisites, final approved
source delivery, candidate quality and the assisted Setup.exe/target-Windows
lifecycle and owner acceptance remain separate gates. Rust's MSVC target name
does not provide the Microsoft Visual C++ redistributable or its license.
