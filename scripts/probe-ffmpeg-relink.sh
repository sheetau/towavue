#!/usr/bin/env bash
set -euo pipefail

# Runs only in the pinned image with read-only inputs and a fresh /work mount.
test "$FFBUILD_TOOLCHAIN" = x86_64-w64-mingw32
test "$TARGET" = win64
test "$VARIANT" = lgpl-shared
test "$FFVER" = 900
test ! -e /work/chromaprint
test ! -e /work/ffmpeg
"$CC" --version
cmake --version

original_cflags=$CFLAGS
original_cxxflags=$CXXFLAGS
export CFLAGS="$CFLAGS $STAGE_CFLAGS"
export CXXFLAGS="$CXXFLAGS $STAGE_CXXFLAGS"
cmake -S /input/chromaprint -B /work/chromaprint \
    -DCMAKE_TOOLCHAIN_FILE="$FFBUILD_CMAKE_TOOLCHAIN" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/work/prefix \
    -DBUILD_SHARED_LIBS=OFF -DBUILD_TOOLS=OFF -DBUILD_TESTS=OFF \
    -DFFT_LIB=kissfft -DCMAKE_DISABLE_FIND_PACKAGE_FFmpeg=ON
cmake --build /work/chromaprint --parallel 4
cmake --install /work/chromaprint
printf '\nLibs.private: -lstdc++\nCflags.private: -DCHROMAPRINT_NODLL\n' \
    >> /work/prefix/lib/pkgconfig/libchromaprint.pc

export PKG_CONFIG_LIBDIR="/work/prefix/lib/pkgconfig:$PKG_CONFIG_LIBDIR"
export LDFLAGS="-L/work/prefix/lib $LDFLAGS"
test "$(pkg-config --variable=libdir libchromaprint)" = /work/prefix/lib
pkg-config --modversion libchromaprint
pkg-config --static --libs libchromaprint
"$NM" --undefined-only /work/prefix/lib/libchromaprint.a > /work/chromaprint-undefined.txt
if grep -E 'fftw[a-z0-9]*_' /work/chromaprint-undefined.txt; then
    echo 'Unexpected FFTW reference in the replacement Chromaprint library.' >&2
    exit 1
fi
sha256sum /work/prefix/lib/libchromaprint.a /work/prefix/lib/pkgconfig/libchromaprint.pc
export CFLAGS=$original_cflags
export CXXFLAGS=$original_cxxflags

# The image fixes these simple argument lists. Do not eval upstream scripts.
read -r -a target_flags <<< "$FFBUILD_TARGET_FLAGS"
read -r -a configure_flags <<< "$FF_CONFIGURE"
mkdir /work/ffmpeg
cd /work/ffmpeg
/input/ffmpeg/configure --prefix=/work/install --pkg-config-flags=--static \
    "${target_flags[@]}" "${configure_flags[@]}" \
    --extra-cflags="$FF_CFLAGS" --extra-cxxflags="$FF_CXXFLAGS" \
    --extra-libs="$FF_LIBS" --extra-ldflags="$FF_LDFLAGS -Wl,-t" \
    --extra-ldexeflags="$FF_LDEXEFLAGS" \
    --cc="$CC" --cxx="$CXX" --ar="$AR" --ranlib="$RANLIB" --nm="$NM" \
    --extra-version=towavue-kissfft-probe-e47273f4d9 \
    || { tail -n 100 ffbuild/config.log; exit 1; }
make -j4 V=1 2>&1 | tee /work/ffmpeg-build.log

grep -F '/work/prefix/lib/libchromaprint.a' /work/ffmpeg-build.log
if grep -E 'libfftw[^ /]*\.(a|so)|-lfftw' /work/ffmpeg-build.log; then
    echo 'Unexpected FFTW link input in the FFmpeg build.' >&2
    exit 1
fi
for binary in \
    libavcodec/avcodec-63.dll libavdevice/avdevice-63.dll \
    libavfilter/avfilter-12.dll libavformat/avformat-63.dll \
    libavutil/avutil-61.dll libswresample/swresample-7.dll \
    libswscale/swscale-10.dll ffmpeg.exe ffprobe.exe; do
    test -s "$binary"
    sha256sum "$binary"
done
strings libavformat/avformat-63.dll > /work/avformat-strings.txt
if grep -E 'fftw[a-z0-9]*_|fftw_wisdom' /work/avformat-strings.txt; then
    echo 'Unexpected FFTW marker in the replacement avformat DLL.' >&2
    exit 1
fi
"${FFBUILD_CROSS_PREFIX}objdump" -p libavformat/avformat-63.dll | grep 'DLL Name:'
echo 'Relink probe passed. Binaries are not published or approved for distribution.'
