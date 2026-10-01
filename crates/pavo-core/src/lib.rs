//! pavo-core: everything pavo can do to a file, in one place.
//!
//! Ask [`actions_for`] what makes sense for some files, then hand one of those
//! action ids to [`run`]. Every result is written next to the original, and
//! nothing is ever overwritten.

mod archive;
mod cancel;
mod ffmpeg;
mod images;
mod paths;
mod pdf;

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

pub use cancel::cancel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Vector,
    Video,
    Audio,
    Pdf,
    Archive,
    Folder,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// "turn this into a .png"
    Convert,
    /// everything else: compress, strip metadata, split…
    Tool,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Group::Convert => "convert",
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

pub enum Event<'a> {
    /// Started working on `input` (`index` of `total`).
    Start { input: &'a Path, index: usize, total: usize },
    /// Progress through the current input, 0.0 – 1.0.
    Progress(f64),
    /// A finished file or folder.
    Output(PathBuf),
}

const RASTER_TARGETS: &[&str] = &["jpg", "png", "webp", "heic", "tiff", "bmp", "gif"];
const VIDEO_TARGETS: &[&str] = &["mp4", "mov", "mkv", "webm", "avi", "gif"];
const AUDIO_TARGETS: &[&str] = &["mp3", "m4a", "wav", "flac", "ogg", "opus", "aiff"];

pub fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    match paths::ext(path).as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "heic" | "heif" | "avif" | "tif" | "tiff" | "bmp" | "gif" => {
            Kind::Image
        }
        "svg" => Kind::Vector,
        "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "flv" | "mpg" | "mpeg" | "3gp" | "ts"
        | "mts" => Kind::Video,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "oga" | "opus" | "aiff" | "aif" | "wma" => Kind::Audio,
        "pdf" => Kind::Pdf,
        "zip" | "tar" | "tar.gz" | "tgz" => Kind::Archive,
        _ => Kind::Other,
    }
}

/// `jpeg` and `jpg` are the same thing, so are `tif`/`tiff` and `heif`/`heic`.
fn canonical_ext(path: &Path) -> String {
    match paths::ext(path).as_str() {
        "jpeg" => "jpg".into(),
        "tif" => "tiff".into(),
        "heif" => "heic".into(),
        "aif" => "aiff".into(),
        "m4v" => "mp4".into(),
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
    let own = if exts.iter().all(|e| *e == exts[0]) { exts[0].as_str() } else { "" };
    let kind = if kinds.iter().all(|k| *k == kinds[0]) {
        kinds[0]
    } else if kinds.iter().all(|k| matches!(k, Kind::Image | Kind::Vector)) {
        Kind::Image
    } else {
        Kind::Other
    };
    let many = inputs.len() > 1;

    let mut out = Vec::new();
    let mut convert = |ext: &str, label: &str| {
        out.push(Action { id: format!("to:{ext}"), label: label.into(), group: Group::Convert })
    };

    match kind {
        Kind::Image => {
            for &t in RASTER_TARGETS {
                if t != own && (t != "heic" || cfg!(target_os = "macos")) {
                    convert(t, t);
                }
            }
            convert("pdf", if many { "one pdf" } else { "pdf" });
            if own == "gif" {
                convert("mp4", "mp4");
                convert("webm", "webm");
            }
        }
        Kind::Vector => {
            for t in ["png", "jpg", "webp"] {
                convert(t, t);
            }
            convert("pdf", if many { "one pdf" } else { "pdf" });
        }
        Kind::Video => {
            for &t in VIDEO_TARGETS {
                if t != own {
                    convert(t, t);
                }
            }
        }
        Kind::Audio => {
            for &t in AUDIO_TARGETS {
                if t != own {
                    convert(t, t);
                }
            }
        }
        _ => {}
    }

    let mut tool = |id: &str, label: &str| {
        out.push(Action { id: id.into(), label: label.into(), group: Group::Tool })
    };

    match kind {
        Kind::Image => {
            if matches!(own, "jpg" | "png" | "webp") || (own == "heic" && cfg!(target_os = "macos")) {
                tool("compress", "compress");
            }
            if matches!(own, "jpg" | "png") {
                tool("strip-metadata", "strip metadata");
            }
        }
        Kind::Video => {
            tool("audio", "pull out the audio");
            tool("compress", "compress");
            tool("strip-metadata", "strip metadata");
        }
        Kind::Audio => tool("strip-metadata", "strip metadata"),
        Kind::Pdf => {
            if many {
                tool("pdf:merge", "merge into one pdf");
            }
            tool("pdf:split", "split into pages");
            tool("pdf:rotate", "rotate 90°");
        }
        Kind::Archive => tool("unpack", "unpack"),
        _ => {}
    }

    if kind != Kind::Archive {
        tool("zip", if many { "zip them" } else { "zip" });
        if matches!(kind, Kind::Folder | Kind::Other) {
            tool("tar.gz", "tar.gz");
        }
    }

    out
}

/// Run an action from [`actions_for`]. Returns everything it made.
pub fn run(action: &str, inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<Vec<PathBuf>> {
    if inputs.is_empty() {
        bail!("no files given");
    }
    cancel::reset();
    let total = inputs.len();

    // actions that turn many inputs into one output
    let combined = match action {
        "to:pdf" => inputs.iter().all(|p| matches!(kind_of(p), Kind::Image | Kind::Vector)),
        "pdf:merge" => true,
        "zip" | "tar.gz" => total > 1,
        _ => false,
    };
    if combined {
        on(Event::Start { input: &inputs[0], index: 0, total: 1 });
        let output = match action {
            "to:pdf" => images::to_pdf(inputs, on)?,
            "pdf:merge" => pdf::merge(inputs, on)?,
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
        on(Event::Start { input, index, total });
        let output = run_one(action, input, on)?;
        on(Event::Progress(1.0));
        on(Event::Output(output.clone()));
        outputs.push(output);
    }
    Ok(outputs)
}

fn run_one(action: &str, input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let kind = kind_of(input);
    match (action, kind) {
        ("compress", Kind::Video) => ffmpeg::compress(input, on),
        ("compress", Kind::Image) => images::compress(input),
        ("strip-metadata", Kind::Video | Kind::Audio) => ffmpeg::strip_metadata(input, on),
        ("strip-metadata", Kind::Image) => images::strip_metadata(input),
        ("audio", Kind::Video) => ffmpeg::extract_audio(input, on),
        ("pdf:split", Kind::Pdf) => pdf::split(input, on),
        ("pdf:rotate", Kind::Pdf) => pdf::rotate(input),
        ("unpack", Kind::Archive) => archive::unpack(input),
        ("zip", _) => archive::zip(&[input.to_path_buf()]),
        ("tar.gz", _) => archive::tar_gz(&[input.to_path_buf()]),
        (a, k) if a.starts_with("to:") => {
            let target = &a[3..];
            match k {
                Kind::Image if paths::ext(input) == "gif" && matches!(target, "mp4" | "webm") => {
                    ffmpeg::convert(input, target, on)
                }
                Kind::Image | Kind::Vector => images::convert(input, target),
                Kind::Video | Kind::Audio => ffmpeg::convert(input, target, on),
                _ => bail!("can't turn {} into .{target}", paths::name(input)),
            }
        }
        _ => bail!("can't {action} {}", paths::name(input)),
    }
}

#[cfg(test)]
mod tests;
