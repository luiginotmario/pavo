//! Merge, split and rotate — page shuffling, never re-rendering, so nothing loses quality.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Context, Result};
use lopdf::{Document, Object, ObjectId};

use crate::paths::{self, Staged};
use crate::{cancel, Event};

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
