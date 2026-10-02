//! Video and audio, through ffmpeg. On a mac we lean on the hardware encoders
//! (VideoToolbox / AudioToolbox) so converting doesn't cook the battery.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;

use anyhow::{bail, ensure, Context, Result};

use crate::paths::{self, Staged};
use crate::{cancel, Event, Level};

const GIF_FILTER: &str = "fps=12,scale='min(640,iw)':-1:flags=lanczos,split[a][b];\
                          [a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4";

/// Bundled next to the pavo binary in releases; otherwise wherever it's installed.
/// GUI apps don't inherit the shell's PATH, so the usual homebrew spots are checked by hand.
pub fn binary() -> Result<PathBuf> {
    if let Some(p) = env::var_os("PAVO_FFMPEG") {
        return Ok(p.into());
    }
    let beside_us = env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("ffmpeg")));
    let path_dirs = env::var_os("PATH").map(|p| env::split_paths(&p).map(|d| d.join("ffmpeg")).collect::<Vec<_>>());
    beside_us
        .into_iter()
        .chain(["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"].map(PathBuf::from))
        .chain(path_dirs.unwrap_or_default())
        .find(|p| p.is_file())
        .context("video needs ffmpeg — install it with `brew install ffmpeg`")
}

fn has_encoder(ff: &Path, name: &str) -> bool {
    static LIST: OnceLock<String> = OnceLock::new();
    let list = LIST.get_or_init(|| {
        Command::new(ff)
            .args(["-hide_banner", "-encoders"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    });
    list.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

#[derive(Debug, Default, PartialEq)]
pub struct Probe {
    pub duration: Option<f64>,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub width: u32,
    pub height: u32,
}

fn probe(ff: &Path, input: &Path) -> Result<Probe> {
    let out = Command::new(ff).args(["-hide_banner", "-nostdin", "-i"]).arg(input).output()?;
    Ok(parse_probe(&String::from_utf8_lossy(&out.stderr)))
}

pub fn parse_probe(text: &str) -> Probe {
    let mut p = Probe::default();
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Duration: ") {
            p.duration = rest.split(',').next().and_then(parse_clock);
        }
        if !line.starts_with("Stream #") {
            continue;
        }
        let codec = |at: usize| line[at..].split([' ', ',']).next().map(str::to_string);
        if let Some(i) = line.find("Video: ") {
            if p.video.is_none() && !line.contains("(attached pic)") {
                p.video = codec(i + 7);
                let dims = line[i..].split([' ', ',']).find_map(|tok| {
                    let (w, h) = tok.split_once('x')?;
                    let (w, h) = (w.parse::<u32>().ok()?, h.parse::<u32>().ok()?);
                    (w >= 2 && h >= 2 && w < 20_000 && h < 20_000).then_some((w, h))
                });
                if let Some((w, h)) = dims {
                    (p.width, p.height) = (w, h);
                }
            }
        } else if let Some(i) = line.find("Audio: ") {
            if p.audio.is_none() {
                p.audio = codec(i + 7);
            }
        }
    }
    p
}

fn parse_clock(s: &str) -> Option<f64> {
    let mut parts = s.trim().split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

fn strs(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

/// H.264 that plays everywhere. `bits_per_pixel` sets the bitrate for the hardware encoder.
fn h264(ff: &Path, p: &Probe, bits_per_pixel: f64, max_width: Option<u32>) -> Vec<String> {
    let mut args = match max_width {
        Some(w) => vec!["-vf".into(), format!("scale='trunc(min({w},iw)/2)*2':-2")],
        None => strs(&["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"]),
    };
    if has_encoder(ff, "h264_videotoolbox") {
        args.extend(strs(&["-c:v", "h264_videotoolbox", "-allow_sw", "1"]));
        args.extend(["-b:v".into(), bitrate(p, bits_per_pixel, max_width)]);
    } else if has_encoder(ff, "libx264") {
        args.extend(strs(&["-c:v", "libx264", "-preset", "veryfast", "-crf", "22"]));
    } else {
        args.extend(strs(&["-c:v", "mpeg4", "-q:v", "3"]));
    }
    args.extend(strs(&["-pix_fmt", "yuv420p"]));
    args
}

fn bitrate(p: &Probe, bits_per_pixel: f64, max_width: Option<u32>) -> String {
    let (mut w, mut h) = (p.width.max(640) as f64, p.height.max(360) as f64);
    if let Some(max) = max_width {
        if w > max as f64 {
            h *= max as f64 / w;
            w = max as f64;
        }
    }
    format!("{}k", ((w * h * bits_per_pixel) / 1000.0).max(800.0) as u64)
}

fn aac(ff: &Path, kbps: u32) -> Vec<String> {
    let enc = if has_encoder(ff, "aac_at") { "aac_at" } else { "aac" };
    vec!["-c:a".into(), enc.into(), "-b:a".into(), format!("{kbps}k")]
}

fn audio_codec(ff: &Path, ext: &str) -> Result<Vec<String>> {
    Ok(match ext {
        "mp3" => {
            ensure!(has_encoder(ff, "libmp3lame"), "this ffmpeg can't make mp3s");
            strs(&["-c:a", "libmp3lame", "-q:a", "2"])
        }
        "m4a" => aac(ff, 192),
        "wav" => strs(&["-c:a", "pcm_s16le"]),
        "aiff" => strs(&["-c:a", "pcm_s16be"]),
        "flac" => strs(&["-c:a", "flac"]),
        "opus" => strs(&["-c:a", "libopus", "-b:a", "128k"]),
        "ogg" if has_encoder(ff, "libvorbis") => strs(&["-c:a", "libvorbis", "-q:a", "5"]),
        "ogg" => strs(&["-c:a", "libopus", "-b:a", "160k"]),
        "wma" => strs(&["-c:a", "wmav2", "-b:a", "192k"]),
        other => bail!("can't make .{other} audio"),
    })
}

pub fn convert(input: &Path, ext: &str, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    let from_gif = paths::ext(input) == "gif";
    let audio_only = matches!(ext, "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "wma");
    if audio_only {
        ensure!(p.audio.is_some(), "{} has no sound in it", paths::name(input));
    } else {
        ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));
    }

    let mut args = Vec::new();
    match ext {
        "mp4" | "mov" => {
            let video_ok = matches!(p.video.as_deref(), Some("h264" | "hevc"));
            let audio_ok = matches!(p.audio.as_deref(), None | Some("aac" | "mp3" | "alac"));
            args.extend(strs(&["-map", "0:v:0", "-map", "0:a?"]));
            if video_ok && audio_ok && !from_gif {
                // same codecs, new box: just repackage it, which is instant and lossless
                args.extend(strs(&["-c", "copy"]));
                if p.video.as_deref() == Some("hevc") {
                    args.extend(strs(&["-tag:v", "hvc1"]));
                }
            } else {
                args.extend(h264(&ff, &p, 4.0, None));
                args.extend(aac(&ff, 160));
            }
            if ext == "mp4" {
                args.extend(strs(&["-movflags", "+faststart"]));
            }
        }
        "mkv" => args.extend(strs(&["-map", "0:v", "-map", "0:a?", "-c", "copy"])),
        "webm" => {
            ensure!(has_encoder(&ff, "libvpx-vp9"), "this ffmpeg can't make webm");
            args.extend(strs(&[
                "-map", "0:v:0", "-map", "0:a?", "-c:v", "libvpx-vp9", "-crf", "33", "-b:v", "0", "-row-mt", "1",
                "-deadline", "good", "-cpu-used", "4", "-c:a", "libopus", "-b:a", "128k",
            ]));
        }
        "avi" => {
            args.extend(strs(&["-map", "0:v:0", "-map", "0:a?", "-c:v", "mpeg4", "-q:v", "4"]));
            if has_encoder(&ff, "libmp3lame") {
                args.extend(strs(&["-c:a", "libmp3lame", "-q:a", "3"]));
            } else {
                args.extend(strs(&["-c:a", "pcm_s16le"]));
            }
        }
        "wmv" => {
            args.extend(strs(&["-map", "0:v:0", "-map", "0:a?", "-c:v", "wmv2"]));
            args.extend(["-b:v".into(), bitrate(&p, 4.0, None)]);
            args.extend(strs(&["-c:a", "wmav2", "-b:a", "160k"]));
        }
        "gif" => args.extend(strs(&["-vf", GIF_FILTER, "-loop", "0", "-an"])),
        _ if audio_only => {
            args.extend(strs(&["-vn", "-map", "0:a:0"]));
            args.extend(audio_codec(&ff, ext)?);
        }
        other => bail!("can't make .{other} files"),
    }

    let staged = Staged::new(paths::output_for(input, ext, ""));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// The soundtrack on its own. AAC audio is copied out untouched as .m4a; anything else becomes an mp3.
pub fn extract_audio(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.audio.is_some(), "{} has no sound in it", paths::name(input));
    let (ext, mut args) = if p.audio.as_deref() == Some("aac") {
        ("m4a", strs(&["-c:a", "copy"]))
    } else {
        ("mp3", audio_codec(&ff, "mp3")?)
    };
    args.splice(0..0, strs(&["-vn", "-map", "0:a:0"]));
    let staged = Staged::new(paths::output_for(input, ext, ""));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// Smaller mp4: HEVC on the hardware encoder when there is one. Balanced caps at 1080p, smallest at 720p.
pub fn compress(input: &Path, level: Level, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));

    let width: u32 = level.pick(3840, 1920, 1280);
    let scale = format!("scale='trunc(min({width},iw)/2)*2':-2");
    let mut args = strs(&["-map", "0:v:0", "-map", "0:a?"]);
    if has_encoder(&ff, "hevc_videotoolbox") {
        args.extend(["-vf".into(), scale, "-c:v".into(), "hevc_videotoolbox".into()]);
        args.extend(["-b:v".into(), bitrate(&p, level.pick(2.2, 1.2, 0.8), Some(width))]);
        args.extend(strs(&["-tag:v", "hvc1", "-pix_fmt", "yuv420p"]));
    } else if has_encoder(&ff, "libx264") {
        args.extend(["-vf".into(), scale, "-c:v".into(), "libx264".into(), "-preset".into(), "veryfast".into()]);
        args.extend(["-crf".into(), level.pick("24", "28", "32").into(), "-pix_fmt".into(), "yuv420p".into()]);
    } else {
        args.extend(h264(&ff, &p, level.pick(3.0, 2.0, 1.3), Some(width)));
    }
    args.extend(aac(&ff, level.pick(160, 128, 96)));
    args.extend(strs(&["-movflags", "+faststart"]));

    let staged = Staged::new(paths::output_for(input, "mp4", " (compressed)"));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    let (before, after) = (fs::metadata(input)?.len(), fs::metadata(staged.path())?.len());
    ensure!(after < before, "{} is already as small as it gets", paths::name(input));
    staged.commit()
}

/// Drops location, device and every other tag. The streams themselves are copied, not re-encoded.
pub fn strip_metadata(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    let args = strs(&["-map", "0:v?", "-map", "0:a?", "-map_metadata", "-1", "-map_chapters", "-1", "-c", "copy"]);
    let staged = Staged::new(paths::output_for(input, &paths::ext(input), " (clean)"));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// A box on the video, as fractions of the frame measured from the top left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Region {
    fn right(&self) -> f64 {
        self.x + self.w
    }
    fn bottom(&self) -> f64 {
        self.y + self.h
    }
    fn overlaps(&self, other: &Region, slack: f64) -> bool {
        self.x - slack < other.right() && other.x - slack < self.right() && self.y - slack < other.bottom() && other.y - slack < self.bottom()
    }
    /// Pieces of one line of text: overlapping vertically, with a gap of no more than a few letters.
    fn same_line(&self, other: &Region) -> bool {
        let vertical = self.bottom().min(other.bottom()) - self.y.max(other.y);
        let gap = (other.x - self.right()).max(self.x - other.right());
        vertical > 0.5 * self.h.min(other.h) && gap < 3.0 * self.h.max(other.h)
    }
    fn union(&self, other: &Region) -> Region {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        Region { x, y, w: self.right().max(other.right()) - x, h: self.bottom().max(other.bottom()) - y }
    }
    fn padded(&self, by: f64) -> Region {
        let (px, py) = (self.w * by + 0.004, self.h * by + 0.004);
        Region { x: self.x - px, y: self.y - py, w: self.w + 2.0 * px, h: self.h + 2.0 * py }
    }
}

/// Paints over watermarks by rebuilding each box from the pixels around it (ffmpeg's removelogo).
/// Without boxes, it finds them: text that Vision reads in the same place all through the video,
/// and marks that stay perfectly still while the rest of the picture moves.
pub fn remove_watermark(input: &Path, regions: Option<Vec<Region>>, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some() && p.width > 0, "{} has no video in it", paths::name(input));
    let regions = match regions {
        Some(given) => given,
        None => find_watermarks(&ff, input, &p)?,
    };
    ensure!(!regions.is_empty(), "couldn't find a watermark in {}. try drawing a box around it", paths::name(input));

    // removelogo rebuilds every white pixel of a mask from the picture around it (lgpl, unlike delogo)
    let (fw, fh) = (p.width, p.height);
    let mask = paths::Scratch::new("png");
    let mut white = image::GrayImage::new(fw, fh);
    for r in &regions {
        let x0 = (r.x.max(0.0) * fw as f64) as u32;
        let y0 = (r.y.max(0.0) * fh as f64) as u32;
        let x1 = ((r.right().min(1.0)) * fw as f64).ceil() as u32;
        let y1 = ((r.bottom().min(1.0)) * fh as f64).ceil() as u32;
        for y in y0..y1.min(fh) {
            for x in x0..x1.min(fw) {
                white.put_pixel(x, y, image::Luma([255]));
            }
        }
    }
    white.save(&mask.0)?;
    let erase = vec![format!("removelogo=f={}", mask.0.display())];

    let ext = paths::ext(input);
    let ext = if matches!(ext.as_str(), "mov" | "mp4" | "m4v") { ext } else { "mp4".into() };
    let mut args = strs(&["-map", "0:v:0", "-map", "0:a?"]);
    let mut video = h264(&ff, &p, 4.0, None);
    video[1] = format!("{},{}", erase.join(","), video[1]);
    args.extend(video);
    args.extend(strs(&["-c:a", "copy", "-movflags", "+faststart"]));
    let staged = Staged::new(paths::output_for(input, &ext, " (no watermark)"));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// Where the watermarks are, from a handful of frames spread across the video.
pub fn find_watermarks(ff: &Path, input: &Path, p: &Probe) -> Result<Vec<Region>> {
    let duration = p.duration.unwrap_or(0.0);
    let times: Vec<f64> = [0.12, 0.3, 0.5, 0.7, 0.88].iter().map(|f| f * duration).collect();
    let mut frames = Vec::new();
    for (i, t) in times.iter().enumerate() {
        cancel::check()?;
        let frame = paths::Scratch::new(&format!("{i}.png"));
        let ok = Command::new(ff)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-ss", &format!("{t:.3}"), "-i"])
            .arg(input)
            .args(["-frames:v", "1", "-vf", "scale=640:-2"])
            .arg(&frame.0)
            .status()?
            .success();
        if ok && frame.0.exists() {
            frames.push(frame);
        }
    }
    ensure!(frames.len() >= 3, "{} is too short to look for a watermark", paths::name(input));

    let mut found = text_that_stays(&frames);
    found.extend(marks_that_stay(&frames));

    // merge pieces of the same watermark ("@pa" + "demo"), then give each a little room
    let mut merged: Vec<Region> = Vec::new();
    for region in found {
        merged.push(region);
        // keep folding until nothing touches anything else
        let mut changed = true;
        while changed {
            changed = false;
            'outer: for i in 0..merged.len() {
                for j in i + 1..merged.len() {
                    if merged[i].overlaps(&merged[j], 0.01) || merged[i].same_line(&merged[j]) {
                        let joined = merged[i].union(&merged[j]);
                        merged.remove(j);
                        merged[i] = joined;
                        changed = true;
                        break 'outer;
                    }
                }
            }
        }
    }
    Ok(merged.into_iter().map(|r| r.padded(0.15)).collect())
}

