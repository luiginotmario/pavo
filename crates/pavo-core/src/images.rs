//! Pictures. Pure rust for the common formats; on a mac, HEIC/AVIF go through
//! `sips`, which ships with macOS and uses the system's own (hardware) codecs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, ensure, Context, Result};
use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage};

use crate::paths::{self, Scratch, Staged};
use crate::{cancel, Event, Level};

pub fn load(path: &Path) -> Result<DynamicImage> {
    let name = paths::name(path);
    match paths::ext(path).as_str() {
        "svg" => render_svg(path),
        "heic" | "heif" | "avif" => {
            // sips carries the photo's turn over as exif in the png, so it gets applied below
            let png = Scratch::new("png");
            sips(&["-s", "format", "png"], path, &png.0)?;
            decode(&png.0, &name)
        }
        _ => decode(path, &name),
    }
}

/// Reads an image the way it's shown: a phone photo saved sideways with a "turn me" tag
/// comes out upright.
fn decode(path: &Path, name: &str) -> Result<DynamicImage> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder().with_context(|| format!("couldn't read {name}"))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).with_context(|| format!("couldn't read {name}"))?;
    img.apply_orientation(orientation);
    Ok(img)
}

fn render_svg(path: &Path) -> Result<DynamicImage> {
    use resvg::{tiny_skia, usvg};

    let data = fs::read(path)?;
    let mut opt = usvg::Options::default();
    // fonts are only worth loading when the svg actually has text in it
    if data.windows(5).any(|w| w == b"<text") {
        opt.fontdb_mut().load_system_fonts();
    }
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| anyhow!("couldn't read {}: {e}", paths::name(path)))?;

    // icons are often tiny; render them at a useful size
    let size = tree.size();
    let longest = size.width().max(size.height());
    let scale = if longest < 1024.0 { 1024.0 / longest } else { 1.0 };
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);

    let mut pixmap = tiny_skia::Pixmap::new(w, h).context("svg is too big")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, rgba).context("svg render failed")?))
}

/// JPEG has no transparency, so transparent pixels land on white instead of black.
fn flatten(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let a = a as u32;
        let blend = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        image::Rgb([blend(r), blend(g), blend(b)])
    })
}

fn jpeg_bytes(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality).encode_image(&flatten(img))?;
    Ok(out)
}

pub(crate) fn save(img: &DynamicImage, path: &Path, ext: &str, quality: u8) -> Result<()> {
    match ext {
        "jpg" | "jpeg" => fs::write(path, jpeg_bytes(img, quality)?)?,
        "png" => img.save_with_format(path, ImageFormat::Png)?,
        "webp" => {
            let rgba = img.to_rgba8();
            let encoded = webp::Encoder::from_rgba(&rgba, rgba.width(), rgba.height()).encode(quality as f32);
            fs::write(path, &*encoded)?;
        }
        "tiff" | "tif" => img.save_with_format(path, ImageFormat::Tiff)?,
        "bmp" => DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(path, ImageFormat::Bmp)?,
        "gif" => DynamicImage::ImageRgba8(img.to_rgba8()).save_with_format(path, ImageFormat::Gif)?,
        "heic" | "heif" | "avif" => {
            let format = if ext == "avif" { "avif" } else { "heic" };
            let png = Scratch::new("png");
            img.save_with_format(&png.0, ImageFormat::Png)?;
            sips(&["-s", "format", format, "-s", "formatOptions", &quality.to_string()], &png.0, path)?;
        }
        "pdf" => {
            let mut pdf = PdfWriter::default();
            pdf.add_image(img)?;
            fs::write(path, pdf.finish())?;
        }
        other => bail!("can't make .{other} images"),
    }
    Ok(())
}

pub fn convert(input: &Path, ext: &str) -> Result<PathBuf> {
    let img = load(input)?;
    let staged = Staged::new(paths::output_for(input, ext, ""));
    save(&img, staged.path(), ext, 90)?;
    staged.commit()
}

/// The subject of the photo cut out by Apple's Vision framework (the model behind "lift subject"
/// in Photos), on transparency or on white. Runs on the Neural Engine; pavo-vision exits when done.
pub fn cut_out(input: &Path, white: bool) -> Result<PathBuf> {
    let helper = vision_helper()?;
    let (mode, ext, label) = if white {
        ("white-background", "jpg", " (white background)")
    } else {
        ("remove-background", "png", " (no background)")
    };
    let staged = Staged::new(paths::output_for(input, ext, label));
    let out = Command::new(&helper).arg(mode).arg(input).arg(staged.path()).output()?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        bail!("{}", if why.is_empty() { format!("couldn't cut out {}", paths::name(input)) } else { why });
    }
    staged.commit()
}

