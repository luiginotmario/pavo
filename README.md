# hi, this is pavo

a tiny, open-source menu bar app that converts files. drop a file on the peacock in your menu bar, pick a format, and the new file shows up right next to the old one.

- free and open source (MIT)
- 100% local. your files never leave your mac.
- no account, no settings, no tracking.
- light: ~11 MB of memory and 0% cpu while it waits. the conversion engine only runs while it's converting.

## what it does

| drop a… | and get |
|---|---|
| photo | jpg · png · webp · heic · tiff · bmp · gif · pdf · compress · strip metadata |
| svg | png · jpg · webp · pdf |
| video | mp4 · mov · mkv · webm · avi · gif · pull out the audio · compress · strip metadata |
| audio | mp3 · m4a · wav · flac · ogg · opus · aiff · strip metadata |
| pdfs | merge · split into pages · rotate |
| a bunch of photos | one pdf |
| zip / tar.gz | unpack |
| anything | zip |

nothing is ever overwritten. if `clip.mp4` already exists you get `clip 2.mp4`.

video goes through [ffmpeg](https://ffmpeg.org) using your mac's hardware encoders, so it's fast and doesn't cook your battery. heic goes through `sips`, which ships with macos. everything else is plain rust.

## if you wanna make an addition + pr, or just wanna remix it for yourself, go for it

- clone the repo
- install [rust](https://rustup.rs) and ffmpeg (`brew install ffmpeg`)
- run `./scripts/build.sh`
- open `build/Pavo.app` — done, you're up and running.

make changes on a pr and i'll build a new version :)

## how it's built

```
crates/pavo-core   rust    every conversion lives here
crates/pavo-cli    rust    `pavo` on the command line — the app runs this same binary
apps/macos           swift   the menu bar app. no dependencies.
```

the swift app stays tiny: it draws the icon, takes the drop, shows the menu, and hands the work to the rust engine. when it's done, the engine exits.

you can use the engine without the app too:

```
cargo build --release
./target/release/pavo actions clip.mov
./target/release/pavo run to:mp4 clip.mov
```

## license

[MIT](LICENSE) — made by [@giginotmario](https://x.com/giginotmario)