/// Text Vision reads in (nearly) the same place in most frames: captions move on, watermarks don't.
fn text_that_stays(frames: &[paths::Scratch]) -> Vec<Region> {
    let Ok(helper) = crate::images::vision_helper() else { return vec![] };
    let per_frame: Vec<Vec<Region>> = frames
        .iter()
        .map(|frame| {
            let out = Command::new(&helper).arg("find-text").arg(&frame.0).output();
            let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
            text.lines()
                .filter_map(|line| {
                    let field = |k: &str| {
                        let at = line.find(&format!("\"{k}\":"))? + k.len() + 3;
                        line[at..].split([',', '}']).next()?.trim().parse::<f64>().ok()
                    };
                    Some(Region { x: field("x")?, y: field("y")?, w: field("w")?, h: field("h")? })
                })
                .collect()
        })
        .collect();

    let needed = (frames.len() * 3).div_ceil(5); // in at least 3 of 5 frames
    let mut steady = Vec::new();
    for region in per_frame.iter().flatten() {
        let seen = per_frame.iter().filter(|boxes| boxes.iter().any(|b| b.overlaps(region, -region.w.min(region.h) * 0.3))).count();
        if seen >= needed && !steady.iter().any(|s: &Region| s.overlaps(region, 0.0)) {
            steady.push(*region);
        }
    }
    steady
}

