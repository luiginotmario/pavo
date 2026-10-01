use std::fs;
use std::path::{Path, PathBuf};

use super::*;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("cambio-test-{name}-{}", std::process::id()));
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
