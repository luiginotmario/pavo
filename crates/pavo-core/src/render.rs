//! Renders pdf pages to pixels with macOS's own pdf engine (CoreGraphics), so pavo
//! doesn't ship a pdf renderer of its own.

use std::ffi::c_void;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use anyhow::{ensure, Context, Result};
use image::RgbaImage;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGAffineTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

type Ref = *const c_void;

const MEDIA_BOX: i32 = 0;
const CROP_BOX: i32 = 1;
const PREMULTIPLIED_LAST_BIG_ENDIAN: u32 = 1 | (4 << 12);
const HIGH_QUALITY: i32 = 3;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(alloc: Ref, buffer: *const u8, len: isize, is_dir: u8) -> Ref;
    fn CFRelease(cf: Ref);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPDFDocumentCreateWithURL(url: Ref) -> Ref;
    fn CGPDFDocumentGetNumberOfPages(doc: Ref) -> usize;
    fn CGPDFDocumentGetPage(doc: Ref, number: usize) -> Ref;
    fn CGPDFDocumentRelease(doc: Ref);
    fn CGPDFPageGetBoxRect(page: Ref, kind: i32) -> CGRect;
    fn CGPDFPageGetRotationAngle(page: Ref) -> i32;
    fn CGPDFPageGetDrawingTransform(page: Ref, kind: i32, rect: CGRect, rotate: i32, keep_aspect: bool) -> CGAffineTransform;
    fn CGColorSpaceCreateDeviceRGB() -> Ref;
    fn CGColorSpaceRelease(space: Ref);
    fn CGBitmapContextCreate(data: *mut c_void, w: usize, h: usize, bits: usize, row: usize, space: Ref, info: u32) -> Ref;
    fn CGContextRelease(ctx: Ref);
    fn CGContextSetRGBFillColor(ctx: Ref, r: f64, g: f64, b: f64, a: f64);
    fn CGContextFillRect(ctx: Ref, rect: CGRect);
    fn CGContextScaleCTM(ctx: Ref, sx: f64, sy: f64);
    fn CGContextConcatCTM(ctx: Ref, t: CGAffineTransform);
    fn CGContextSetInterpolationQuality(ctx: Ref, quality: i32);
    fn CGContextDrawPDFPage(ctx: Ref, page: Ref);
}

/// An open pdf. Pages are numbered from 1, like the pdf itself.
pub struct Pdf(Ref);

impl Pdf {
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = path.as_os_str().as_bytes();
        // SAFETY: CoreFoundation copies the path bytes; the url is released right after use
        let doc = unsafe {
            let url = CFURLCreateFromFileSystemRepresentation(std::ptr::null(), bytes.as_ptr(), bytes.len() as isize, 0);
            ensure!(!url.is_null(), "couldn't open {}", path.display());
            let doc = CGPDFDocumentCreateWithURL(url);
            CFRelease(url);
            doc
        };
        ensure!(!doc.is_null(), "couldn't read this pdf");
        Ok(Self(doc))
    }

    pub fn pages(&self) -> usize {
        // SAFETY: self.0 is a live CGPDFDocument
        unsafe { CGPDFDocumentGetNumberOfPages(self.0) }
    }

    /// Draws a page on white at `dpi`, the right way up.
    pub fn render(&self, number: usize, dpi: f64) -> Result<RgbaImage> {
        // SAFETY: the page is owned by the document, which outlives this call; the bitmap
        // context draws into `pixels`, which stays alive (and unmoved) until the context is released
        unsafe {
            let page = CGPDFDocumentGetPage(self.0, number);
            ensure!(!page.is_null(), "page {number} is missing");

            let mut rect = CGPDFPageGetBoxRect(page, CROP_BOX);
            if rect.w <= 0.0 || rect.h <= 0.0 {
                rect = CGPDFPageGetBoxRect(page, MEDIA_BOX);
            }
            let turned = CGPDFPageGetRotationAngle(page).rem_euclid(180) == 90;
            let (pw, ph) = if turned { (rect.h, rect.w) } else { (rect.w, rect.h) };

            // keep huge pages (posters, maps) to a sane bitmap
            let scale = (dpi / 72.0).min(10_000.0 / pw.max(ph));
            let (w, h) = ((pw * scale).round().max(1.0) as usize, (ph * scale).round().max(1.0) as usize);

            let mut pixels = vec![0u8; w * h * 4];
            let space = CGColorSpaceCreateDeviceRGB();
            let ctx = CGBitmapContextCreate(pixels.as_mut_ptr().cast(), w, h, 8, w * 4, space, PREMULTIPLIED_LAST_BIG_ENDIAN);
            CGColorSpaceRelease(space);
            ensure!(!ctx.is_null(), "page {number} is too big to render");

            CGContextSetRGBFillColor(ctx, 1.0, 1.0, 1.0, 1.0);
            CGContextFillRect(ctx, CGRect { x: 0.0, y: 0.0, w: w as f64, h: h as f64 });
            CGContextSetInterpolationQuality(ctx, HIGH_QUALITY);
            CGContextScaleCTM(ctx, scale, scale);
            let box_kind = if CGPDFPageGetBoxRect(page, CROP_BOX).w > 0.0 { CROP_BOX } else { MEDIA_BOX };
            let fit = CGPDFPageGetDrawingTransform(page, box_kind, CGRect { x: 0.0, y: 0.0, w: pw, h: ph }, 0, true);
            CGContextConcatCTM(ctx, fit);
            CGContextDrawPDFPage(ctx, page);
            CGContextRelease(ctx);

            RgbaImage::from_raw(w as u32, h as u32, pixels).context("couldn't render the page")
        }
    }
}

impl Drop for Pdf {
    fn drop(&mut self) {
        // SAFETY: we own the only reference
        unsafe { CGPDFDocumentRelease(self.0) }
    }
}
