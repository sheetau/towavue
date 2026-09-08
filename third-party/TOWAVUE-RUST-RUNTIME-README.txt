towavue - Rust 1.98.0 MSVC runtime notice materials

Only the x86_64-pc-windows-msvc toolchain used for towavue is included.
The excluded BtbN FFmpeg build's Rust 1.97.1 GNU materials are not included.
Rust inside the current native rav1e/libdovi libraries is documented in the
separate native Rust runtime kit, not by substituting this MSVC toolchain.

The eighteen original documents include COPYRIGHT-library.html, COPYRIGHT,
LICENSE-APACHE, LICENSE-MIT, the release's full SPDX text dictionary and the
matching rust-src compiler-builtins/libm texts. Original AND/OR alternatives
and the LLVM exception are preserved. The SPDX dictionary and cross-platform
standard-library report are not a claim that every listed license or component
applies to the Windows executable. These are notice materials, not a linked
component SBOM or proof of complete native/static-input coverage.

INPUTS.json contains only the two matching official rustc/rust-src archive
records, including release-manifest URLs/hashes and exact notice member hashes.
No compiler binaries, libraries, downloaded Windows fonts or MSVC redistributable
are included. The Rust Cargo dependency/font notices, towavue's own licenses,
FFmpeg/native materials and Microsoft runtime prerequisites remain separate.
This ZIP is not a public source offer, installer or distribution approval.