/// Sharp details that stay pixel-for-pixel the same while the rest of the picture changes:
/// a logo. Skipped when the whole video barely moves, since then everything looks "still".
fn marks_that_stay(frames: &[paths::Scratch]) -> Vec<Region> {
    let images: Vec<image::GrayImage> = frames.iter().filter_map(|f| image::open(&f.0).ok()).map(|i| i.to_luma8()).collect();
    let Some(first) = images.first() else { return vec![] };
    let (w, h) = first.dimensions();
    if w < 32 || h < 32 || images.iter().any(|i| i.dimensions() != (w, h)) {
        return vec![];
    }

    // still pixels, and still pixels sitting on a sharp edge
    let (cols, rows) = (32u32, 18u32);
    let mut cells = vec![0u32; (cols * rows) as usize];
    let mut still = 0u64;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let values: Vec<i32> = images.iter().map(|i| i.get_pixel(x, y).0[0] as i32).collect();
            let spread = values.iter().max().unwrap() - values.iter().min().unwrap();
            if spread > 10 {
                continue;
            }
            still += 1;
            let at = |dx: i32, dy: i32| first.get_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32).0[0] as i32;
            let edge = (at(1, 0) - at(-1, 0)).abs() + (at(0, 1) - at(0, -1)).abs();
            if edge > 60 {
                cells[((y * rows / h) * cols + x * cols / w) as usize] += 1;
            }
        }
    }
    if still as f64 > 0.5 * (w * h) as f64 {
        return vec![]; // a mostly still video (a screen recording, a talking head): can't tell marks from content
    }

    // group busy cells that touch into boxes, and keep the small ones
    let cell_area = (w / cols) * (h / rows);
    let busy = |i: usize| cells[i] as f64 > cell_area as f64 * 0.04;
    let mut seen = vec![false; cells.len()];
    let mut boxes = Vec::new();
    for start in 0..cells.len() {
        if seen[start] || !busy(start) {
            continue;
        }
        let (mut min_c, mut max_c, mut min_r, mut max_r) = (u32::MAX, 0, u32::MAX, 0);
        let mut queue = vec![start];
        seen[start] = true;
        let mut count = 0;
        while let Some(i) = queue.pop() {
            count += 1;
            let (c, r) = (i as u32 % cols, i as u32 / cols);
            (min_c, max_c, min_r, max_r) = (min_c.min(c), max_c.max(c), min_r.min(r), max_r.max(r));
            for (dc, dr) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nc, nr) = (c as i32 + dc, r as i32 + dr);
                if nc < 0 || nr < 0 || nc >= cols as i32 || nr >= rows as i32 {
                    continue;
                }
                let n = (nr as u32 * cols + nc as u32) as usize;
                if !seen[n] && busy(n) {
                    seen[n] = true;
                    queue.push(n);
                }
            }
        }
        let area = ((max_c - min_c + 1) * (max_r - min_r + 1)) as f64 / (cols * rows) as f64;
        if area <= 0.08 && count >= 1 {
            boxes.push(Region {
                x: min_c as f64 / cols as f64,
                y: min_r as f64 / rows as f64,
                w: (max_c - min_c + 1) as f64 / cols as f64,
                h: (max_r - min_r + 1) as f64 / rows as f64,
            });
        }
    }
    boxes
}

