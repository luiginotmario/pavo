//! Merge, split and rotate shuffle pages without re-rendering, so nothing loses quality.
//! Pdf → images draws pages with macOS's own pdf engine; compress re-encodes the photos inside.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use image::{DynamicImage, ImageFormat};
use lopdf::{Document, Object, ObjectId, Stream};

use crate::paths::{self, Staged};
use crate::{docs, images};
use crate::{cancel, Event, Level};

fn open(path: &Path) -> Result<Document> {
    let doc = Document::load(path).map_err(|e| anyhow!("couldn't read {}: {e}", paths::name(path)))?;
    ensure!(!doc.is_encrypted(), "{} is password-protected", paths::name(path));
    Ok(doc)
}

fn save(doc: &mut Document, path: &Path) -> Result<()> {
    doc.compress();
    doc.save(path).map_err(|e| anyhow!("couldn't save {}: {e}", paths::name(path)))?;
    Ok(())
}

pub fn rotate(input: &Path) -> Result<PathBuf> {
    let mut doc = open(input)?;
    for (_, id) in doc.get_pages() {
        let page = doc.get_object_mut(id).and_then(Object::as_dict_mut).map_err(|e| anyhow!("{e}"))?;
        let current = page.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0);
        page.set("Rotate", (current + 90) % 360);
    }
    let staged = Staged::new(paths::output_for(input, "pdf", " (rotated)"));
    save(&mut doc, staged.path())?;
    staged.commit()
}

/// Every page as its own pdf, in a folder next to the original.
pub fn split(input: &Path, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let mut doc = open(input)?;
    flatten_inherited(&mut doc);
    let pages: Vec<u32> = doc.get_pages().into_keys().collect();
    ensure!(pages.len() > 1, "{} only has one page", paths::name(input));

    let base = paths::base(input);
    let staged = Staged::new(paths::folder_for(input, &format!("{base} pages")));
    fs::create_dir(staged.path())?;
    for (i, &keep) in pages.iter().enumerate() {
        cancel::check()?;
        on(Event::Progress(i as f64 / pages.len() as f64));
        let mut page = doc.clone();
        let others: Vec<u32> = pages.iter().copied().filter(|&n| n != keep).collect();
        page.delete_pages(&others);
        page.prune_objects();
        save(&mut page, &staged.path().join(format!("{base} - page {keep}.pdf")))?;
    }
    staged.commit()
}

/// All the pdfs, one after another, in the order they were dropped.
pub fn merge(inputs: &[PathBuf], on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let mut next_id = 1;
    let mut pages: Vec<(ObjectId, Object)> = Vec::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();

    for (i, input) in inputs.iter().enumerate() {
        cancel::check()?;
        on(Event::Progress(i as f64 / inputs.len() as f64 * 0.8));
        let mut doc = open(input)?;
        flatten_inherited(&mut doc);
        doc.renumber_objects_with(next_id);
        next_id = doc.max_id + 1;
        for (_, id) in doc.get_pages() {
            pages.push((id, doc.get_object(id).map_err(|e| anyhow!("{e}"))?.clone()));
        }
        objects.extend(doc.objects);
    }

    let mut merged = Document::with_version("1.5");
    let mut catalog: Option<(ObjectId, Object)> = None;
    let mut root: Option<(ObjectId, lopdf::Dictionary)> = None;
    for (id, object) in objects {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" if catalog.is_none() => catalog = Some((id, object)),
            b"Pages" if root.is_none() => root = Some((id, object.as_dict().map_err(|e| anyhow!("{e}"))?.clone())),
            // other page trees, pages and bookmarks are rebuilt below
            b"Catalog" | b"Pages" | b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                merged.objects.insert(id, object);
            }
        }
    }
    let (root_id, mut root) = root.context("these pdfs have no pages")?;
    let (catalog_id, catalog) = catalog.context("these pdfs look damaged")?;

    for (id, page) in &pages {
        let mut page = page.as_dict().map_err(|e| anyhow!("{e}"))?.clone();
        page.set("Parent", root_id);
        merged.objects.insert(*id, Object::Dictionary(page));
    }
    root.set("Count", pages.len() as i64);
    root.set("Kids", pages.iter().map(|(id, _)| Object::Reference(*id)).collect::<Vec<_>>());
    root.remove(b"Parent");
    merged.objects.insert(root_id, Object::Dictionary(root));

    let mut catalog = catalog.as_dict().map_err(|e| anyhow!("{e}"))?.clone();
    catalog.set("Pages", root_id);
    catalog.remove(b"Outlines");
    merged.objects.insert(catalog_id, Object::Dictionary(catalog));
    merged.trailer.set("Root", catalog_id);
    merged.max_id = merged.objects.keys().map(|(n, _)| *n).max().unwrap_or(0);
    merged.renumber_objects();

    on(Event::Progress(0.9));
    let staged = Staged::new(paths::output_for(&inputs[0], "pdf", " (merged)"));
    save(&mut merged, staged.path())?;
    staged.commit()
}

