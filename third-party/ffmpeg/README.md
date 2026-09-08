# FFmpeg build adjustment

`chromaprint-kissfft.patch` targets only
`BtbN/FFmpeg-Builds@8267213e26c1031621e6e1210fe3aa4867214f6a`.
It preserves Chromaprint functionality while selecting the bundled KissFFT
backend instead of statically linking GPL FFTW. It is a rebuild input, not
an approval of the existing FFmpeg binary or a completed replacement build.

The zero-context patch requires `git -c core.autocrlf=false apply --unidiff-zero`.
Verify the original script hash before applying it and the result hash after
application; both are recorded in `docs/ffmpeg-distribution-rejection.json`.
See `docs/FFMPEG_REBUILD.md` for the evidence, prototype and remaining gates.
