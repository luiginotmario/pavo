# hi, this is pavo

a tiny, open-source menu bar app that converts files. drop a file on the peacock in your menu bar, pick a format, and the new file shows up right next to the old one.

- free and open source (MIT)
- 100% local. your files never leave your mac.
- no account, no settings, no tracking.
- light: ~11 MB of memory and 0% cpu while it waits. the conversion engine only runs while it's converting.
- updates itself quietly. it only looks for a new version when you use it (never in the background), checks it's signed by pavo, and swaps it in when you're done.
- the menu bar icon is optional: hide it and right-click → convert with pavo and ⇧-drag keep working. open pavo again to bring it back.

## what it does

three ways in:

- **drop** files on the peacock in your menu bar (or click it and choose some)
- **hold ⇧ while dragging** files anywhere and a wheel of formats opens around your pointer. hold ⌥ too for edits and tools
- **right-click** files → services → convert with pavo, and the same wheel opens

| drop a… | convert to | edit & tools |
|---|---|---|
| photo | jpg · png · webp · heic · avif · tiff · bmp · gif · pdf | remove background · white background · compress (light, balanced or smallest) · crop · rotate · half size · strip metadata |
| svg | png · jpg · webp · pdf | |
| video | mp4 · mov · mkv · webm · avi · wmv · gif | remove watermark · compress · trim · split · join · crop · rotate · mute · pull out the audio · save a frame · strip metadata |
| audio | mp3 · m4a · wav · flac · ogg · opus · aiff · wma | compress · trim · split · join · strip metadata |
| pdf | png · jpg (every page, 300 dpi) · txt · docx | remove watermark · compress · merge · split into pages · rotate |
| pages | pdf · docx · epub · txt | |
| numbers | pdf · xlsx · csv | |
| keynote | pdf · pptx | |
| word, rtf, odt, html | docx · doc · rtf · odt · html · txt · pdf | |
| text | pdf · png · jpg · docx · rtf · html · srt · vtt | |
| subtitles | srt · vtt · txt | |
| archives | | unpack zip · tar · gz · rar · 7z · xz · bz2 |
| anything | | zip · tar.gz · gzip |

**remove watermark** finds it by itself (pdf watermark objects; on video, text apple's vision framework reads in the same spot all the way through, and marks that never move) or takes the text or a box you point at. it's for your own files: drafts, your exports and recordings. removing watermarks from other people's work can break copyright law.

nothing is ever overwritten. if `clip.mp4` already exists you get `clip 2.mp4`.

video goes through [ffmpeg](https://ffmpeg.org) (an lgpl build compiled from source by `scripts/build-ffmpeg.sh`, bundled inside the app) using your mac's hardware encoders, so it's fast and doesn't cook your battery. pdfs are drawn by macos's own pdf engine, and backgrounds are removed by apple's vision framework (the same on-device model photos uses), running on the neural engine. heic, avif, word documents and rar files go through tools that ship with macos (`sips`, `textutil`, `bsdtar`), and pages, numbers and keynote files are exported by those apps themselves. everything else is plain rust.

## download

grab **[Pavo.dmg](https://github.com/luiginotmario/pavo/releases/latest/download/Pavo.dmg)**, open it, drag the peacock into applications. apple silicon, macos 14 or newer.

signed and notarized by apple, so it opens like any other app.

## if you wanna make an addition + pr, or just wanna remix it for yourself, go for it

- clone the repo
- install [rust](https://rustup.rs) and ffmpeg (`brew install ffmpeg`)
- run `./scripts/build.sh` (and `./scripts/build-ffmpeg.sh` once first, if you want video to work without homebrew)
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
