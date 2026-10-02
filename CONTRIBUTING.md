# contributing

thanks for wanting to help. pavo stays useful by staying small, so here's the deal:

**yes please**
- new formats and conversions
- making conversions faster, smaller or better quality
- bug fixes
- windows & linux support

**probably not**
- settings, accounts, notifications, onboarding — pavo does one thing

## getting set up

```
./scripts/build.sh        # builds build/Pavo.app
cargo test                # the engine's tests (PAVO_VISION=apps/macos/.build/release/PavoVision for background removal)
swift test --package-path apps/macos   # the app's tests (after build.sh)
```

- conversions live in `crates/pavo-core`. `actions_for` decides what shows up in the menu, `run` does the work.
- the menu bar app is `apps/macos`: swift 6, main-actor by default, no dependencies. swift code follows [write-swift](https://github.com/emilkowalski/skills/blob/main/skills/write-swift/SKILL.md).
- every output goes next to the original and never overwrites anything — use `paths::output_for` and `Staged`.
- keep it light. if a change adds a heavy dependency or something that runs in the background, say why in the pr.

open a pr with what you changed and how you tested it. small prs get merged fastest.