fn is_audio_ext(ext: &str) -> bool {
    matches!(ext, "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "oga" | "opus" | "aiff" | "aif" | "wma")
}

/// Keeps `start`..`end` (seconds). Video is re-encoded so the cut lands on the exact frame.
pub fn trim(input: &Path, start: f64, end: f64, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    trim_to(input, start, end, " (trimmed)", on)
}

/// Two halves, `name (part 1)` and `name (part 2)`.
pub fn split(input: &Path, on: &mut dyn FnMut(Event)) -> Result<Vec<PathBuf>> {
    let ff = binary()?;
    let duration = probe(&ff, input)?.duration.context("couldn't tell how long this is")?;
    let half = duration / 2.0;
    let first = trim_to(input, 0.0, half, " (part 1)", &mut |e| {
        if let Event::Progress(f) = e {
            on(Event::Progress(f / 2.0))
        }
    })?;
    let second = trim_to(input, half, duration, " (part 2)", &mut |e| {
        if let Event::Progress(f) = e {
            on(Event::Progress(0.5 + f / 2.0))
        }
    })?;
    Ok(vec![first, second])
}

/// The biggest `w`:`h` area from the middle of the frame.
pub fn crop(input: &Path, w: u32, h: u32, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));
    let ratio = w as f64 / h as f64;
    let ext = paths::ext(input);
    let ext = if matches!(ext.as_str(), "mov" | "mp4" | "m4v") { ext } else { "mp4".into() };
    let mut args = strs(&["-map", "0:v:0", "-map", "0:a?"]);
    let mut video = h264(&ff, &p, 4.0, None);
    video[1] = format!(
        "crop=w='if(gt(iw/ih,{ratio}),ih*{ratio},iw)':h='if(gt(iw/ih,{ratio}),ih,iw/{ratio})',{}",
        video[1]
    );
    args.extend(video);
    args.extend(strs(&["-c:a", "copy", "-movflags", "+faststart"]));
    let label = if w == h { " (square)".to_string() } else { format!(" ({w}x{h})") };
    let staged = Staged::new(paths::output_for(input, &ext, &label));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// Several videos (or several audio files) one after another. Videos take the first one's size.
