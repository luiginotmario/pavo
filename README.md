# hi, this is cambio

a tiny, open-source menu bar app that converts files. drop a file on the peacock in your menu bar, pick a format, and the new file shows up right next to the old one.

- free and open source
- runs 100% on your computer. your files never go anywhere.
- no account, no settings, no tracking
- built to sit quietly on a base macbook air

> early days — i'm building this in public. mac first, windows & linux after.

## what it converts

| | |
|---|---|
| images | jpg, png, webp, heic, avif, tiff, bmp, gif, svg → pdf |
| video | mp4, mov, mkv, webm, avi, gif, pull out the audio |
| audio | mp3, m4a, wav, flac, ogg, opus, aiff |
| pdfs | → images, merge, split, rotate, compress |
| archives | zip, tar, gz, 7z, unpack rar |
| anything | compress, strip metadata |

## how it's built

```
cambio-core   rust    all the conversion logic, shared by every platform
cambio        rust    command-line tool
Cambio.app    swift   the tiny mac menu bar app that calls the core
```

video and audio go through [ffmpeg](https://ffmpeg.org), using your mac's hardware encoders so it doesn't cook your battery.

## make it yours

```
git clone https://github.com/luiginotmario/cambio
cd cambio
```

if you wanna add a format or fix something, open a pr and i'll ship it in the next release. or just fork it and make your own version.

## website

the site lives in [`site/`](site). it's plain html — run `python3 -m http.server --directory site` and open http://localhost:8000.

## license

[MIT](LICENSE)
