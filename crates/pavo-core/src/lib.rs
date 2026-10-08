//! pavo-core: everything pavo can do to a file, in one place.
//!
//! Ask [`actions_for`] what makes sense for some files, then hand one of those
//! action ids to [`run`]. Every result is written next to the original, and
//! nothing is ever overwritten.

mod archive;
mod cancel;
mod docs;
mod ffmpeg;
mod images;
#[cfg(target_os = "macos")]
mod iwork;
mod paths;
mod pdf;
mod watermark;
#[cfg(target_os = "macos")]
mod render;

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};

pub use cancel::cancel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Vector,
    Video,
    Audio,
    Pdf,
    Document,
    Text,
    Subtitle,
    /// Pages, Numbers and Keynote
    IWork,
    Archive,
    Folder,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// "turn this into a .png"
    Convert,
    /// changes the file but keeps its format: compress, trim, crop…
    Edit,
    /// everything else: zip, strip metadata, pull out the audio…
    Tool,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Convert => "convert",
            Group::Edit => "edit",
            Group::Tool => "tool",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub group: Group,
}

/// How hard compress works. Balanced is the default: smaller, with no difference you'd notice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// barely touches quality
    Light,
    /// noticeably smaller, looks and sounds the same at normal size
    Balanced,
    /// as small as it gets: lower quality, and very large photos and videos are scaled down
    Smallest,
}

impl Level {
    fn from_action(action: &str) -> Option<Self> {
        match action {
            "compress" | "compress:balanced" => Some(Level::Balanced),
            "compress:light" => Some(Level::Light),
            "compress:smallest" => Some(Level::Smallest),
            _ => None,
        }
    }

    /// Pick one of three values, lightest first.
    pub(crate) fn pick<T>(self, light: T, balanced: T, smallest: T) -> T {
        match self {
            Level::Light => light,
            Level::Balanced => balanced,
            Level::Smallest => smallest,
        }
    }
}

pub enum Event<'a> {
    /// Started working on `input` (`index` of `total`).
    Start { input: &'a Path, index: usize, total: usize },
    /// Progress through the current input, 0.0 – 1.0.
    Progress(f64),
    /// A finished file or folder.
    Output(PathBuf),
}

const RASTER_TARGETS: &[&str] = &["jpg", "png", "webp", "heic", "avif", "tiff", "bmp", "gif"];
const VIDEO_TARGETS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "wmv", "gif"];
const AUDIO_TARGETS: &[&str] = &["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff", "wma"];
const TEXT_TARGETS: &[&str] = &["pdf", "png", "jpg", "docx", "rtf", "html", "srt", "vtt"];
const SUBTITLE_TARGETS: &[&str] = &["srt", "vtt", "txt"];

pub fn kind_of(path: &Path) -> Kind {
    let ext = paths::ext(path);
    // older Pages, Numbers and Keynote files are folders that look like files
    if matches!(ext.as_str(), "pages" | "numbers" | "key") {
        return Kind::IWork;
    }
    if path.is_dir() {
        return Kind::Folder;
    }
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "heic" | "heif" | "avif" | "tif" | "tiff" | "bmp" | "gif" => {
            Kind::Image
        }
        "svg" => Kind::Vector,
        "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "flv" | "mpg" | "mpeg" | "3gp" | "ts"
        | "mts" => Kind::Video,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "oga" | "opus" | "aiff" | "aif" | "wma" => Kind::Audio,
        "pdf" => Kind::Pdf,
        "docx" | "doc" | "rtf" | "odt" | "html" | "htm" => Kind::Document,
        "txt" | "md" | "markdown" => Kind::Text,
        "srt" | "vtt" => Kind::Subtitle,
        "zip" | "tar" | "tar.gz" | "tgz" | "gz" | "rar" | "7z" | "xz" | "bz2" | "tar.xz" | "tar.bz2" => {
            Kind::Archive
        }
        _ => Kind::Other,
    }
}

/// `jpeg` and `jpg` are the same thing, so are `tif`/`tiff`, `heif`/`heic`…
fn canonical_ext(path: &Path) -> String {
    match paths::ext(path).as_str() {
        "jpeg" => "jpg".into(),
        "tif" => "tiff".into(),
        "heif" => "heic".into(),
        "aif" => "aiff".into(),
        "m4v" => "mp4".into(),
        "htm" => "html".into(),
        other => other.into(),
    }
}

