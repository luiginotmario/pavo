#!/usr/bin/env bash
# packs build/Pavo.app into build/Pavo.dmg: the pencil "drag it over" window.
# notarizes it too when a notary profile exists: xcrun notarytool store-credentials pavo
set -euo pipefail
cd "$(dirname "$0")/.."

app=build/Pavo.app
dmg=build/Pavo.dmg
[ -d "$app" ] || { echo "build the app first: scripts/build.sh"; exit 1; }

# dmgbuild writes the finder layout without needing finder, so it works in ci too
# kept in ~/Library/Caches: macos clears old files out of the temp folder, which broke this once
venv="$HOME/Library/Caches/pavo-dmgbuild"
"$venv/bin/python3" -c "import dmgbuild, ds_store" 2>/dev/null || { rm -rf "$venv" && python3 -m venv "$venv" && "$venv/bin/pip" install --quiet dmgbuild; }

rm -f "$dmg"
"$venv/bin/dmgbuild" -s apps/macos/dmg/settings.py -D app="$app" "Pavo" "$dmg"

identity=$(security find-identity -v -p codesigning | grep -o '"Developer ID Application: [^"]*"' | head -1 | tr -d '"' || true)
if [ -n "$identity" ]; then
  codesign --force --timestamp --sign "$identity" "$dmg"
fi

profile="${NOTARY_PROFILE:-pavo}"
if xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
  echo "→ notarizing (a few minutes)"
  xcrun notarytool submit "$dmg" --keychain-profile "$profile" --wait
  xcrun stapler staple "$dmg"
  spctl --assess --type open --context context:primary-signature --verbose "$dmg"
else
  echo "  not notarized: no '$profile' notary profile in the keychain"
fi
echo "✓ $dmg ($(du -h "$dmg" | cut -f1))"
