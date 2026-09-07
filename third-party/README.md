# Third-party license material

`licenses/WTFPL.txt` is the unmodified SPDX standard license text from
`spdx/license-list-data@3ac5a9c241d97f95b22a5e366c9c841404a35639/text/WTFPL.txt`.
Its SHA256 and Git blob identity are recorded in `docs/rust-license-inputs.json`.

ffmpeg-sys-next 9.0.0 declares `WTFPL` in its published Cargo manifest but does not
include a separate license file. The notice generator labels this standard text
as a declaration-based fallback, not as a file recovered from that project.
The license document's author is not identified as the crate's author.

The remaining Rust notice texts are read from checksum-verified Cargo archives
and pinned upstream notices. See `docs/DISTRIBUTION.md` for generation and
verification commands. The generated Rust bundle does not cover the native
FFmpeg libraries or Microsoft runtime and is not a complete installer payload.
