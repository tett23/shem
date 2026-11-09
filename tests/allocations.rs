//! The check and the rendering of a page allocate a number of times that
//! does not grow with the glyphs, the segments and the curves of the page
//! (docs/adr/0013). musl's allocator takes one lock for every allocation,
//! so the threads of `--jobs` wait for each other on every one of them.
//!
//! The allocator of this test counts the allocations and reallocations of
//! each thread. This file is a test binary of its own, so the allocator
//! counts nothing of the other tests.

mod common;

use shem::font::FontCache;
use shem::pdf::{Document, Page};
use shem::render::{check_page_with, render_page_with};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Counting;

thread_local! {
    static COUNT: Cell<u64> = const { Cell::new(0) };
}

fn bump() {
    // After the thread's locals are gone, nothing is counted.
    let _ = COUNT.try_with(|c| c.set(c.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn count() -> u64 {
    COUNT.with(Cell::get)
}

/// The first page of a fixture with other contents.
fn page_of(doc: &Document, contents: String) -> Page {
    Page {
        contents: contents.into_bytes(),
        ..doc.pages().unwrap()[0].clone()
    }
}

/// The allocations of the check and of the rendering of a page, after the
/// fonts and the outlines are in the cache.
fn allocations(doc: &Document, page: &Page, cache: &FontCache) -> u64 {
    check_page_with(doc, page, 72, cache).unwrap();
    render_page_with(doc, page, 72, cache).unwrap();
    let before = count();
    check_page_with(doc, page, 72, cache).unwrap();
    let canvas = render_page_with(doc, page, 72, cache).unwrap();
    let after = count();
    drop(canvas);
    after - before
}

/// The allocations of a page of `n` copies of `item`, made by `wrap`.
fn of_copies(fixture: &str, n: usize, item: &str, wrap: impl Fn(String) -> String) -> u64 {
    let doc = Document::parse(&std::fs::read(common::golem_fixture(fixture)).unwrap()).unwrap();
    let cache = FontCache::new();
    let page = page_of(&doc, wrap(item.repeat(n)));
    allocations(&doc, &page, &cache)
}

/// Golem shows each glyph in a text object of its own, with decimal numbers.
fn glyph(font: &str, cid: &str) -> String {
    format!("BT /{font} 9.2126 Tf\n1 0 0 1 223.5827 300.8819 Tm <{cid}> Tj\nET\n")
}

fn assert_per_glyph(fixture: &str, item: &str, wrap: impl Fn(String) -> String + Copy) {
    let one = of_copies(fixture, 1, item, wrap);
    let many = of_copies(fixture, 101, item, wrap);
    assert!(
        many <= one + 100 * 4,
        "{fixture}: 1 glyph: {one} allocations, 101 glyphs: {many}"
    );
}

#[test]
fn a_glyph_of_cff_allocates_at_most_four_times() {
    assert_per_glyph("sample", &glyph("F1", "73ED"), |s| s);
}

#[test]
fn a_glyph_of_truetype_allocates_at_most_four_times() {
    assert_per_glyph("truetype", &glyph("F1", "0014"), |s| s);
}

#[test]
fn a_glyph_stroked_in_mode_2_allocates_at_most_four_times() {
    assert_per_glyph("sample", &glyph("F1", "73ED"), |s| {
        format!("0.4 w 2 Tr\n{s}0 Tr\n")
    });
}

/// The allocations grow less than 1 for 10 items, as the capacities of
/// buffers that double.
fn assert_not_per_item(item: &str, wrap: impl Fn(String) -> String + Copy) {
    let few = of_copies("sample", 100, item, wrap);
    let many = of_copies("sample", 1000, item, wrap);
    assert!(
        many < few + 90,
        "100 items: {few} allocations, 1000 items: {many}"
    );
}

#[test]
fn the_allocations_do_not_grow_with_the_rectangles() {
    assert_not_per_item("10 10 50 50 re f\n", |s| s);
}

#[test]
fn the_allocations_do_not_grow_with_the_segments_of_a_path() {
    assert_not_per_item("10 10 50 50 re\n", |s| format!("{s}f\n"));
}

#[test]
fn the_allocations_do_not_grow_with_the_curves_of_a_path() {
    assert_not_per_item("20 20 300 30 40 10 c\n", |s| format!("40 10 m\n{s}h f\n"));
}

#[test]
fn the_allocations_do_not_grow_with_the_strokes() {
    // docs/adr/0025: the polygons of the strokes of a page go into one
    // path, with round caps and dashes.
    assert_not_per_item("10 10 m 50 50 l 60 20 l S\n", |s| s);
    assert_not_per_item("10 10 m 50 50 l 60 20 l S\n", |s| {
        format!("1 J [3 2] 0 d\n{s}")
    });
}

#[test]
fn the_allocations_do_not_grow_with_the_dashes() {
    // A zigzag of two segments of 1.4 points per item, dashed every half
    // point: about 6 dashes and 12 discs per item.
    assert_not_per_item("1 0 l 0 1 l\n", |s| {
        format!("1 J [0.5 0.5] 0 d 1 0 0 1 10 10 cm 0 0 m\n{s}S\n")
    });
}

/// The distinct CIDs of the `Tj` of a page, as two-byte hexadecimal strings.
fn distinct_cids(page: &Page) -> Vec<String> {
    let ops = shem::content::parse(&page.contents).unwrap();
    let mut cids: Vec<u16> = ops
        .iter()
        .filter_map(|op| match &op.operator {
            shem::content::Operator::ShowText(t) => Some(t.bytes().collect::<Vec<u8>>()),
            _ => None,
        })
        .flat_map(|b| {
            b.chunks(2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect::<Vec<_>>()
        })
        .collect();
    cids.sort_unstable();
    cids.dedup();
    cids.iter().map(|c| format!("{c:04X}")).collect()
}

#[test]
fn a_glyph_interpreted_for_the_first_time_allocates_a_few_times() {
    // docs/adr/0014: the charstring of each glyph is interpreted once per
    // document, by the thread that meets it first. Its stack and its
    // segments are reused, and the outline is copied once into the cache.
    let doc = Document::parse(&std::fs::read(common::golem_fixture("kanikosen")).unwrap()).unwrap();
    let text = &doc.pages().unwrap()[2];
    let cids = distinct_cids(text);
    assert!(cids.len() > 100, "{}", cids.len());
    let content: String = cids.iter().map(|c| glyph("F1", c)).collect();
    let page = page_of(&doc, content);
    let cache = FontCache::new();
    // The font is read before counting.
    check_page_with(
        &doc,
        &page_of(&doc, "BT /F1 9.2126 Tf ET\n".into()),
        72,
        &cache,
    )
    .unwrap();
    let before = count();
    check_page_with(&doc, &page, 72, &cache).unwrap();
    let cold = count() - before;
    let before = count();
    check_page_with(&doc, &page, 72, &cache).unwrap();
    let warm = count() - before;
    let n = cids.len() as u64;
    assert!(
        cold <= warm + n * 4,
        "{n} glyphs: {cold} allocations at the first check, {warm} at the second"
    );
}
