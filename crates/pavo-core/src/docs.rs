//! Word, rtf, odt, html and plain text through `textutil` (ships with macOS), plus subtitles.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, ensure, Context, Result};

use crate::paths::{self, Scratch, Staged};
use crate::Event;

pub const TARGETS: &[&str] = &["docx", "doc", "rtf", "odt", "html", "txt", "pdf"];

/// Convert between document formats. `pdf` keeps the words but not the layout.
pub fn convert(input: &Path, ext: &str) -> Result<PathBuf> {
    let staged = Staged::new(paths::output_for(input, ext, ""));
    if ext == "pdf" {
        text_to_pdf(input, staged.path())?;
    } else {
        textutil(input, ext, staged.path())?;
    }
    staged.commit()
}

/// Text typeset onto pages, then each page saved as a picture.
pub fn text_to_images(input: &Path, ext: &str, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let pdf = Scratch::new("pdf");
    text_to_pdf(input, &pdf.0)?;
    crate::pdf::to_images(&pdf.0, input, ext, on)
}

/// Each line of text becomes a three-second subtitle, one after another.
pub fn text_to_subtitles(input: &Path, ext: &str) -> Result<PathBuf> {
    let text = fs::read_to_string(input)?;
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    ensure!(!lines.is_empty(), "{} is empty", paths::name(input));
    let stamp = |seconds: usize, sep: char| {
        format!("{:02}:{:02}:{:02}{sep}000", seconds / 3600, seconds / 60 % 60, seconds % 60)
    };
    let sep = if ext == "vtt" { '.' } else { ',' };
    let mut out = if ext == "vtt" { String::from("WEBVTT\n\n") } else { String::new() };
    for (i, line) in lines.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{line}\n\n", i + 1, stamp(i * 3, sep), stamp(i * 3 + 3, sep)));
    }
    let staged = Staged::new(paths::output_for(input, ext, ""));
    fs::write(staged.path(), out)?;
    staged.commit()
}

/// Plain words into a .docx (or any other textutil format).
pub fn text_into(text: &str, ext: &str, output: &Path) -> Result<()> {
    let txt = Scratch::new("txt");
    fs::write(&txt.0, text)?;
    textutil(&txt.0, ext, output)
}

fn textutil(input: &Path, ext: &str, output: &Path) -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "documents are mac-only for now");
    let mut cmd = Command::new("/usr/bin/textutil");
    if matches!(paths::ext(input).as_str(), "md" | "markdown") {
        cmd.args(["-format", "txt"]);
    }
    let status = cmd
        .args(["-convert", ext])
        .arg(input)
        .arg("-output")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    ensure!(status.success() && output.exists(), "couldn't convert {}", paths::name(input));
    Ok(())
}

fn text_to_pdf(input: &Path, output: &Path) -> Result<()> {
    let txt = Scratch::new("txt");
    let source = if paths::ext(input) == "txt" {
        input.to_path_buf()
    } else {
        textutil(input, "txt", &txt.0)?;
        txt.0.clone()
    };
    let out = Command::new("/usr/sbin/cupsfilter")
        .args(["-m", "application/pdf"])
        .arg(&source)
        .stderr(Stdio::null())
        .output()
        .context("couldn't start the pdf printer")?;
    ensure!(out.status.success() && out.stdout.starts_with(b"%PDF"), "couldn't make a pdf of {}", paths::name(input));
    fs::write(output, out.stdout)?;
    Ok(())
}

// --- subtitles ---------------------------------------------------------------------------

pub fn convert_subtitles(input: &Path, ext: &str) -> Result<PathBuf> {
    let text = fs::read_to_string(input)?.replace("\r\n", "\n");
    let out = match (paths::ext(input).as_str(), ext) {
        ("srt", "vtt") => srt_to_vtt(&text),
        ("vtt", "srt") => vtt_to_srt(&text),
        (_, "txt") => subtitle_text(&text),
        (from, to) => bail!("can't turn .{from} into .{to}"),
    };
    let staged = Staged::new(paths::output_for(input, ext, ""));
    fs::write(staged.path(), out)?;
    staged.commit()
}

fn is_timing(line: &str) -> bool {
    line.contains("-->")
}

pub fn srt_to_vtt(srt: &str) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for line in srt.lines() {
        if is_timing(line) {
            out.push_str(&line.replace(',', "."));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

pub fn vtt_to_srt(vtt: &str) -> String {
    let mut out = String::new();
    let mut cue = 0;
    for block in vtt.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
        let lines: Vec<&str> = block.lines().collect();
        let Some(timing) = lines.iter().position(|l| is_timing(l)) else { continue }; // header, NOTE, STYLE
        cue += 1;
        let (start, end) = lines[timing].split_once("-->").expect("checked above");
        let end = end.split_whitespace().next().unwrap_or(""); // drop cue settings
        out.push_str(&format!("{cue}\n{} --> {}\n", srt_time(start.trim()), srt_time(end)));
        for line in &lines[timing + 1..] {
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

/// `01:02.500` or `00:01:02.500` → `00:01:02,500`
fn srt_time(t: &str) -> String {
    let t = t.replace('.', ",");
    if t.matches(':').count() == 1 {
        format!("00:{t}")
    } else {
        t
    }
}

/// Just the words people say, one line each.
pub fn subtitle_text(subs: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in subs.lines().map(str::trim) {
        let skip = line.is_empty()
            || is_timing(line)
            || line == "WEBVTT"
            || line.chars().all(|c| c.is_ascii_digit());
        if !skip && out.last() != Some(&line) {
            out.push(line);
        }
    }
    out.join("\n") + "\n"
}