pub fn join(inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    ensure!(inputs.len() > 1, "join needs at least two files");
    let ff = binary()?;
    let probes = inputs.iter().map(|i| probe(&ff, i)).collect::<Result<Vec<_>>>()?;
    let total: f64 = probes.iter().filter_map(|p| p.duration).sum();
    let video = probes.iter().all(|p| p.video.is_some());
    let audio = probes.iter().all(|p| p.audio.is_some());
    ensure!(video || audio, "these files can't be joined");

    let mut graph = String::new();
    let mut streams = String::new();
    let (w, h) = (probes[0].width.max(2) / 2 * 2, probes[0].height.max(2) / 2 * 2);
    for i in 0..inputs.len() {
        if video {
            graph.push_str(&format!(
                "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{i}];"
            ));
            streams.push_str(&format!("[v{i}]"));
        }
        if audio {
            graph.push_str(&format!("[{i}:a]aresample=48000,aformat=channel_layouts=stereo[a{i}];"));
            streams.push_str(&format!("[a{i}]"));
        }
    }
    graph.push_str(&format!("{streams}concat=n={}:v={}:a={}", inputs.len(), video as u8, audio as u8));
    graph.push_str(match (video, audio) {
        (true, true) => "[v][a]",
        (true, false) => "[v]",
        _ => "[a]",
    });

    let mut args = vec!["-filter_complex".to_string(), graph];
    let ext = if video {
        args.extend(strs(&["-map", "[v]"]));
        if audio {
            args.extend(strs(&["-map", "[a]"]));
        }
        args.extend(h264(&ff, &probes[0], 4.0, None).split_off(2)); // the graph already sized it
        if audio {
            args.extend(aac(&ff, 160));
        }
        args.extend(strs(&["-movflags", "+faststart"]));
        "mp4".to_string()
    } else {
        let ext = paths::ext(&inputs[0]);
        let ext = if audio_codec(&ff, &ext).is_ok() { ext } else { "m4a".into() };
        args.extend(strs(&["-map", "[a]"]));
        args.extend(audio_codec(&ff, &ext)?);
        ext
    };

    let refs: Vec<&Path> = inputs.iter().map(PathBuf::as_path).collect();
    let staged = Staged::new(paths::output_for(&inputs[0], &ext, " (joined)"));
    run_inputs(&ff, &[], &refs, staged.path(), &args, Some(total), on)?;
    staged.commit()
}

