# GCC 15.2.0 runtime license material

`COPYING3` and `COPYING.RUNTIME` are unmodified files from GCC release commit
`5115c7e447fc07457443df874bf57840e8316d5f` (`releases/gcc-15.2.0`).
Their checksums and source references are in `docs/ffmpeg-runtime-inputs.json`.

The fixed FFmpeg binary reports GCC 15.2.0. Its build recipe specifies
`-static-libgcc` and `-static-libstdc++`, and its configuration includes `-lgomp`.
The inspected libgcc, libstdc++ and libgomp source notices explicitly reference
GPL version 3 or later and GCC Runtime Library Exception version 3.1.

The exception applies to files bearing the corresponding notice and has its own
conditions, including an eligible compilation process. These files do not
replace the licenses of independent modules, Rust runtime code, MinGW, or the
Microsoft runtime. See `docs/DISTRIBUTION.md` for the remaining distribution work.
