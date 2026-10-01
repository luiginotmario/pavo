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
use crate::{cancel, Event};

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
        other => bail!("can't make .{other} audio"),
    })
}

pub fn convert(input: &Path, ext: &str, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    let from_gif = paths::ext(input) == "gif";
    let audio_only = matches!(ext, "mp3" | "m4a" | "wav" | "flac" | "ogg" | "opus" | "aiff");
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

/// Smaller mp4: HEVC on the hardware encoder when there is one, capped at 1080p wide.
pub fn compress(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let ff = binary()?;
    let p = probe(&ff, input)?;
    ensure!(p.video.is_some(), "{} has no video in it", paths::name(input));

    let mut args = strs(&["-map", "0:v:0", "-map", "0:a?"]);
    if has_encoder(&ff, "hevc_videotoolbox") {
        args.extend(strs(&["-vf", "scale='trunc(min(1920,iw)/2)*2':-2", "-c:v", "hevc_videotoolbox"]));
        args.extend(["-b:v".into(), bitrate(&p, 1.2, Some(1920))]);
        args.extend(strs(&["-tag:v", "hvc1", "-pix_fmt", "yuv420p"]));
    } else if has_encoder(&ff, "libx264") {
        args.extend(strs(&["-vf", "scale='trunc(min(1920,iw)/2)*2':-2", "-c:v", "libx264", "-preset", "veryfast"]));
        args.extend(strs(&["-crf", "28", "-pix_fmt", "yuv420p"]));
    } else {
        args.extend(h264(&ff, &p, 2.0, Some(1920)));
    }
    args.extend(aac(&ff, 128));
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

fn run(ff: &Path, input: &Path, out: &Path, args: &[String], duration: Option<f64>, on: &mut dyn FnMut(Event)) -> Result<()> {
    cancel::check()?;
    let mut child = Command::new(ff)
        .args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats", "-i"])
        .arg(input)
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