fn trim_to(input: &Path, start: f64, end: f64, label: &str, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    let end = p.duration.map_or(end, |d| end.min(d));
    ensure!(start >= 0.0 && end > start, "the end has to come after the start");

    let ext = paths::ext(input);
    let (out_ext, args) = if is_audio_ext(&ext) {
        let ext = if ext == "aac" { "m4a".to_string() } else { ext };
        let codec = audio_codec(&ff, &ext).unwrap_or_else(|_| strs(&["-c:a", "copy"]));
        (ext, [strs(&["-map", "0:a:0"]), codec].concat())
    } else {
        ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));
        let ext = if matches!(ext.as_str(), "mov" | "mp4" | "m4v") { ext } else { "mp4".into() };
        let mut args = strs(&["-map", "0:v:0", "-map", "0:a?"]);
        args.extend(h264(&ff, &p, 4.0, None));
        args.extend(aac(&ff, 160));
        args.extend(strs(&["-movflags", "+faststart"]));
        (ext, args)
    };
    let before = vec!["-ss".into(), format!("{start:.3}"), "-t".into(), format!("{:.3}", end - start)];
    let staged = Staged::new(paths::output_for(input, &out_ext, label));
    run_from(&ff, &before, input, staged.path(), &args, Some(end - start), on)?;
    staged.commit()
}

