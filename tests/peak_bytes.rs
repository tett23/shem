//! The Flate decoder stops where a byte would go past the limit of a
//! stream, so the memory of a hostile stream is bounded by the limit, not
//! by the data it would inflate to (docs/adr/0021).
//!
//! The allocator of this test counts the bytes allocated at each moment and
//! the largest such count. This file is a test binary of its own, so the
//! allocator counts nothing of the other tests; its tests run one at a
//! time, so that the peak is the one of the test that reads it.

use shem::image::decode;
use shem::pdf::Document;
use shem::png::{crc32, zlib};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Peak;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn add(bytes: usize) {
    let now = CURRENT.fetch_add(bytes, Ordering::SeqCst) + bytes;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

fn remove(bytes: usize) {
    CURRENT.fetch_sub(bytes, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        add(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        add(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // The old block lives until the new one is made.
        add(new_size);
        let out = unsafe { System.realloc(ptr, layout, new_size) };
        remove(layout.size());
        out
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        remove(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Peak = Peak;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Held for the whole of a test, from before it makes its input: the
/// allocator counts the process, so the input of one test (tens of
/// megabytes) would otherwise add to the peak of the other.
fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner())
}

/// The largest number of bytes allocated beyond those at the start, while
/// `f` runs. The caller holds [`one_at_a_time`].
fn peak_of<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = CURRENT.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let out = f();
    (out, PEAK.load(Ordering::SeqCst) - base)
}

/// A PDF of one page whose content is `z`, Flate-compressed.
fn flate_page_pdf(z: &[u8]) -> Vec<u8> {
    let mut stream =
        format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", z.len()).into_bytes();
    stream.extend_from_slice(z);
    stream.extend(b"\nendstream");
    let objects: [Vec<u8>; 4] = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] /Resources << >> /Contents 4 0 R >>"
            .to_vec(),
        stream,
    ];
    let mut out = b"%PDF-1.6\n".to_vec();
    let offsets: Vec<usize> = objects
        .iter()
        .enumerate()
        .map(|(i, body)| {
            let at = out.len();
            out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
            out.extend(body);
            out.extend(b"\nendobj\n");
            at
        })
        .collect();
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
    offsets
        .iter()
        .for_each(|o| out.extend(format!("{o:010} 00000 n \n").into_bytes()));
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .into_bytes(),
    );
    out
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    out.extend(crc32(&[&kind[..], data].concat()).to_be_bytes());
    out
}

/// A PNG of 8-bit RGB whose IDAT is `z`.
fn png(width: u32, height: u32, z: &[u8]) -> Vec<u8> {
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", z));
    out.extend(chunk(b"IEND", &[]));
    out
}

#[test]
fn a_stream_past_the_limit_allocates_at_most_twice_the_limit() {
    let _serial = one_at_a_time();
    let limit: usize = 1 << 26;
    let data = flate_page_pdf(&zlib(&vec![0u8; limit + 1]));
    let (error, peak) = peak_of(|| {
        Document::parse(&data)
            .unwrap()
            .pages()
            .unwrap_err()
            .to_string()
    });
    assert!(
        error.contains("decoded data larger than 67108864 bytes"),
        "{error}"
    );
    assert!(peak <= 2 * limit, "{peak} bytes");
}

#[test]
fn a_png_past_its_size_allocates_at_most_twice_its_size() {
    let _serial = one_at_a_time();
    let (width, height) = (200u32, 200u32);
    let size = height as usize * (width as usize * 3 + 1);
    let data = png(width, height, &zlib(&vec![0u8; 1_000_000]));
    let (error, peak) = peak_of(|| decode(&data).unwrap_err().to_string());
    assert_eq!(
        error,
        "malformed image: the image data does not have the size of the image"
    );
    assert!(peak <= 2 * size, "{peak} bytes for {size}");
}