/// Pages can inherit their size and resources from the page tree above them. Copy those
/// down onto each page so it survives being moved into a different document.
fn flatten_inherited(doc: &mut Document) {
    const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
    for id in doc.get_pages().into_values().collect::<Vec<_>>() {
        let Ok(page) = doc.get_dictionary(id) else { continue };
        let missing: Vec<&[u8]> = INHERITABLE.into_iter().filter(|k| !page.has(k)).collect();
        let mut found: Vec<(Vec<u8>, Object)> = Vec::new();
        let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
        for _ in 0..32 {
            let Some(node) = parent.and_then(|p| doc.get_dictionary(p).ok()) else { break };
            for key in &missing {
                if !found.iter().any(|(k, _)| k == key) {
                    if let Ok(value) = node.get(key) {
                        found.push((key.to_vec(), value.clone()));
                    }
                }
            }
            parent = node.get(b"Parent").and_then(Object::as_reference).ok();
        }
        if let Ok(page) = doc.get_dictionary_mut(id) {
            for (key, value) in found {
                page.set(key, value);
            }
        }
    }
}

/// Every page as a picture at print quality (300 dpi). One page → one file; more → a folder.
/// Outputs are named after `name_from`, which is the pdf itself unless it was made on the fly.
pub fn to_images(pdf_path: &Path, name_from: &Path, ext: &str, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let input = name_from;
        let pdf = crate::render::Pdf::open(pdf_path)?;
        let count = pdf.pages();
        ensure!(count > 0, "{} has no pages", paths::name(input));
        let save = |page: usize, path: &Path| -> Result<()> {
            let img = DynamicImage::ImageRgba8(pdf.render(page, 300.0)?);
            match ext {
                "png" => img.save_with_format(path, ImageFormat::Png)?,
                _ => images::save(&img, path, ext, 90)?,
            }
            Ok(())
        };

        if count == 1 {
            let staged = Staged::new(paths::output_for(input, ext, ""));
            save(1, staged.path())?;
            return staged.commit();
        }
        let base = paths::base(input);
        let staged = Staged::new(paths::folder_for(input, &format!("{base} pages")));
        fs::create_dir(staged.path())?;
        for page in 1..=count {
            cancel::check()?;
            on(Event::Progress((page - 1) as f64 / count as f64));
            save(page, &staged.path().join(format!("{base} - page {page}.{ext}")))?;
        }
        staged.commit()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (pdf_path, name_from, ext, on);
        anyhow::bail!("pdf → image is mac-only for now")
    }
}

fn text_of(input: &Path) -> Result<String> {
    let doc = open(input)?;
    let pages: Vec<u32> = doc.get_pages().into_keys().collect();
    let mut text = String::new();
    for page in pages {
        cancel::check()?;
        if let Ok(words) = doc.extract_text(&[page]) {
            let lines: Vec<&str> = words.lines().map(str::trim_end).collect();
            text.push_str(lines.join("\n").trim_end());
            text.push_str("\n\n");
        }
    }
    ensure!(
        text.chars().any(char::is_alphanumeric),
        "{} has no text in it — it's probably a scan",
        paths::name(input)
    );
    Ok(text)
}

pub fn to_text(input: &Path) -> Result<PathBuf> {
    let text = text_of(input)?;
    let staged = Staged::new(paths::output_for(input, "txt", ""));
    fs::write(staged.path(), text)?;
    staged.commit()
}

