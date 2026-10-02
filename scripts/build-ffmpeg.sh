#!/usr/bin/env bash
# builds the standalone ffmpeg that ships inside Pavo.app → vendor/ffmpeg/bin/ffmpeg
#
# lgpl only (no --enable-gpl), statically linked, nothing but macos's own libraries needed at runtime.
# h.264/hevc/aac go through the mac's hardware (videotoolbox, audiotoolbox); mp3, opus and vp9 come from
# lame, libopus and libvpx. sources are the official releases, checked against pinned sha256s.
set -euo pipefail
cd "$(dirname "$0")/.."

out="$PWD/vendor/ffmpeg"
work="${TMPDIR:-/tmp}/pavo-ffmpeg-build"
jobs=$(sysctl -n hw.ncpu)
export MACOSX_DEPLOYMENT_TARGET=14.0

if [ -x "$out/bin/ffmpeg" ]; then
  echo "✓ $out/bin/ffmpeg already built"
  exit 0
fi

fetch() { # url file sha256
  [ -f "$work/$2" ] || curl -sfL -o "$work/$2" "$1"
  echo "$3  $work/$2" | shasum -a 256 -c --quiet
  tar -xf "$work/$2" -C "$work"
}

rm -rf "$work/src" && mkdir -p "$work/src" "$out"
cd "$work"

echo "→ lame"
fetch https://downloads.sourceforge.net/project/lame/lame/3.100/lame-3.100.tar.gz lame-3.100.tar.gz \
  ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e
(cd lame-3.100 && ./configure --prefix="$out" --disable-shared --enable-static --disable-frontend --disable-dependency-tracking >/dev/null && make -j"$jobs" >/dev/null && make install >/dev/null)

echo "→ opus"
fetch https://downloads.xiph.org/releases/opus/opus-1.5.2.tar.gz opus-1.5.2.tar.gz \
  65c1d2f78b9f2fb20082c38cbe47c951ad5839345876e46941612ee87f9a7ce1
(cd opus-1.5.2 && ./configure --prefix="$out" --disable-shared --enable-static --disable-doc --disable-extra-programs >/dev/null && make -j"$jobs" >/dev/null && make install >/dev/null)

echo "→ libvpx"
fetch https://github.com/webmproject/libvpx/archive/refs/tags/v1.15.0.tar.gz libvpx-1.15.0.tar.gz \
  e935eded7d81631a538bfae703fd1e293aad1c7fd3407ba00440c95105d2011e
(cd libvpx-1.15.0 && ./configure --prefix="$out" --disable-shared --enable-static --disable-examples --disable-tools \
  --disable-docs --disable-unit-tests --enable-vp9-highbitdepth >/dev/null && make -j"$jobs" >/dev/null && make install >/dev/null)

# reads every format ffmpeg knows (all decoders, demuxers and parsers stay), but only writes what pavo makes
encoders=h264_videotoolbox,hevc_videotoolbox,mpeg4,libvpx_vp9,wmv2,gif,png,aac,aac_at,libmp3lame,libopus,flac,pcm_s16le,pcm_s16be,wmav2
muxers=mp4,mov,ipod,matroska,webm,avi,asf,gif,image2,mp3,wav,flac,ogg,opus,aiff
filters=scale,crop,pad,transpose,fps,format,setsar,split,palettegen,paletteuse,concat,aresample,aformat,null,anull,removelogo

echo "→ ffmpeg"
fetch https://ffmpeg.org/releases/ffmpeg-7.1.1.tar.xz ffmpeg-7.1.1.tar.xz \
  733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1
(cd ffmpeg-7.1.1 && PKG_CONFIG_PATH="$out/lib/pkgconfig" ./configure --prefix="$out" \
  --pkg-config-flags=--static --extra-cflags="-I$out/include" --extra-ldflags="-L$out/lib" \
  --enable-static --disable-shared --disable-debug --disable-doc --disable-ffplay --disable-ffprobe \
  --disable-network --disable-autodetect --disable-devices --disable-hwaccels \
  --enable-videotoolbox --enable-audiotoolbox --enable-zlib \
  --enable-libmp3lame --enable-libopus --enable-libvpx \
  --disable-encoders --enable-encoder="$encoders" \
  --disable-muxers --enable-muxer="$muxers" \
  --disable-filters --enable-filter="$filters" \
  --disable-protocols --enable-protocol=file,pipe \
  --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox >/dev/null && make -j"$jobs" >/dev/null && make install >/dev/null)

cp "$work/ffmpeg-7.1.1/LICENSE.md" "$out/LICENSE.md"
"$out/bin/ffmpeg" -hide_banner -version | head -1
echo "✓ $out/bin/ffmpeg"
