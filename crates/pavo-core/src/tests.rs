use std::fs;
use std::path::{Path, PathBuf};

use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("pavo-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ids(paths: &[PathBuf]) -> Vec<String> {
    actions_for(paths).into_iter().map(|a| a.id).collect()
}

fn quiet() -> impl FnMut(Event) {
    |_| {}
}

fn photo(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([x as u8 * 4, y as u8 * 5, 128])).save(&path).unwrap();
    path
}

fn page_count(path: &Path) -> usize {
    lopdf::Document::load(path).unwrap().get_pages().len()
}

#[test]
fn offers_sensible_actions() {
    let mov = ids(&["clip.mov".into()]);
    assert!(mov.contains(&"to:mp4".into()));
    assert!(!mov.contains(&"to:mov".into()));
    assert!(mov.contains(&"audio".into()));

    let jpeg = ids(&["photo.JPEG".into()]);
    assert!(!jpeg.contains(&"to:jpg".into()), "jpeg and jpg are the same format");
    assert!(jpeg.contains(&"strip-metadata".into()));

    assert!(ids(&["a.pdf".into(), "b.pdf".into()]).contains(&"pdf:merge".into()));
    assert_eq!(ids(&["stuff.tar.gz".into()]), vec!["unpack".to_string()]);
}

#[test]
fn never_overwrites() {
    let dir = TempDir::new("names");
    let input = dir.0.join("clip.mov");
    assert_eq!(paths::output_for(&input, "mp4", ""), dir.0.join("clip.mp4"));
    fs::write(dir.0.join("clip.mp4"), b"taken").unwrap();
    assert_eq!(paths::output_for(&input, "mp4", ""), dir.0.join("clip 2.mp4"));
    assert_eq!(paths::base(Path::new("x/backup.tar.gz")), "backup");
}

#[test]
fn converts_images() {
    let dir = TempDir::new("images");
    let png = photo(&dir.0, "pic.png");
    for target in ["jpg", "webp", "tiff", "bmp", "gif"] {
        let out = run(&format!("to:{target}"), &[png.clone()], &mut quiet()).unwrap();
        let back = image::open(&out[0]).unwrap();
        assert_eq!((back.width(), back.height()), (64, 48), "{target}");
    }
}

#[test]
fn strips_jpeg_metadata_without_touching_pixels() {
    let dir = TempDir::new("strip");
    let jpg = photo(&dir.0, "pic.jpg");
    let clean = fs::read(&jpg).unwrap();

    // splice a fake exif block in after the SOI marker
    let mut tagged = clean[..2].to_vec();
    let exif = b"Exif\0\0secret location";
    tagged.extend([0xFF, 0xE1]);
    tagged.extend(((exif.len() + 2) as u16).to_be_bytes());
    tagged.extend(exif);
    tagged.extend(&clean[2..]);
    fs::write(&jpg, &tagged).unwrap();

    let out = run("strip-metadata", &[jpg], &mut quiet()).unwrap();
    let stripped = fs::read(&out[0]).unwrap();
    assert!(!stripped.windows(6).any(|w| w == b"secret"));
    assert_eq!(stripped, images::strip_jpeg(&clean).unwrap());
    image::load_from_memory(&stripped).unwrap();
}

#[test]
fn pdfs_round_trip() {
    let dir = TempDir::new("pdf");
    let a = photo(&dir.0, "a.png");
    let b = photo(&dir.0, "b.png");
    let c = photo(&dir.0, "c.png");

    let two = run("to:pdf", &[a.clone(), b], &mut quiet()).unwrap().remove(0);
    let one = run("to:pdf", &[c], &mut quiet()).unwrap().remove(0);
    assert_eq!(page_count(&two), 2);

    let merged = run("pdf:merge", &[two.clone(), one], &mut quiet()).unwrap().remove(0);
    assert_eq!(page_count(&merged), 3);

    let pages = run("pdf:split", &[merged], &mut quiet()).unwrap().remove(0);
    let split: Vec<_> = fs::read_dir(&pages).unwrap().collect();
    assert_eq!(split.len(), 3);

    let rotated = run("pdf:rotate", &[two], &mut quiet()).unwrap().remove(0);
    let doc = lopdf::Document::load(&rotated).unwrap();
    let first = doc.get_pages()[&1];
    assert_eq!(doc.get_dictionary(first).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 90);
}

#[test]
fn archives_round_trip() {
    let dir = TempDir::new("archive");
    let folder = dir.0.join("notes");
    fs::create_dir_all(folder.join("inner")).unwrap();
    fs::write(folder.join("a.txt"), "hello").unwrap();
    fs::write(folder.join("inner/b.txt"), "world").unwrap();

    for kind in ["zip", "tar.gz"] {
        let archive = run(kind, &[folder.clone()], &mut quiet()).unwrap().remove(0);
        let out = run("unpack", &[archive], &mut quiet()).unwrap().remove(0);
        assert_eq!(fs::read_to_string(out.join("notes/inner/b.txt")).unwrap(), "world", "{kind}");
    }
}

#[test]
fn reads_ffmpeg_output() {
    let probe = ffmpeg::parse_probe(
        "  Duration: 00:01:02.50, start: 0.000000, bitrate: 1205 kb/s\n\
         Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p(tv, bt709), 1920x1080 [SAR 1:1 DAR 16:9], 30 fps\n\
         Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 48000 Hz, stereo, fltp, 128 kb/s",
    );
    assert_eq!(probe.duration, Some(62.5));
    assert_eq!(probe.video.as_deref(), Some("h264"));
    assert_eq!(probe.audio.as_deref(), Some("aac"));
    assert_eq!((probe.width, probe.height), (1920, 1080));
}

