# towavue png patch

Base: crates.io `png` 0.18.1, archive SHA256
`60769b8b31b2a9f263dae2776c37b1b28ae246943cf719eb6946a1db05128a61`.
The published source, manifests, lockfile and MIT/Apache-2.0 licenses are retained;
only Cargo's local `.cargo-ok` marker is omitted. `Cargo.toml.orig` describes the
unmodified upstream package. No downloaded image fixtures or binaries are added.

## Delta

- The optional `towavue-row-filter` feature exposes a safe function-pointer
  override through `Reader::set_row_filter`. Both in-place and scratch-buffer
  reconstruction invoke it after validating the filter byte. Returning false
  without modifying the current row retains the existing implementation.
- Parser, limits, CRC/tail checks, inflate history, row buffers, transformations,
  frame/interlace resets and `#![forbid(unsafe_code)]` are unchanged. No callback
  is installed by default; other png readers retain the standard path. Callers
  of towavue's shared static decoder inherit its explicit override.
- towavue installs the callback only in its static PNG decoder. The x64 SSE2
  implementation stays in the Windows runtime behind safe, equal-length row
  slices, handling Paeth with four-byte stride and a previous row. All other
  cases use the existing filter. It adds no image-sized buffer.

Generated internal checks exercise both buffer paths, false-return fallback,
true-return override, row resets and invalid-filter rejection before dispatch:

```powershell
cargo test --manifest-path vendor/png/Cargo.toml --lib row_filter_tests --features towavue-row-filter --locked
cargo test --manifest-path vendor/png/Cargo.toml --lib filter:: --features towavue-row-filter --locked
```

The packaged crate omits upstream `tests/pngsuite` and other fixture directories;
do not report glob-based tests with zero files as corpus verification. Use
towavue's generated PNG/Adam7, depth/orientation, metadata, cancellation, budget,
tail/CRC and export regressions as well. Performance/handoff evidence belongs in
`docs/STATUS.md`, not here. Rebase this small interface extension before upgrading
png; do not weaken the upstream unsafe prohibition to move SIMD into this crate.