/// Ships next to the pavo binary inside the app (Contents/Helpers/pavo-vision).
pub(crate) fn vision_helper() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("PAVO_VISION") {
        return Ok(path.into());
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("pavo-vision")))
        .filter(|p| p.is_file())
        .context("background removal needs pavo-vision, which comes with the mac app")
}

/// Cuts the biggest `w`:`h` area out of the middle of the picture.
pub fn crop(input: &Path, w: u32, h: u32) -> Result<PathBuf> {
    let ext = paths::ext(input);
    let img = load(input)?;
    let (iw, ih) = (img.width(), img.height());
    let (cw, ch) = if iw as u64 * h as u64 > ih as u64 * w as u64 {
        ((ih as u64 * w as u64 / h as u64) as u32, ih)
    } else {
        (iw, (iw as u64 * h as u64 / w as u64) as u32)
    };
    let label = if w == h { " (square)".to_string() } else { format!(" ({w}x{h})") };
    let staged = Staged::new(paths::output_for(input, &ext, &label));
    save(&img.crop_imm((iw - cw) / 2, (ih - ch) / 2, cw, ch), staged.path(), &ext, 92)?;
    staged.commit()
}

/// Turns the picture a quarter turn clockwise.
pub fn rotate(input: &Path) -> Result<PathBuf> {
    let ext = paths::ext(input);
    let staged = Staged::new(paths::output_for(input, &ext, " (rotated)"));
    save(&load(input)?.rotate90(), staged.path(), &ext, 92)?;
    staged.commit()
}

/// Half the width and height: a quarter of the pixels.
pub fn shrink(input: &Path) -> Result<PathBuf> {
    let ext = paths::ext(input);
    let img = load(input)?;
    ensure!(img.width() >= 32 && img.height() >= 32, "{} is already tiny", paths::name(input));
    let staged = Staged::new(paths::output_for(input, &ext, " (smaller)"));
    let half = img.resize_exact(img.width() / 2, img.height() / 2, image::imageops::FilterType::Lanczos3);
    save(&half, staged.path(), &ext, 90)?;
    staged.commit()
}

/// Every image becomes a page of one pdf, in the order they were dropped.
pub fn to_pdf(inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    if inputs.iter().any(|p| crate::kind_of(p) == crate::Kind::Pdf) {
        return with_pdfs(inputs, on);
    }
    let mut pdf = PdfWriter::default();
    for (i, input) in inputs.iter().enumerate() {
        cancel::check()?;
        on(Event::Progress(i as f64 / inputs.len() as f64));
        pdf.add_image(&load(input)?)?;
    }
    let staged = Staged::new(paths::output_for(&inputs[0], "pdf", ""));
    fs::write(staged.path(), pdf.finish())?;
    staged.commit()
}

/// Pictures and pdfs mixed: each picture becomes a page, each pdf keeps its pages, in the
/// order they were picked.
fn with_pdfs(inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let mut pages = Vec::with_capacity(inputs.len());
    let mut scratch = Vec::new();
    for input in inputs {
        cancel::check()?;
        if crate::kind_of(input) == crate::Kind::Pdf {
            pages.push(input.clone());
            continue;
        }
        let mut pdf = PdfWriter::default();
        pdf.add_image(&load(input)?)?;
        let page = Scratch::new("pdf");
        fs::write(&page.0, pdf.finish())?;
        pages.push(page.0.clone());
        scratch.push(page);
    }
    crate::pdf::combine(&pages, &inputs[0], "", on)
}

pub fn compress(input: &Path, level: Level) -> Result<PathBuf> {
    let ext = paths::ext(input);
    let staged = Staged::new(paths::output_for(input, &ext, " (compressed)"));
    // smallest also brings huge photos down to a size that still fills a screen
    let fit = |img: DynamicImage| match level {
        Level::Smallest if img.width().max(img.height()) > 2560 => {
            img.resize(2560, 2560, image::imageops::FilterType::Lanczos3)
        }
        _ => img,
    };
    match ext.as_str() {
        "png" => {
            let source = if level == Level::Smallest {
                let mut buffer = std::io::Cursor::new(Vec::new());
                fit(load(input)?).write_to(&mut buffer, ImageFormat::Png)?;
                buffer.into_inner()
            } else {
                fs::read(input)?
            };
            let mut opts = oxipng::Options::from_preset(level.pick(2, 3, 5));
            opts.strip = oxipng::StripChunks::Safe;
            let out = oxipng::optimize_from_memory(&source, &opts).map_err(|e| anyhow!("{e}"))?;
            fs::write(staged.path(), out)?;
        }
        "jpg" | "jpeg" | "webp" => save(&fit(load(input)?), staged.path(), &ext, level.pick(85, 72, 55))?,
        "heic" | "heif" | "avif" => {
            let quality = level.pick(70, 50, 35).to_string();
            let format = if ext == "avif" { "avif" } else { "heic" };
            if level == Level::Smallest {
                save(&fit(load(input)?), staged.path(), format, level.pick(70, 50, 35))?;
            } else {
                sips(&["-s", "format", format, "-s", "formatOptions", &quality], input, staged.path())?;
            }
        }
        other => bail!("can't compress .{other} yet"),
    }
    let (before, after) = (fs::metadata(input)?.len(), fs::metadata(staged.path())?.len());
    ensure!(after < before, "{} is already as small as it gets", paths::name(input));
    staged.commit()
}