/// The same video without its sound. Nothing is re-encoded.
pub fn mute(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));
    let args = strs(&["-map", "0:v", "-c", "copy", "-an"]);
    let staged = Staged::new(paths::output_for(input, &paths::ext(input), " (muted)"));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// A quarter turn clockwise.
pub fn rotate(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));
    let ext = paths::ext(input);
    let ext = if matches!(ext.as_str(), "mov" | "mp4" | "m4v") { ext } else { "mp4".into() };
    let mut args = strs(&["-map", "0:v:0", "-map", "0:a?"]);
    let mut video = h264(&ff, &p, 4.0, None);
    video[1] = format!("transpose=1,{}", video[1]); // rotate before the even-size scale
    args.extend(video);
    args.extend(strs(&["-c:a", "copy", "-movflags", "+faststart"]));
    let staged = Staged::new(paths::output_for(input, &ext, " (rotated)"));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    staged.commit()
}

/// A still from one second in (the very first frame is often black).
pub fn frame(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));
    let at = p.duration.map_or(0.0, |d| (d / 2.0).min(1.0));
    let before = vec!["-ss".into(), format!("{at:.3}")];
    let args = strs(&["-frames:v", "1", "-update", "1"]);
    let staged = Staged::new(paths::output_for(input, "png", " (frame)"));
    run_from(&ff, &before, input, staged.path(), &args, None, on)?;
    staged.commit()
}

