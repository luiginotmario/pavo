use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Lowercased extension. `archive.tar.gz` → `tar.gz`.
pub fn ext(path: &Path) -> String {
    let name = name(path).to_lowercase();
    if name.ends_with(".tar.gz") {
        return "tar.gz".into();
    }
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into())
}

/// The name without its extension. Folders keep their whole name.
pub fn base(path: &Path) -> String {
    let name = name(path);
    if path.is_dir() {
        return name;
    }
    let ext = ext(path);
    if ext.is_empty() {
        return name;
    }
    name[..name.len() - ext.len() - 1].to_string()
}

fn dir(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// `clip.mov` + `mp4` → `clip.mp4`, or `clip 2.mp4` if that's taken. Never overwrites.
pub fn output_for(input: &Path, ext: &str, suffix: &str) -> PathBuf {
    unique(dir(input), &format!("{}{suffix}", base(input)), Some(ext))
}

/// A folder next to `input`, e.g. `report pages`.
pub fn folder_for(input: &Path, name: &str) -> PathBuf {
    unique(dir(input), name, None)
}

pub fn unique(dir: &Path, base: &str, ext: Option<&str>) -> PathBuf {
    (1..)
        .map(|n| {
            let stem = if n == 1 { base.to_string() } else { format!("{base} {n}") };
            dir.join(match ext {
                Some(ext) => format!("{stem}.{ext}"),
                None => stem,
            })
        })
        .find(|p| !p.exists())
        .expect("ran out of names")
}

/// Work happens in a hidden file next to the destination and is moved into
/// place only once it's complete, so a cancelled or failed job leaves nothing behind.
pub struct Staged {
    tmp: PathBuf,
    dest: PathBuf,
    done: bool,
}

impl Staged {
    pub fn new(dest: PathBuf) -> Self {
        // keep the extension at the end: ffmpeg picks the container from it
        let tmp = dest.with_file_name(format!(".cambio-{}", name(&dest)));
        Self { tmp, dest, done: false }
    }

    pub fn path(&self) -> &Path {
        &self.tmp
    }

    pub fn commit(mut self) -> Result<PathBuf> {
        fs::rename(&self.tmp, &self.dest).with_context(|| format!("couldn't save {}", name(&self.dest)))?;
        self.done = true;
        Ok(self.dest.clone())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if self.tmp.is_dir() {
            let _ = fs::remove_dir_all(&self.tmp);
        } else {
            let _ = fs::remove_file(&self.tmp);
        }
    }
}

/// A scratch file in the system temp folder, deleted when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(ext: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!("cambio-{}-{n}.{ext}", std::process::id())))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