/// What can be done with these files, in the order it should be shown.
pub fn actions_for(inputs: &[PathBuf]) -> Vec<Action> {
    if inputs.is_empty() {
        return vec![];
    }
    let kinds: Vec<Kind> = inputs.iter().map(|p| kind_of(p)).collect();
    let exts: Vec<String> = inputs.iter().map(|p| canonical_ext(p)).collect();
    let own = if exts.iter().all(|e| *e == exts[0]) { exts[0].clone() } else { String::new() };
    let kind = if kinds.iter().all(|k| *k == kinds[0]) {
        kinds[0]
    } else if kinds.iter().all(|k| matches!(k, Kind::Image | Kind::Vector)) {
        Kind::Image
    } else {
        Kind::Other
    };
    let many = inputs.len() > 1;

    let mut out = Vec::new();
    let mut add = |group: Group, id: &str, label: &str| {
        out.push(Action { id: id.into(), label: label.into(), group })
    };
    let convert_to = |add: &mut dyn FnMut(Group, &str, &str), targets: &[&str]| {
        for &t in targets {
            if t != own && (cfg!(target_os = "macos") || !matches!(t, "heic" | "avif")) {
                add(Group::Convert, &format!("to:{t}"), t);
            }
        }
    };

    match kind {
        Kind::Image => {
            convert_to(&mut add, RASTER_TARGETS);
            add(Group::Convert, "to:pdf", if many { "one pdf" } else { "pdf" });
            if own == "gif" {
                convert_to(&mut add, &["mp4", "webm"]);
            }
            if matches!(own.as_str(), "jpg" | "png" | "webp" | "heic" | "avif") {
                add(Group::Edit, "compress", "compress");
            }
            if own != "gif" && cfg!(target_os = "macos") {
                add(Group::Edit, "cutout", "remove background");
                add(Group::Edit, "cutout:white", "white background");
            }
            if own != "gif" {
                add(Group::Edit, "crop:square", "crop square");
                add(Group::Edit, "crop:16x9", "crop 16:9");
                add(Group::Edit, "crop:9x16", "crop 9:16");
                add(Group::Edit, "rotate", "rotate 90°");
                add(Group::Edit, "shrink", "half the size");
            }
            if matches!(own.as_str(), "jpg" | "png") {
                add(Group::Tool, "strip-metadata", "strip metadata");
            }
        }
        Kind::Vector => {
            convert_to(&mut add, &["png", "jpg", "webp"]);
            add(Group::Convert, "to:pdf", if many { "one pdf" } else { "pdf" });
        }
        Kind::Video => {
            convert_to(&mut add, VIDEO_TARGETS);
            add(Group::Edit, "compress", "compress");
            add(Group::Edit, "trim", "trim");
            if many {
                add(Group::Edit, "join", "join into one");
            }
            add(Group::Edit, "split", "split in half");
            add(Group::Edit, "crop:square", "crop square");
            add(Group::Edit, "crop:16x9", "crop 16:9");
            add(Group::Edit, "crop:9x16", "crop 9:16");
            add(Group::Edit, "rotate", "rotate 90°");
            add(Group::Edit, "mute", "mute");
            add(Group::Edit, "unwatermark", "remove watermark");
            add(Group::Tool, "audio", "pull out the audio");
            add(Group::Tool, "frame", "save a frame");
            add(Group::Tool, "strip-metadata", "strip metadata");
        }
        Kind::Audio => {
            convert_to(&mut add, AUDIO_TARGETS);
            add(Group::Edit, "compress", "compress");
            add(Group::Edit, "trim", "trim");
            if many {
                add(Group::Edit, "join", "join into one");
            }
            add(Group::Edit, "split", "split in half");
            add(Group::Tool, "strip-metadata", "strip metadata");
        }
        Kind::Pdf => {
            convert_to(&mut add, &["png", "jpg", "txt", "docx"]);
            add(Group::Edit, "compress", "compress");
            if many {
                add(Group::Edit, "pdf:merge", "merge into one");
            }
            add(Group::Edit, "pdf:split", "split into pages");
            add(Group::Edit, "pdf:rotate", "rotate 90°");
            add(Group::Edit, "unwatermark", "remove watermark");
        }
        Kind::Document if cfg!(target_os = "macos") => convert_to(&mut add, docs::TARGETS),
        Kind::Text if cfg!(target_os = "macos") => convert_to(&mut add, TEXT_TARGETS),
        Kind::Subtitle => convert_to(&mut add, SUBTITLE_TARGETS),
        #[cfg(target_os = "macos")]
        Kind::IWork => convert_to(
            &mut add,
            match own.as_str() {
                "pages" => iwork::PAGES,
                "numbers" => iwork::NUMBERS,
                _ => iwork::KEYNOTE,
            },
        ),
        Kind::Archive => add(Group::Tool, "unpack", "unpack"),
        _ => {}
    }
    // pictures and pdfs picked together: one pdf of all of it
    if kind == Kind::Other
        && kinds.iter().all(|k| matches!(k, Kind::Image | Kind::Vector | Kind::Pdf))
        && kinds.contains(&Kind::Pdf)
    {
        add(Group::Convert, "to:pdf", "one pdf");
    }

    if kind != Kind::Archive {
        add(Group::Tool, "zip", if many { "zip them" } else { "zip" });
        if matches!(kind, Kind::Folder | Kind::Other) || many {
            add(Group::Tool, "tar.gz", "tar.gz");
        }
        if !many && kind != Kind::Folder {
            add(Group::Tool, "gz", "gzip");
        }
    }

    out
}

