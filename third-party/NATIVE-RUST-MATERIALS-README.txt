Native Rust dependency materials (2026-09-08)

This is an audit/preparation bundle, not an approved release or linked SBOM.
Keep it with the native source supplements for rav1e 0.8.1 and libdovi 3.4.0.
Those contain the original library sources, build recipes and package notices.
The two unchanged source locks are copied here for checksum verification.

INPUTS.json records 129 rav1e and 28 libdovi normal/build-reachable registry
packages, their declared license expressions, original .crate archives and
selected documents. Source locks are authoritative for these archive hashes.
All archives were inventoried for safe paths and regular file/directory types.
The collector is offline, validates all inputs before output, never executes
Cargo/build scripts, and copies original bytes without text re-encoding.
Use a fresh output directory; failed extraction has no INPUTS completion marker.

Derivation uses Cargo 1.98.0 metadata, GNU target filtering and the source's
default+capi (rav1e) or all-features (libdovi) selection. The analysis host is
MSVC, unlike the original GNU build host. Metadata may also unify test
features. CLI/build-only and host-specific packages are included. This is
not a reconstructed historical unit graph or a claim that all these crates
are linked into the DLLs. Twelve/eleven printable crate paths in the actual
DLLs match this inventory, but strings cannot establish a complete closure.
Do not substitute towavue's own Cargo.lock or the excluded BtbN materials.

The original package records identify Rust 1.87.0/cargo-c 0.10.13 for rav1e
and Rust 1.97.0/cargo-c 0.10.24 for libdovi. Their standard-library/compiler
notices and original GNU-host unit selection remain separate work; current
towavue Rust 1.98.0 notices do not cover those binaries automatically.
Libdovi's recipe fetch is not --locked; its subsequent build is --frozen.
Our --locked metadata leaves the library lock unchanged and matches the
observed DLL versions, but does not prove the historical fetch made no change.

Root notice discovery is supplemented by original nested notices, including
av-metrics color conversion, libgit2/libz build sources, crc-catalog LICENSES
and Unicode texts. Root Cargo license declarations do not replace additional
source conditions. Build-only libgit2 material includes its own exceptions
and bundled GPL/LGPL text; those texts are not assigned to every runtime DLL.
Further individual source/generated-data scope review remains necessary.

profiling and profiling-procmacros omit root license files from their crates.
Their .cargo_vcs_info.json identifies the exact shared upstream commit used
for the original MIT/Apache texts. av-metrics has no VCS metadata: its original
manifest and all thirteen source blobs match the recorded upstream tree,
whose MIT notice is retained along with the two nested licenses.

simd_helpers declares MIT but contains no original license file, and the
recorded upstream tree has none either. MIT-standard.txt is the unmodified
SPDX text corresponding to that declaration, not a notice issued by the
crate author. Its placeholders stay unchanged. The original Cargo.toml.orig
identifies Luca Barbato as author; no copyright year or grant is invented.
Its procedural-macro source is retained for attribution/scope inspection.

Alternative license expressions remain alternatives, not AND obligations.
These materials do not establish patent/trademark clearance, full binary
reproduction, distribution approval or completion of the installer gates.
