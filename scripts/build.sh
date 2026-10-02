#!/usr/bin/env bash
# builds build/Pavo.app: the swift menu bar app with the rust engine tucked inside it.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(grep -m1 '^version' Cargo.toml | cut -d '"' -f 2)
app=build/Pavo.app

echo "→ rust engine"
cargo build --release -p pavo-cli

echo "→ menu bar app"
swift build -c release --package-path apps/macos
bin=$(swift build -c release --package-path apps/macos --show-bin-path)

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Resources"
cp "$bin/Pavo" "$app/Contents/MacOS/Pavo"
cp target/release/pavo "$app/Contents/Helpers/pavo"
cp "$bin/PavoVision" "$app/Contents/Helpers/pavo-vision"
sed "s/__VERSION__/$version/g" apps/macos/Info.plist > "$app/Contents/Info.plist"

echo "→ icon"
work=$(mktemp -d)
cp apps/macos/AppIcon.svg "$work/icon.svg"
target/release/pavo run to:png "$work/icon.svg" 2>/dev/null
mkdir "$work/Pavo.iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$work/icon.png" --out "$work/Pavo.iconset/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$work/icon.png" --out "$work/Pavo.iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$work/Pavo.iconset" -o "$app/Contents/Resources/Pavo.icns"
rm -rf "$work"

# the standalone ffmpeg from scripts/build-ffmpeg.sh, if it's been built (releases always have it)
if [ -x vendor/ffmpeg/bin/ffmpeg ]; then
  cp vendor/ffmpeg/bin/ffmpeg "$app/Contents/Helpers/ffmpeg"
  strip -x "$app/Contents/Helpers/ffmpeg"
  cp vendor/ffmpeg/LICENSE.md "$app/Contents/Resources/ffmpeg-LICENSE.md"
else
  echo "  (no bundled ffmpeg: run scripts/build-ffmpeg.sh for video in a release; a local build uses homebrew's)"
fi

# sign with your Developer ID if it's in the keychain, otherwise ad-hoc for local runs
identity=$(security find-identity -v -p codesigning | grep -o '"Developer ID Application: [^"]*"' | head -1 | tr -d '"' || true)
sign=(codesign --force --options runtime --timestamp --sign "${identity:-}")
if [ -z "$identity" ]; then
  sign=(codesign --force --sign -)
fi
entitlements=apps/macos/Pavo.entitlements # apple events, for exporting pages/numbers/keynote files
"${sign[@]}" "$app/Contents/Helpers/ffmpeg" "$app/Contents/Helpers/pavo-vision"
"${sign[@]}" --entitlements "$entitlements" "$app/Contents/Helpers/pavo"
"${sign[@]}" --entitlements "$entitlements" "$app"
echo "  signed: ${identity:-ad-hoc (local only)}"

echo "✓ $app"
