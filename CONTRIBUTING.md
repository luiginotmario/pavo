# contributing

thanks for wanting to help. cambio stays useful by staying small, so here's the deal:

**yes please**
- new formats and conversions
- making conversions faster, smaller or better quality
- bug fixes
- windows & linux support

**probably not**
- settings, accounts, notifications, onboarding — cambio does one thing

## getting set up

```
./scripts/build.sh        # builds build/Cambio.app
cargo test                # runs the engine's tests
```

- conversions live in `crates/cambio-core`. `actions_for` decides what shows up in the menu, `run` does the work.
- the menu bar app is `apps/macos` (swift, no dependencies).
- every output goes next to the original and never overwrites anything — use `paths::output_for` and `Staged`.
- keep it light. if a change adds a heavy dependency or something that runs in the background, say why in the pr.

open a pr with what you changed and how you tested it. small prs get merged fastest.
