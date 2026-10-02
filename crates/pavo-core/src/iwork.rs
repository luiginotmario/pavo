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
    let installed = ["/Applications", "/System/Applications"].iter().any(|dir| Path::new(dir).join(format!("{app}.app")).exists());
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
        bail!("{app} couldn't export {}", paths::name(input));
    }
    staged.commit()
}