/// Smaller audio as aac: 128 kbps (light), 96 (balanced, still fine for music), 64 (smallest, fine for voice).
pub fn compress_audio(input: &Path, level: Level, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.audio.is_some(), "{} has no sound in it", paths::name(input));
    let mut args = strs(&["-vn", "-map", "0:a:0"]);
    args.extend(aac(&ff, level.pick(128, 96, 64)));
    let staged = Staged::new(paths::output_for(input, "m4a", " (compressed)"));
    run(&ff, input, staged.path(), &args, p.duration, on)?;
    let (before, after) = (fs::metadata(input)?.len(), fs::metadata(staged.path())?.len());
    ensure!(after < before, "{} is already as small as it gets", paths::name(input));
    staged.commit()
}

fn run(ff: &Path, input: &Path, out: &Path, args: &[String], duration: Option<f64>, on: &mut dyn FnMut(Event)) -> Result<()> {
    run_from(ff, &[], input, out, args, duration, on)
}

/// `before` goes ahead of `-i`, where seeking is fast.
fn run_from(
    ff: &Path,
    before: &[String],
    input: &Path,
    out: &Path,
    args: &[String],
    duration: Option<f64>,
    on: &mut dyn FnMut(Event),
) -> Result<()> {
    run_inputs(ff, before, &[input], out, args, duration, on)
}

fn run_inputs(
    ff: &Path,
    before: &[String],
    inputs: &[&Path],
    out: &Path,
    args: &[String],
    duration: Option<f64>,
    on: &mut dyn FnMut(Event),
) -> Result<()> {
    cancel::check()?;
    let input = inputs[0];
    let mut command = Command::new(ff);
    command
        .args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"])
        .args(before);
    for path in inputs {
        command.arg("-i").arg(path);
    }
    let mut child = command
        .args(args)
        .arg(out)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't start ffmpeg")?;
    cancel::set_child(child.id());
    if cancel::check().is_err() {
        cancel::cancel();
    }

    let mut stderr = child.stderr.take().expect("piped");
    let errors = thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });

    let stdout = BufReader::new(child.stdout.take().expect("piped"));
    for line in stdout.lines().map_while(Result::ok) {
        let Some(us) = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms=")) else {
            continue;
        };
        if let (Ok(us), Some(total)) = (us.trim().parse::<f64>(), duration) {
            if total > 0.0 {
                on(Event::Progress((us / 1e6 / total).clamp(0.0, 0.99)));
            }
        }
    }

    let status = child.wait()?;
    cancel::clear_child();
    let errors = errors.join().unwrap_or_default();
    cancel::check()?;
    if !status.success() {
        let why = errors.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        bail!("ffmpeg couldn't convert {}: {why}", paths::name(input));
    }
    Ok(())
}
