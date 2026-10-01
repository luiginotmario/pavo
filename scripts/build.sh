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

# releases ship a standalone ffmpeg inside the app: FFMPEG=/path/to/ffmpeg ./scripts/build.sh
if [ -n "${FFMPEG:-}" ]; then
  cp "$FFMPEG" "$app/Contents/Helpers/ffmpeg"
fi

# ad-hoc signature so macOS will open a local build
codesign --force --sign - "$app/Contents/Helpers/"*
codesign --force --sign - "$app"

echo "✓ $app"
