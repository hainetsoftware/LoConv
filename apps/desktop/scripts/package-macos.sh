#!/bin/bash
set -euo pipefail

desktop=$(cd "$(dirname "$0")/.." && pwd)
repo=$(cd "$desktop/../.." && pwd)
version=9.0.2
source_hash=8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
# The static codec libraries in the Apple Silicon Homebrew bottles target macOS 26.
export MACOSX_DEPLOYMENT_TARGET=26.0

if [[ $(uname -sm) != 'Darwin arm64' || $# != 1 ]]; then
    echo 'Usage on Apple Silicon: npm run package:macos -- /absolute/path/to/ffmpeg-9.0.2.tar.xz' >&2
    exit 1
fi

archive=$1
hash() { shasum -a 256 "$1" | awk '{print $1}'; }
if [[ $(hash "$archive") != "$source_hash" ]]; then
    echo 'FFmpeg source tarball does not match the pinned SHA-256.' >&2
    exit 1
fi

work=$(mktemp -d /tmp/loconv-package.XXXXXXXX)
trap 'rm -rf "$work"' EXIT
codecs=$work/codecs
tools=$work/tools
licenses=$tools/licenses
mkdir -p "$codecs/include" "$codecs/lib/pkgconfig" "$licenses"

stage_codec() {
    local prefix
    prefix=$(brew --prefix "$1")
    cp -R "$prefix/include/." "$codecs/include/"
    cp "$prefix/lib/$2.a" "$codecs/lib/"
    sed -E -e "s|^prefix=.*|prefix=$codecs|" -e 's|^libdir=.*|libdir=${prefix}/lib|' \
        "$prefix/lib/pkgconfig/$3.pc" > "$codecs/lib/pkgconfig/$3.pc"
    cp "$prefix/$4" "$licenses/$1.txt"
    printf '%s %s %s\n' "$1" "$(pkg-config --modversion "$prefix/lib/pkgconfig/$3.pc")" \
        "$(hash "$codecs/lib/$2.a")" >> "$tools/BUILD.txt"
}

printf 'FFmpeg %s for arm64 macOS %s+\nSource SHA-256: %s\nStatic codec libraries (name, version, SHA-256):\n' \
    "$version" "$MACOSX_DEPLOYMENT_TARGET" "$source_hash" > "$tools/BUILD.txt"
stage_codec lame libmp3lame lame COPYING
stage_codec mpg123 libmpg123 libmpg123 COPYING
stage_codec x264 libx264 x264 COPYING
stage_codec opus libopus opus COPYING
stage_codec libvpx libvpx vpx LICENSE

tar -xf "$archive" -C "$work"
src=$work/ffmpeg-$version
(
    cd "$src"
    PKG_CONFIG_PATH= PKG_CONFIG_LIBDIR="$codecs/lib/pkgconfig" ./configure \
        --disable-autodetect --disable-network --disable-protocols --enable-protocol=fd,file,pipe \
        --disable-shared --enable-static --disable-doc --disable-ffplay \
        --disable-indevs --disable-outdevs --enable-indev=lavfi \
        --disable-demuxers --enable-demuxer=avi,flac,image2,matroska,mov,mp3,ogg,wav \
        --disable-muxers --enable-muxer=avi,flac,matroska,mp3,mp4,null,ogg,pcm_s16le,wav,webm \
        --disable-encoders --enable-encoder=aac,flac,libmp3lame,libopus,libvpx_vp9,libx264,mpeg4,pcm_s16le,vorbis,wrapped_avframe \
        --disable-filters --enable-filter=aformat,anull,aresample,atrim,format,hflip,null,pad,rotate,scale,sine,testsrc2,transpose,trim,vflip \
        --enable-gpl --enable-libmp3lame --enable-libopus --enable-libvpx --enable-libx264 \
        --pkg-config-flags=--static --extra-cflags="-I$codecs/include" --extra-ldflags="-L$codecs/lib" \
        --extra-libs=-lmpg123 || {
        tail -80 ffbuild/config.log >&2
        exit 1
    }
    make -j "$(getconf _NPROCESSORS_ONLN)" ffmpeg ffprobe > "$work/make.log" 2>&1 || {
        tail -80 "$work/make.log" >&2
        exit 1
    }
    cp COPYING.GPLv2 COPYING.LGPLv2.1 LICENSE.md "$licenses/"
    install -m 755 ffmpeg ffprobe "$tools/"
)

for name in ffmpeg ffprobe; do
    tool=$tools/$name
    # Sign the exact files before calculating the pins embedded in the app.
    codesign --force --sign - "$tool"
    [[ $(lipo -archs "$tool") == arm64 ]]
    otool -L "$tool" | awk 'NR > 1 && $1 !~ /^\/(usr\/lib|System\/Library)\// { print; bad=1 } END { exit bad }'
    "$tool" -hide_banner -buildconf 2>&1 | grep -Fx '    --disable-network' >/dev/null
    protocols=$("$tool" -hide_banner -protocols | awk '/^  / {print $1}' | sort -u)
    [[ "$protocols" == $'fd\nfile\npipe' ]]
    printf '%s %s\n' "$name" "$(hash "$tool")" >> "$tools/BUILD.txt"
done
"$tools/ffmpeg" -hide_banner -buildconf >> "$tools/BUILD.txt" 2>&1
LOCONV_PACKAGE_FFMPEG_SHA256=$(hash "$tools/ffmpeg")
LOCONV_PACKAGE_FFPROBE_SHA256=$(hash "$tools/ffprobe")
export LOCONV_PACKAGE_FFMPEG_SHA256 LOCONV_PACKAGE_FFPROBE_SHA256

cd "$desktop"
# Tauri copies each resource to its destination under Contents/Resources.
node --input-type=module - "$tools" > "$work/tauri.json" <<'JS'
import { readdirSync } from 'node:fs';
const tools = process.argv[2];
const resources = {};
for (const name of readdirSync(tools)) {
  if (name !== 'licenses') resources[tools + '/' + name] = 'tools/' + name;
}
for (const name of readdirSync(tools + '/licenses')) {
  resources[tools + '/licenses/' + name] = 'tools/licenses/' + name;
}
console.log(JSON.stringify({ bundle: {
  active: true, resources, macOS: { minimumSystemVersion: '26.0' },
} }));
JS
export CARGO_TARGET_DIR="$repo/target"
npm run tauri build -- --bundles app --no-sign --no-binary-patching --config "$work/tauri.json" -- --locked

app=$CARGO_TARGET_DIR/release/bundle/macos/LoConv.app
for name in ffmpeg ffprobe; do
    cmp "$tools/$name" "$app/Contents/Resources/tools/$name"
    test -x "$app/Contents/Resources/tools/$name"
done
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
LOCONV_TEST_MEDIA_RESOURCES="$app/Contents/Resources" \
    cargo test --release --locked -p loconv-engine --features dev-tools real_ -- --ignored

# Create a disk image without relying on Finder to arrange an installer window.
output=$CARGO_TARGET_DIR/release/bundle/dmg
mkdir -p "$output" "$work/dmg"
ditto "$app" "$work/dmg/LoConv.app"
ln -s /Applications "$work/dmg/Applications"
app_version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
dmg="$output/LoConv_${app_version}_arm64.dmg"
hdiutil makehybrid -quiet -hfs -hfs-volume-name LoConv -o "$work/hybrid.dmg" "$work/dmg"
hdiutil convert -quiet "$work/hybrid.dmg" -format UDZO -o "$work/LoConv.dmg"
hdiutil verify "$work/LoConv.dmg"
mv "$work/LoConv.dmg" "$dmg"
printf 'macOS app: %s\nDisk image: %s\n' "$app" "$dmg"
