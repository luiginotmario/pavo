//! Takes watermarks out of pdfs without touching anything else. A pdf watermark is almost always
//! its own object, so it can be removed exactly, with nothing redrawn:
//! - `/Watermark` annotations
//! - content marked as a watermark artifact (`/Artifact <</Subtype /Watermark>> BDC … EMC`)
//! - content on a layer (optional content group) named like "watermark"
//! - text or stamps drawn see-through *and* tilted, the classic diagonal "DRAFT"
//! - or any text the person asks for ("CONFIDENTIAL")

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, ensure, Result};
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::paths::{self, Staged};
use crate::{cancel, Event};

pub fn remove_from_pdf(input: &Path, text: Option<&str>, on: &mut dyn FnMut(Event)) -> Result<PathBuf> {
    let mut doc = Document::load(input).map_err(|e| anyhow!("couldn't read {}: {e}", paths::name(input)))?;
    ensure!(!doc.is_encrypted(), "{} is password-protected", paths::name(input));
    let wanted = text.map(normalise).filter(|t| !t.is_empty());
    let layers = watermark_layers(&doc);

    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let mut removed = 0;
    for (i, page) in pages.iter().enumerate() {
        cancel::check()?;
        on(Event::Progress(i as f64 / pages.len() as f64));
        removed += remove_annotations(&mut doc, *page);

        let resources = Resources::of(&doc, *page, &layers);
        let content = Content::decode(&doc.get_page_content(*page)).map_err(|e| anyhow!("{e}"))?;
        let (kept, dropped) = filter(content.operations, &resources, wanted.as_deref());
        if dropped > 0 {
            removed += dropped;
            let bytes = Content { operations: kept }.encode().map_err(|e| anyhow!("{e}"))?;
            doc.change_page_content(*page, bytes).map_err(|e| anyhow!("{e}"))?;
        }
    }

    ensure!(
        removed > 0,
        "couldn't find a watermark in {}{}",
        paths::name(input),
        if wanted.is_some() { " with that text" } else { ". try typing its text" }
    );
    doc.prune_objects();
    doc.compress();
    let staged = Staged::new(paths::output_for(input, "pdf", " (no watermark)"));
    doc.save(staged.path()).map_err(|e| anyhow!("{e}"))?;
    staged.commit()
}

