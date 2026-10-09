# towavue image-webp patch

Base: crates.io `image-webp` 0.2.4, archive SHA256
`525e9ff3e1a4be2fbea1fdf0e98686a6d98b4d8f937e1bf7402245af1909e8c3`.
Published source, manifests, lockfile and MIT/Apache-2.0 licenses are retained;
only Cargo's local `.cargo-ok` marker is omitted. `Cargo.toml.orig` describes
the unmodified upstream package. No external image fixtures or binaries are added.

## Delta

- `src/vp8.rs` speculates a complete coefficient block in a 16-element scratch
  array, including any existing DC coefficient. `src/vp8_arithmetic_decoder.rs`
  keeps arithmetic state uncommitted until every read in the block is valid.
  A boundary miss discards both scratch and speculative state, then executes
  the original checked coefficient loop. Partial final bytes, the existing
  single-zero-byte allowance and post-EOF error behavior remain unchanged.
- The fixed DCT token topology uses direct branches with the same contextual
  probabilities and read order. Fast bit/flag/tree helpers are inlined. The
  normalization values are unchanged. For the normalized 128..=255 input
  range, probability splitting leaves 1..=255, so byte leading-zero count
  equals the former u32 count minus 24. A half-probability flag leaves
  64..=128, requiring only zero or one shift. No refill or EOF logic changes.
- Lossy frames of at least 1,048,576 pixels with nonzero frame filter level
  may use one scoped worker for independent inverse DCTs, prediction and loop
  filtering while the caller decodes coefficients. The caller retains entropy
  contexts and the Y2 Walsh transform. Per-block masks preserve the original
  inverse-DCT conditions and nonzero flags. The worker owns the complete output
  planes and separate unfiltered predictor borders, filtering in original
  macroblock order after reconstructing each row. Three reusable coefficient
  rows (at most 4.5 MiB of coefficients plus masks and macroblock metadata)
  circulate through bounded queues. No second full frame is allocated.
  The generic reader remains on the caller thread and need not be Send.
  Worker-creation failure uses the serial path. Close the producer before joining
  on decode errors; a worker cannot outlive the synchronous decode call. Small
  frames and frames with zero filter level retain the serial scheduling path.
- `WebPDecoder::read_image_rgba` adds an explicit RGBA output path. Source-alpha
  images use the existing decoder; opaque static images write RGBA directly.
  Lossy Bilinear conversion uses the same fancy interpolation while storing
  RGB and alpha 255 together. Simple interpolation and opaque lossless output
  also force alpha 255, including a false alpha hint. Opaque animation calls
  preserve the original compositing/state path through a temporary RGB buffer;
  towavue selects the new entry point only for static images.
- Existing `read_image`, source-alpha metadata, animation, container parsing,
  error behavior and `#![forbid(unsafe_code)]` remain unchanged. No dependency
  is added. Static opaque output avoids an intermediate RGB image; this is not
  a measured whole-process peak-memory guarantee. A codec call remains
  synchronous; caller cancellation checks are not an entropy-loop interrupt.

## Checks and maintenance

```powershell
cargo test --manifest-path vendor/image-webp/Cargo.toml --lib --locked
cargo fmt --manifest-path vendor/image-webp/Cargo.toml --check
```

The added tree test compares tokens and exact probability-node read order for
all 4,096 branch patterns and starting-node combinations. The arithmetic test
compares results, errors and complete internal state across 165,120 deterministic
transactions, covering partial chunks, rollback, probability extremes and EOF.
A further 394,752 steps compare exact output bits and arithmetic state against
the original u32 normalization for every reachable range/probability, both split
branches and three reservoir positions, including the half-probability endpoint.
The row-filter tests compare full padded YUV across filter parameters, odd
dimensions, every truncation prefix and injected worker-creation failure; the
reader deliberately uses Rc to preserve the non-Send API contract.
The RGBA controls compare full pixels over alpha/lossless/interpolation and
odd-size combinations, false alpha hints, invalid lengths, bounded malformed
inputs and non-Send readers. The existing ignored optimization benchmark is
not a correctness pass.

Also run towavue's image, WebP metadata/export and animation regressions, plus
the full workspace suite when integrating this patch. Performance claims need
the exact normal Release executable and representative desktop measurements;
raw codec timing alone does not establish startup improvement.

When rebasing, preserve the checked fallback and keep its coefficient semantics
in sync with the speculative loop. Revalidate tree topology if upstream changes
`DCT_TOKEN_TREE`; do not infer it from one decoded fixture. Preserve the unsafe
prohibition and pinned dependency version.
