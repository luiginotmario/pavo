//! Pages, Numbers and Keynote files. Their format is Apple's own and nothing else renders it
//! faithfully, so pavo asks the app itself to export, exactly like File → Export. The app is
//! opened in the background and quit again afterwards if it wasn't already running.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::paths::{self, Staged};

pub const PAGES: &[&str] = &["pdf", "docx", "epub", "txt"];
pub const NUMBERS: &[&str] = &["pdf", "xlsx", "csv"];
pub const KEYNOTE: &[&str] = &["pdf", "pptx"];

pub fn export(input: &Path, ext: &str) -> Result<PathBuf> {
    let (app, script) = match paths::ext(input).as_str() {
        "pages" => ("Pages", include_str!("iwork/pages.applescript")),
        "numbers" => ("Numbers", include_str!("iwork/numbers.applescript")),
        "key" => ("Keynote", include_str!("iwork/keynote.applescript")),
        other => bail!("can't export .{other}"),
    };
    // the current apps are called "Pages Creator Studio" and so on; older Macs have plain "Pages"
    let installed = ["/Applications", "/System/Applications"]
        .iter()
        .flat_map(|dir| [format!("{dir}/{app}.app"), format!("{dir}/{app} Creator Studio.app")])
        .any(|path| Path::new(&path).exists());
    if !installed {
        bail!("{} needs {app}, which is free on the App Store", paths::name(input));
    }

    let staged = Staged::new(paths::output_for(input, ext, ""));
    let mut child = Command::new("/usr/bin/osascript")
        .arg("-")
        .arg(input)
        .arg(staged.path())
        .arg(ext)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("couldn't start osascript")?;
    child.stdin.take().expect("piped").write_all(script.as_bytes())?;
    let out = child.wait_with_output()?;

    if !out.status.success() || !staged.path().exists() {
        let why = String::from_utf8_lossy(&out.stderr);
        if why.contains("-1743") || why.contains("Not authorized") {
            bail!("pavo isn't allowed to use {app} yet: System Settings → Privacy & Security → Automation → Pavo → {app}");
        }
        let detail = why.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given");
        bail!("{app} couldn't export {}: {}", paths::name(input), detail.split("error: ").last().unwrap_or(detail).trim());
    }
    staged.commit()
}