/// The words of the pdf in a word document. Layout and pictures stay behind.
pub fn to_docx(input: &Path) -> Result<PathBuf> {
    let text = text_of(input)?;
    let staged = Staged::new(paths::output_for(input, "docx", ""));
    docs::text_into(&text, "docx", staged.path())?;
    staged.commit()
}

/// Shrinks the photos inside: anything bigger than the level's limit is scaled down and re-saved as jpeg.
pub fn compress(input: &Path, level: Level, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let longest: u32 = level.pick(2600, 2000, 1400);
    let quality: u8 = level.pick(75, 60, 45);
    let mut doc = open(input)?;
    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for (i, id) in ids.iter().enumerate() {
        cancel::check()?;
        if i % 50 == 0 {
            on(Event::Progress(i as f64 / ids.len() as f64 * 0.9));
        }
        let Some(components) = image_components(&doc, *id) else { continue };
        let Ok(Object::Stream(stream)) = doc.get_object(*id) else { continue };
        let Some(img) = decode_image(stream, components) else { continue };

        let img = if img.width().max(img.height()) > longest {
            img.resize(longest, longest, image::imageops::FilterType::Lanczos3)
        } else {
            img
        };
        let img = if components == 1 { DynamicImage::ImageLuma8(img.to_luma8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality).encode_image(&img)?;
        if jpeg.len() >= stream.content.len() {
            continue;
        }

        let Ok(Object::Stream(stream)) = doc.get_object_mut(*id) else { continue };
        stream.dict.set("Width", img.width() as i64);
        stream.dict.set("Height", img.height() as i64);
        stream.dict.set("BitsPerComponent", 8);
        stream.dict.set("Filter", "DCTDecode");
        stream.dict.remove(b"DecodeParms");
        stream.set_content(jpeg);
    }

    let staged = Staged::new(paths::output_for(input, "pdf", " (compressed)"));
    save(&mut doc, staged.path())?;
    let (before, after) = (fs::metadata(input)?.len(), fs::metadata(staged.path())?.len());
    ensure!(after < before, "{} is already as small as it gets", paths::name(input));
    staged.commit()
}

/// 1 (gray) or 3 (rgb) for an ordinary 8-bit image we know how to re-encode; None for anything else.
fn image_components(doc: &Document, id: ObjectId) -> Option<u8> {
    let Ok(Object::Stream(stream)) = doc.get_object(id) else { return None };
    let dict = &stream.dict;
    if dict.get(b"Subtype").and_then(Object::as_name).ok()? != b"Image"
        || dict.get(b"ImageMask").and_then(Object::as_bool).unwrap_or(false)
        || dict.has(b"Decode")
        || dict.get(b"BitsPerComponent").and_then(Object::as_i64).unwrap_or(8) != 8
    {
        return None;
    }
    let space = dict.get(b"ColorSpace").ok()?;
    let space = match space {
        Object::Reference(r) => doc.get_object(*r).ok()?,
        other => other,
    };
    match space {
        Object::Name(n) if n == b"DeviceRGB" => Some(3),
        Object::Name(n) if n == b"DeviceGray" => Some(1),
        Object::Array(a) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => {
            let profile = doc.get_object(a.get(1)?.as_reference().ok()?).ok()?.as_stream().ok()?;
            match profile.dict.get(b"N").and_then(Object::as_i64).ok()? {
                n @ (1 | 3) => Some(n as u8),
                _ => None,
            }
        }
        _ => None,
    }
}

fn decode_image(stream: &Stream, components: u8) -> Option<DynamicImage> {
    let filters = stream.filters().ok()?;
    match filters.as_slice() {
        [f] if f == b"DCTDecode" => image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg).ok(),
        [f] if f == b"FlateDecode" => {
            let w = stream.dict.get(b"Width").and_then(Object::as_i64).ok()? as u32;
            let h = stream.dict.get(b"Height").and_then(Object::as_i64).ok()? as u32;
            let raw = stream.decompressed_content().ok()?;
            match components {
                3 => image::RgbImage::from_raw(w, h, raw).map(DynamicImage::ImageRgb8),
                _ => image::GrayImage::from_raw(w, h, raw).map(DynamicImage::ImageLuma8),
            }
        }
        _ => None,
    }
}