/// Removes EXIF (camera, location, date), XMP and comments. Pixels stay untouched
/// unless the photo relies on EXIF to be shown the right way up.
pub fn strip_metadata(input: &Path) -> Result<PathBuf> {
    let ext = paths::ext(input);
    let staged = Staged::new(paths::output_for(input, &ext, " (clean)"));
    match ext.as_str() {
        "jpg" | "jpeg" => {
            let data = fs::read(input)?;
            let mut decoder = ImageReader::open(input)?.with_guessed_format()?.into_decoder()?;
            if decoder.orientation().unwrap_or(Orientation::NoTransforms) == Orientation::NoTransforms {
                fs::write(staged.path(), strip_jpeg(&data)?)?;
            } else {
                // bake the rotation into the pixels, since the tag that held it is going away
                fs::write(staged.path(), jpeg_bytes(&load(input)?, 95)?)?;
            }
        }
        "png" => {
            let mut opts = oxipng::Options::from_preset(0);
            opts.strip = oxipng::StripChunks::Safe;
            let out = oxipng::optimize_from_memory(&fs::read(input)?, &opts).map_err(|e| anyhow!("{e}"))?;
            fs::write(staged.path(), out)?;
        }
        other => bail!("can't strip .{other} yet"),
    }
    staged.commit()
}

/// Copies a jpeg segment by segment, leaving out the ones that carry metadata.
pub fn strip_jpeg(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.starts_with(&[0xFF, 0xD8]), "not a jpeg");
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    while i + 4 <= data.len() {
        ensure!(data[i] == 0xFF, "this jpeg looks damaged");
        let marker = data[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xDA {
            // start of scan: everything from here on is the picture itself
            out.extend_from_slice(&data[i..]);
            return Ok(out);
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let end = i + 2 + len;
        ensure!(end <= data.len(), "this jpeg looks damaged");
        let segment = &data[i..end];
        let keep = match marker {
            0xE2 => segment.get(4..16) == Some(b"ICC_PROFILE\0".as_slice()), // keep the color profile
            0xE1 | 0xE3..=0xED | 0xEF | 0xFE => false,                     // exif, xmp, iptc, comments…
            _ => true,
        };
        if keep {
            out.extend_from_slice(segment);
        }
        i = end;
    }
    bail!("this jpeg looks damaged")
}

fn sips(args: &[&str], input: &Path, output: &Path) -> Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("{} isn't supported on this system yet", paths::name(input));
    }
    let status = Command::new("/usr/bin/sips")
        .args(args)
        .arg(input)
        .arg("--out")
        .arg(output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    ensure!(status.success() && output.exists(), "couldn't convert {}", paths::name(input));
    Ok(())
}

/// The smallest pdf writer that does the job: one jpeg per page, no dependencies.
#[derive(Default)]
pub struct PdfWriter {
    objects: Vec<Vec<u8>>,
    pages: Vec<usize>,
}

impl PdfWriter {
    fn add(&mut self, body: Vec<u8>) -> usize {
        if self.objects.is_empty() {
            // 1 and 2 are the catalog and page tree, filled in by finish()
            self.objects = vec![vec![], vec![]];
        }
        self.objects.push(body);
        self.objects.len()
    }

    pub fn add_image(&mut self, img: &DynamicImage) -> Result<()> {
        let jpeg = jpeg_bytes(img, 90)?;
        let (w, h) = (img.width(), img.height());
        // longest side on an A4 sheet
        let scale = 842.0 / w.max(h) as f64;
        let (pw, ph) = (w as f64 * scale, h as f64 * scale);

        let mut xobject = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        xobject.extend(jpeg);
        xobject.extend(b"\nendstream");
        let image_id = self.add(xobject);

        let draw = format!("q {pw:.2} 0 0 {ph:.2} 0 0 cm /Im0 Do Q");
        let content_id = self.add(format!("<< /Length {} >>\nstream\n{draw}\nendstream", draw.len()).into_bytes());
        let page_id = self.add(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] \
                 /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>"
            )
            .into_bytes(),
        );
        self.pages.push(page_id);
        Ok(())
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
        let kids: Vec<String> = self.pages.iter().map(|p| format!("{p} 0 R")).collect();
        self.objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), self.pages.len()).into_bytes();

        let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::with_capacity(self.objects.len());
        for (i, body) in self.objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend(body);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", self.objects.len() + 1).as_bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", self.objects.len() + 1).as_bytes(),
        );
        out
    }
}