/// Seconds from `90`, `1:30`, `0:01:30` or `1:30.5`.
pub fn parse_time(text: &str) -> Option<f64> {
    text.trim().split(':').try_fold(0.0, |total, part| Some(total * 60.0 + part.trim().parse::<f64>().ok()?))
}

fn parse_regions(list: &str) -> Option<Vec<ffmpeg::Region>> {
    list.split(';')
        .map(|one| {
            let n: Vec<f64> = one.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
            let [x, y, w, h] = n[..] else { return None };
            (w > 0.0 && h > 0.0).then_some(ffmpeg::Region { x, y, w, h })
        })
        .collect()
}

/// Run an action from [`actions_for`]. Returns everything it made.
///
/// Trim takes its range in the id: `trim:0:05-0:20` (start-end, anything [`parse_time`] reads).
/// Compress takes an optional level: `compress:light`, `compress` (balanced) or `compress:smallest`.
/// Remove watermark finds it by itself (`unwatermark`) or by its text (`unwatermark:text=CONFIDENTIAL`).
pub fn run(action: &str, inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<Vec<PathBuf>> {
    if inputs.is_empty() {
        bail!("no files given");
    }
    cancel::reset();
    let total = inputs.len();

    // actions that turn many inputs into one output
    let combined = match action {
        "to:pdf" => inputs.iter().all(|p| matches!(kind_of(p), Kind::Image | Kind::Vector | Kind::Pdf)),
        "pdf:merge" | "join" => true,
        "zip" | "tar.gz" => total > 1,
        _ => false,
    };
    if combined {
        on(Event::Start { input: &inputs[0], index: 0, total: 1 });
        let output = match action {
            "to:pdf" => images::to_pdf(inputs, on)?,
            "pdf:merge" => pdf::merge(inputs, on)?,
            "join" => ffmpeg::join(inputs, on)?,
            "zip" => archive::zip(inputs)?,
            _ => archive::tar_gz(inputs)?,
        };
        on(Event::Progress(1.0));
        on(Event::Output(output.clone()));
        return Ok(vec![output]);
    }

    let mut outputs = Vec::with_capacity(total);
    for (index, input) in inputs.iter().enumerate() {
        cancel::check()?;
        // a jpg picked with a heic for "to jpg" is already done; no copy of it
        if total > 1 && action.strip_prefix("to:") == Some(canonical_ext(input).as_str()) {
            continue;
        }
        on(Event::Start { input, index, total });
        for output in run_one(action, input, on)? {
            on(Event::Output(output.clone()));
            outputs.push(output);
        }
        on(Event::Progress(1.0));
    }
    Ok(outputs)
}

fn run_one(action: &str, input: &Path, on: &mut dyn FnMut(Event)) -> Result<Vec<PathBuf>> {
    use Kind::*;
    let kind = kind_of(input);
    let one = |path: Result<PathBuf>| path.map(|p| vec![p]);

    if let Some(range) = action.strip_prefix("trim:") {
        let (start, end) = range.split_once('-').context("trim needs a range like trim:0:05-0:20")?;
        let (Some(start), Some(end)) = (parse_time(start), parse_time(end)) else {
            bail!("couldn't read the times in {range}")
        };
        ensure!(matches!(kind, Video | Audio), "can't trim {}", paths::name(input));
        return one(ffmpeg::trim(input, start, end, on));
    }
    if let Some(aspect) = action.strip_prefix("crop:") {
        let (w, h) = match aspect {
            "square" => (1, 1),
            "16x9" => (16, 9),
            "9x16" => (9, 16),
            other => bail!("unknown crop {other}"),
        };
        return match kind {
            Image => one(images::crop(input, w, h)),
            Video => one(ffmpeg::crop(input, w, h, on)),
            _ => bail!("can't crop {}", paths::name(input)),
        };
    }

    if action == "unwatermark" || action.starts_with("unwatermark:") {
        let detail = action.strip_prefix("unwatermark:");
        return match kind {
            Pdf => one(watermark::remove_from_pdf(input, detail.and_then(|d| d.strip_prefix("text=")), on)),
            Video => {
                // unwatermark:x,y,w,h (fractions of the frame, from the top left), boxes separated by ;
                let boxes = match detail {
                    Some(list) => Some(parse_regions(list).context("boxes look like unwatermark:0.8,0.9,0.15,0.05")?),
                    None => None,
                };
                one(ffmpeg::remove_watermark(input, boxes, on))
            }
            _ => bail!("can't remove a watermark from {}", paths::name(input)),
        };
    }

    if let Some(level) = Level::from_action(action) {
        return match kind {
            Video => one(ffmpeg::compress(input, level, on)),
            Audio => one(ffmpeg::compress_audio(input, level, on)),
            Image => one(images::compress(input, level)),
            Pdf => one(pdf::compress(input, level, on)),
            _ => bail!("can't compress {}", paths::name(input)),
        };
    }

    match (action, kind) {
        ("trim", _) => bail!("trim needs a range like trim:0:05-0:20"),
        ("split", Video | Audio) => ffmpeg::split(input, on),
        ("rotate", Image) => one(images::rotate(input)),
        ("rotate", Video) => one(ffmpeg::rotate(input, on)),
        ("shrink", Image) => one(images::shrink(input)),
        ("cutout", Image) => one(images::cut_out(input, false)),
        ("cutout:white", Image) => one(images::cut_out(input, true)),
        ("mute", Video) => one(ffmpeg::mute(input, on)),
        ("frame", Video) => one(ffmpeg::frame(input, on)),
        ("strip-metadata", Video | Audio) => one(ffmpeg::strip_metadata(input, on)),
        ("strip-metadata", Image) => one(images::strip_metadata(input)),
        ("audio", Video) => one(ffmpeg::extract_audio(input, on)),
        ("pdf:split", Pdf) => one(pdf::split(input, on)),
        ("pdf:rotate", Pdf) => one(pdf::rotate(input)),
        ("unpack", Archive) => one(archive::unpack(input)),
        ("zip", _) => one(archive::zip(&[input.to_path_buf()])),
        ("tar.gz", _) => one(archive::tar_gz(&[input.to_path_buf()])),
        ("gz", _) => one(archive::gzip(input)),
        (a, k) if a.starts_with("to:") => {
            let target = &a[3..];
            one(match k {
                Image if paths::ext(input) == "gif" && matches!(target, "mp4" | "webm") => {
                    ffmpeg::convert(input, target, on)
                }
                Image | Vector => images::convert(input, target),
                Video | Audio => ffmpeg::convert(input, target, on),
                Pdf => match target {
                    "png" | "jpg" => pdf::to_images(input, input, target, on),
                    "txt" => pdf::to_text(input),
                    "docx" => pdf::to_docx(input),
                    _ => bail!("can't turn a pdf into .{target}"),
                },
                Document => docs::convert(input, target),
                Text => match target {
                    "png" | "jpg" => docs::text_to_images(input, target, on),
                    "srt" | "vtt" => docs::text_to_subtitles(input, target),
                    _ => docs::convert(input, target),
                },
                Subtitle => docs::convert_subtitles(input, target),
                #[cfg(target_os = "macos")]
                IWork => iwork::export(input, target),
                _ => bail!("can't turn {} into .{target}", paths::name(input)),
            })
        }
        _ => bail!("can't {action} {}", paths::name(input)),
    }
}

#[cfg(test)]
mod tests;