/// Lowercase letters and digits only, so "C O N F I D E N T I A L" and "Confidential" match.
fn normalise(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn remove_annotations(doc: &mut Document, page: ObjectId) -> usize {
    let Ok(dict) = doc.get_dictionary(page) else { return 0 };
    let Ok(annots) = dict.get(b"Annots") else { return 0 };
    let list: Vec<Object> = match annots {
        Object::Array(list) => list.clone(),
        Object::Reference(id) => match doc.get_object(*id) {
            Ok(Object::Array(list)) => list.clone(),
            _ => return 0,
        },
        _ => return 0,
    };
    let is_watermark = |o: &Object| {
        let annot = match o {
            Object::Reference(id) => doc.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        };
        annot.and_then(|a| a.get(b"Subtype").ok()).and_then(|s| s.as_name().ok()) == Some(b"Watermark".as_slice())
    };
    let kept: Vec<Object> = list.iter().filter(|o| !is_watermark(o)).cloned().collect();
    let dropped = list.len() - kept.len();
    if dropped > 0 {
        if let Ok(page) = doc.get_dictionary_mut(page) {
            page.set("Annots", kept);
        }
    }
    dropped
}

/// Layers whose name says they're a watermark.
fn watermark_layers(doc: &Document) -> HashSet<ObjectId> {
    let mut layers = HashSet::new();
    for (id, object) in &doc.objects {
        let Ok(dict) = object.as_dict() else { continue };
        if dict.get(b"Type").and_then(Object::as_name).ok() != Some(b"OCG".as_slice()) {
            continue;
        }
        let name = dict.get(b"Name").and_then(Object::as_str).map(|n| String::from_utf8_lossy(n).to_lowercase());
        if name.is_ok_and(|n| n.contains("watermark")) {
            layers.insert(*id);
        }
    }
    layers
}

/// What a page's content needs to know: which graphics states are see-through, and which
/// marked-content names point at a watermark layer.
#[derive(Default)]
struct Resources {
    see_through: HashSet<Vec<u8>>,
    watermark_layers: HashSet<Vec<u8>>,
}

impl Resources {
    fn of(doc: &Document, page: ObjectId, layers: &HashSet<ObjectId>) -> Self {
        let mut out = Resources::default();
        let Ok((own, inherited)) = doc.get_page_resources(page) else { return out };
        let mut dicts: Vec<&Dictionary> = own.into_iter().collect();
        dicts.extend(inherited.iter().filter_map(|id| doc.get_dictionary(*id).ok()));
        let lookup = |o: &Object| -> Option<Dictionary> {
            match o {
                Object::Reference(id) => doc.get_dictionary(*id).ok().cloned(),
                Object::Dictionary(d) => Some(d.clone()),
                _ => None,
            }
        };
        for resources in dicts {
            if let Some(states) = resources.get(b"ExtGState").ok().and_then(lookup) {
                for (name, state) in states.iter() {
                    let Some(state) = lookup(state) else { continue };
                    let alpha = ["ca", "CA"]
                        .iter()
                        .filter_map(|k| state.get(k.as_bytes()).and_then(Object::as_float).ok())
                        .fold(1.0_f32, f32::min);
                    if alpha < 0.95 {
                        out.see_through.insert(name.clone());
                    }
                }
            }
            if let Some(properties) = resources.get(b"Properties").ok().and_then(lookup) {
                for (name, target) in properties.iter() {
                    if target.as_reference().is_ok_and(|id| layers.contains(&id)) {
                        out.watermark_layers.insert(name.clone());
                    }
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy, Default)]
struct State {
    see_through: bool,
    tilted: bool,
}

fn tilted(operands: &[Object]) -> bool {
    let b = operands.get(1).and_then(|o| o.as_float().ok()).unwrap_or(0.0);
    let c = operands.get(2).and_then(|o| o.as_float().ok()).unwrap_or(0.0);
    b.abs() > 0.01 || c.abs() > 0.01
}

/// Text a text-showing operation draws, as well as plain strings can be read.
fn shown_text(op: &Operation) -> String {
    let decode = |bytes: &[u8]| -> String {
        if bytes.starts_with(&[0xFE, 0xFF]) {
            let units: Vec<u16> = bytes[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        } else {
            bytes.iter().map(|&b| b as char).collect()
        }
    };
    op.operands
        .iter()
        .flat_map(|o| match o {
            Object::String(bytes, _) => vec![decode(bytes)],
            Object::Array(items) => items.iter().filter_map(|i| i.as_str().ok()).map(decode).collect(),
            _ => vec![],
        })
        .collect()
}

fn is_watermark_mark(op: &Operation, resources: &Resources) -> bool {
    if op.operator != "BDC" {
        return false;
    }
    let tag = op.operands.first().and_then(|o| o.as_name().ok());
    match (tag, op.operands.get(1)) {
        (Some(b"Artifact"), Some(Object::Dictionary(props))) => {
            props.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Watermark".as_slice())
        }
        (Some(b"OC"), Some(Object::Name(name))) => resources.watermark_layers.contains(name),
        _ => false,
    }
}

/// The page's drawing instructions minus the watermark, and how many pieces were taken out.
fn filter(ops: Vec<Operation>, resources: &Resources, wanted: Option<&str>) -> (Vec<Operation>, usize) {
    let mut kept = Vec::with_capacity(ops.len());
    let mut dropped = 0;
    let mut stack = vec![State::default()];
    let mut i = 0;
    while i < ops.len() {
        let op = &ops[i];
        let state = *stack.last().expect("never empty");

        // a whole marked watermark section: skip to its matching end
        if is_watermark_mark(op, resources) {
            let mut depth = 0;
            while i < ops.len() {
                match ops[i].operator.as_str() {
                    "BDC" | "BMC" => depth += 1,
                    "EMC" => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            dropped += 1;
            i += 1;
            continue;
        }

        match op.operator.as_str() {
            "q" => stack.push(state),
            "Q" if stack.len() > 1 => {
                stack.pop();
            }
            "cm" if tilted(&op.operands) => stack.last_mut().expect("never empty").tilted = true,
            "gs" => {
                let name = op.operands.first().and_then(|o| o.as_name().ok()).unwrap_or_default();
                if resources.see_through.contains(name) {
                    stack.last_mut().expect("never empty").see_through = true;
                }
            }
            "BT" => {
                // a text block: keep it unless it's a see-through tilted stamp or the asked-for text
                let end = ops[i..].iter().position(|o| o.operator == "ET").map_or(ops.len() - 1, |p| i + p);
                let block = &ops[i..=end];
                let tilted_text = block.iter().any(|o| o.operator == "Tm" && tilted(&o.operands));
                let text: String = block.iter().filter(|o| matches!(o.operator.as_str(), "Tj" | "TJ" | "'" | "\"")).map(shown_text).collect();
                let stamp = state.see_through && (state.tilted || tilted_text);
                let asked = wanted.is_some_and(|w| normalise(&text).contains(w));
                if stamp || asked {
                    dropped += 1;
                } else {
                    kept.extend_from_slice(block);
                }
                i = end + 1;
                continue;
            }
            "Do" if state.see_through && state.tilted => {
                dropped += 1; // a see-through, tilted image or form: a stamp
                i += 1;
                continue;
            }
            _ => {}
        }
        kept.push(op.clone());
        i += 1;
    }
    (kept, dropped)
}

#[cfg(test)]
pub(crate) fn sample(path: &Path) {
    // a page with real text, a see-through diagonal "DRAFT", a marked watermark artifact and a watermark annotation
    use lopdf::dictionary;
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
    let faint = doc.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.3, "CA" => 0.3 });
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font },
        "ExtGState" => dictionary! { "GS1" => faint },
    });
    let content = b"BT /F1 24 Tf 72 700 Td (Quarterly report) Tj ET \
        q /GS1 gs BT /F1 90 Tf 0.7071 0.7071 -0.7071 0.7071 150 250 Tm (DRAFT) Tj ET Q \
        /Artifact <</Subtype /Watermark /Type /Pagination>> BDC BT /F1 10 Tf 72 40 Td (Sample copy) Tj ET EMC \
        BT /F1 12 Tf 72 600 Td (CONFIDENTIAL - internal) Tj ET";
    let content_id = doc.add_object(lopdf::Stream::new(dictionary! {}, content.to_vec()));
    let annot = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Watermark", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()] });
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id, "Resources" => resources,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()], "Annots" => vec![annot.into()],
    });
    doc.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}
