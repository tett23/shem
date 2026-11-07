//! The module for the browser: functions with the C ABI over integers, for
//! `wasm32-unknown-unknown` (docs/adr/0018).
//!
//! The state is a [`Module`]: the inputs that the page writes the PDF to,
//! the open documents with the last image of each, and the last error. The
//! functions are plain Rust, tested natively; on wasm32, `extern "C"`
//! functions named `shem_*` call them on one module of the thread. Inputs
//! are found by their pointer among the buffers of the module, so no raw
//! pointer is read.

use crate::font::FontCache;
use crate::pdf::{Document, Page};
use crate::render::render_page_with;
use std::collections::BTreeMap;

/// The resolution, as `shem render --dpi` takes it (docs/adr/0007).
const DPI: std::ops::RangeInclusive<u32> = 1..=1200;

/// An open document: its pages read once, the cache of its fonts, and the
/// RGBA of its last rendered page.
struct Opened {
    doc: Document,
    pages: Vec<Page>,
    dpi: u32,
    fonts: FontCache,
    image: Vec<u8>,
    width: u32,
    height: u32,
}

/// The state of the module: the inputs, the open documents and the last
/// error.
#[derive(Default)]
pub struct Module {
    inputs: BTreeMap<usize, Vec<u8>>,
    docs: BTreeMap<u32, Opened>,
    last: u32,
    error: String,
}

impl Module {
    pub fn new() -> Module {
        Module::default()
    }

    /// Returns 0 after keeping the message of an error.
    fn fail(&mut self, message: impl Into<String>) -> u32 {
        self.error = message.into();
        0
    }

    /// A new input of `len` bytes of 0, and its pointer.
    pub fn alloc(&mut self, len: usize) -> usize {
        let mut buffer = vec![0u8; len];
        let mut ptr = buffer.as_mut_ptr() as usize;
        // Inputs of no bytes share the dangling pointer of an empty Vec;
        // they get one byte each, so that every input has its own pointer.
        if self.inputs.contains_key(&ptr) {
            buffer = Vec::with_capacity(1);
            ptr = buffer.as_mut_ptr() as usize;
        }
        self.inputs.insert(ptr, buffer);
        ptr
    }

    /// The bytes of the input at `ptr`, for the native tests to write.
    pub fn input_mut(&mut self, ptr: usize) -> Option<&mut [u8]> {
        self.inputs.get_mut(&ptr).map(Vec::as_mut_slice)
    }

    pub fn free(&mut self, ptr: usize) {
        self.inputs.remove(&ptr);
    }

    /// Opens the first `len` bytes of the input at `ptr` as a PDF to render
    /// at `dpi`: the number of the document, from 1, or 0.
    pub fn open(&mut self, ptr: usize, len: usize, dpi: u32) -> u32 {
        if !DPI.contains(&dpi) {
            return self.fail(format!("dpi must be an integer from 1 to 1200: {dpi}"));
        }
        let Some(input) = self.inputs.get(&ptr) else {
            return self.fail(format!("no such input: {ptr}"));
        };
        let Some(data) = input.get(..len) else {
            let size = input.len();
            return self.fail(format!(
                "length {len} is more than the input of {size} bytes"
            ));
        };
        let opened = Document::parse(data).and_then(|doc| {
            let pages = doc.pages()?;
            Ok(Opened {
                doc,
                pages,
                dpi,
                fonts: FontCache::new(),
                image: Vec::new(),
                width: 0,
                height: 0,
            })
        });
        match opened {
            Ok(opened) => {
                self.last += 1;
                self.docs.insert(self.last, opened);
                self.last
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    /// The number of pages of a document, or 0 if there is no such document.
    pub fn page_count(&self, doc: u32) -> u32 {
        self.docs.get(&doc).map_or(0, |d| d.pages.len() as u32)
    }

    /// Renders page `page` (from 0) of a document into its image: 1, or 0.
    /// Only that page is checked.
    pub fn render(&mut self, doc: u32, page: u32) -> u32 {
        let Some(d) = self.docs.get_mut(&doc) else {
            return self.fail(format!("no such document: {doc}"));
        };
        let Some(p) = d.pages.get(page as usize) else {
            let count = d.pages.len();
            return self.fail(format!(
                "page {} does not exist: the document has {count} pages",
                u64::from(page) + 1
            ));
        };
        match render_page_with(&d.doc, p, d.dpi, &d.fonts) {
            Ok(canvas) => {
                d.image.clear();
                d.image.extend(
                    canvas
                        .pixels
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .flat_map(|&[r, g, b]| [r, g, b, 255]),
                );
                d.width = canvas.width as u32;
                d.height = canvas.height as u32;
                1
            }
            Err(e) => self.fail(e.to_string()),
        }
    }

    /// The RGBA of the last rendered page of a document, its width and its
    /// height.
    pub fn image(&self, doc: u32) -> Option<(&[u8], u32, u32)> {
        self.docs
            .get(&doc)
            .map(|d| (d.image.as_slice(), d.width, d.height))
    }

    pub fn close(&mut self, doc: u32) {
        self.docs.remove(&doc);
    }

    /// The message of the last error.
    pub fn error(&self) -> &str {
        &self.error
    }
}

/// The exports of `shem.wasm`, on one module of the thread.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::Module;
    use std::cell::RefCell;

    thread_local! {
        static MODULE: RefCell<Module> = RefCell::new(Module::new());
    }

    fn with<T>(f: impl FnOnce(&mut Module) -> T) -> T {
        MODULE.with(|m| f(&mut m.borrow_mut()))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_alloc(len: u32) -> u32 {
        with(|m| m.alloc(len as usize) as u32)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_free(ptr: u32) {
        with(|m| m.free(ptr as usize))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_open(ptr: u32, len: u32, dpi: u32) -> u32 {
        with(|m| m.open(ptr as usize, len as usize, dpi))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_page_count(doc: u32) -> u32 {
        with(|m| m.page_count(doc))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_render(doc: u32, page: u32) -> u32 {
        with(|m| m.render(doc, page))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_image_ptr(doc: u32) -> u32 {
        with(|m| m.image(doc).map_or(0, |(rgba, _, _)| rgba.as_ptr() as u32))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_image_width(doc: u32) -> u32 {
        with(|m| m.image(doc).map_or(0, |(_, w, _)| w))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_image_height(doc: u32) -> u32 {
        with(|m| m.image(doc).map_or(0, |(_, _, h)| h))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_close(doc: u32) {
        with(|m| m.close(doc))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_error_ptr() -> u32 {
        with(|m| m.error().as_ptr() as u32)
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn shem_error_len() -> u32 {
        with(|m| m.error().len() as u32)
    }
}

#[cfg(test)]
mod tests;
