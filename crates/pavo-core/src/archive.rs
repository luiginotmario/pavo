use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use zip::write::SimpleFileOptions;

use crate::cancel;
use crate::paths::{self, Staged};

/// One input → `name.zip` beside it. Several → `Archive.zip` beside the first.
fn destination(inputs: &[PathBuf], ext: &str) -> PathBuf {
    match inputs {
        [one] => paths::output_for(one, ext, ""),
        _ => {
            let dir = inputs[0].parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            paths::unique(dir, "Archive", Some(ext))
        }
    }
}

pub fn zip(inputs: &[PathBuf]) -> Result<PathBuf> {
    let staged = Staged::new(destination(inputs, "zip"));
    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(staged.path())?));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for input in inputs {
        add_to_zip(&mut zip, input, &paths::name(input), options)?;
    }
    zip.finish()?;
    staged.commit()
}

fn add_to_zip(zip: &mut zip::ZipWriter<BufWriter<File>>, path: &Path, name: &str, options: SimpleFileOptions) -> Result<()> {
    cancel::check()?;
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        zip.add_directory(format!("{name}/"), options)?;
        let mut entries: Vec<_> = fs::read_dir(path)?.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let child = entry.file_name().to_string_lossy().into_owned();
            if child != ".DS_Store" {
                add_to_zip(zip, &entry.path(), &format!("{name}/{child}"), options)?;
            }
        }
    } else if meta.is_file() {
        zip.start_file(name, options)?;
        io::copy(&mut File::open(path)?, zip)?;
    }
    Ok(())
}

pub fn tar_gz(inputs: &[PathBuf]) -> Result<PathBuf> {
    let staged = Staged::new(destination(inputs, "tar.gz"));
    let gz = GzEncoder::new(BufWriter::new(File::create(staged.path())?), Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.follow_symlinks(false);
    for input in inputs {
        cancel::check()?;
        let name = paths::name(input);
        if input.is_dir() {
            tar.append_dir_all(&name, input)?;
        } else {
            tar.append_path_with_name(input, &name)?;
        }
    }
    tar.into_inner()?.finish()?;
    staged.commit()
}

/// Unpacks into a new folder named after the archive. Entries that try to escape it are refused.
pub fn unpack(input: &Path) -> Result<PathBuf> {
    let staged = Staged::new(paths::folder_for(input, &paths::base(input)));
    fs::create_dir(staged.path())?;
    match paths::ext(input).as_str() {
        "zip" => zip::ZipArchive::new(File::open(input)?)?.extract(staged.path())?,
        "tar" => tar::Archive::new(File::open(input)?).unpack(staged.path())?,
        "tar.gz" | "tgz" => tar::Archive::new(GzDecoder::new(File::open(input)?)).unpack(staged.path())?,
        other => bail!("can't unpack .{other} yet"),
    }
    staged.commit()
}