#[test]
fn reads_times() {
    assert_eq!(parse_time("90"), Some(90.0));
    assert_eq!(parse_time("1:30"), Some(90.0));
    assert_eq!(parse_time("0:01:30.5"), Some(90.5));
    assert_eq!(parse_time("soon"), None);
}

#[test]
fn offers_every_conversion_and_tool() {
    let video = ids(&["clip.mov".into()]);
    for id in ["to:wmv", "to:gif", "trim", "split", "crop:square", "mute", "compress", "audio"] {
        assert!(video.contains(&id.to_string()), "video should offer {id}");
    }
    let pdf = ids(&["doc.pdf".into()]);
    for id in ["to:png", "to:jpg", "to:txt", "to:docx", "compress", "pdf:split"] {
        assert!(pdf.contains(&id.to_string()), "pdf should offer {id}");
    }
    assert!(ids(&["a.mp3".into(), "b.mp3".into()]).contains(&"join".to_string()));
    assert!(ids(&["photo.png".into()]).contains(&"to:avif".to_string()));
    assert_eq!(ids(&["backup.rar".into()]), vec!["unpack".to_string()]);
}

#[test]
fn crops_images_from_the_middle() {
    let dir = TempDir::new("crop");
    let png = photo(&dir.0, "wide.png"); // 64x48
    let square = run("crop:square", &[png.clone()], &mut quiet()).unwrap().remove(0);
    let img = image::open(&square).unwrap();
    assert_eq!((img.width(), img.height()), (48, 48));
    let tall = image::open(run("crop:9x16", &[png], &mut quiet()).unwrap().remove(0)).unwrap();
    assert_eq!((tall.width(), tall.height()), (27, 48));
}

#[test]
fn converts_subtitles_both_ways() {
    let srt = "1\n00:00:01,000 --> 00:00:02,500\nhi\n\n2\n00:00:03,000 --> 00:00:04,000\nbye\n";
    let vtt = docs::srt_to_vtt(srt);
    assert!(vtt.starts_with("WEBVTT"));
    assert!(vtt.contains("00:00:01.000 --> 00:00:02.500"));
    assert_eq!(docs::vtt_to_srt(&vtt).trim(), srt.trim());
    assert_eq!(docs::subtitle_text(srt), "hi\nbye\n");
}

#[test]
fn gzips_and_unpacks() {
    let dir = TempDir::new("gz");
    let note = dir.0.join("note.txt");
    fs::write(&note, "hello").unwrap();
    let gz = run("gz", &[note.clone()], &mut quiet()).unwrap().remove(0);
    assert_eq!(gz.file_name().unwrap(), "note.txt.gz");
    let back = run("unpack", &[gz], &mut quiet()).unwrap().remove(0);
    assert_eq!(fs::read_to_string(back).unwrap(), "hello");
}

#[test]
#[cfg(target_os = "macos")]
fn removes_backgrounds_when_the_helper_is_there() {
    // set PAVO_VISION to apps/macos/.build/release/PavoVision to run this
    if std::env::var_os("PAVO_VISION").is_none() {
        return;
    }
    let dir = TempDir::new("cutout");
    let parrot = dir.0.join("parrot.heic");
    fs::copy("/Library/User Pictures/Animals/Parrot.heic", &parrot).unwrap();
    let png = run("cutout", &[parrot.clone()], &mut quiet()).unwrap().remove(0);
    assert_eq!(png.file_name().unwrap(), "parrot (no background).png");
    let img = image::open(&png).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(img.width() - 1, img.height() - 1).0[3], 0, "the corner should be see-through");
    let jpg = run("cutout:white", &[parrot], &mut quiet()).unwrap().remove(0);
    assert_eq!(jpg.file_name().unwrap(), "parrot (white background).jpg");
}

#[test]
fn compress_levels_get_smaller() {
    let dir = TempDir::new("levels");
    // a noisy photo, so there's something to squeeze
    let path = dir.0.join("noisy.jpg");
    let img = image::RgbImage::from_fn(800, 600, |x, y| {
        let n = ((x * 7919 + y * 104_729) % 251) as u8;
        image::Rgb([n, n.wrapping_mul(3), (x % 255) as u8])
    });
    image::codecs::jpeg::JpegEncoder::new_with_quality(fs::File::create(&path).unwrap(), 98).encode_image(&img).unwrap();

    let size = |level: &str| {
        let out = run(level, &[path.clone()], &mut quiet()).unwrap().remove(0);
        let bytes = fs::metadata(&out).unwrap().len();
        fs::remove_file(out).unwrap();
        bytes
    };
    let (light, balanced, smallest) = (size("compress:light"), size("compress"), size("compress:smallest"));
    assert!(light > balanced && balanced > smallest, "{light} > {balanced} > {smallest}");
}

#[test]
#[cfg(target_os = "macos")]
fn offers_exports_for_pages_numbers_and_keynote() {
    let pages = ids(&["letter.pages".into()]);
    for id in ["to:pdf", "to:docx", "to:epub", "to:txt"] {
        assert!(pages.contains(&id.to_string()), "pages should offer {id}");
    }
    assert!(ids(&["budget.numbers".into()]).contains(&"to:xlsx".to_string()));
    assert!(ids(&["talk.key".into()]).contains(&"to:pptx".to_string()));
}
