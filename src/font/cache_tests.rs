//! The reuse of fonts and glyph outlines (docs/adr/0011).

use std::sync::Arc;

use crate::font::truetype::tests::{font, simple};
use crate::font::{FontCache, cff::Cff};
use crate::pdf::tests::build_pdf_bytes;
use crate::pdf::{Document, Object, Page};
use crate::render::{check_page_with, render_page, render_page_with};

fn square_font() -> Vec<u8> {
    font(
        &[
            vec![],
            simple(&[&[
                (0, 0, true),
                (1000, 0, true),
                (1000, 1000, true),
                (0, 1000, true),
            ]]),
        ],
        false,
        1000,
    )
}

fn stream(data: &[u8], dict: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {dict} >>\nstream\n", data.len()).into_bytes();
    out.extend(data);
    out.extend(b"\nendstream");
    out
}

/// A PDF whose pages share /F1 (a TrueType square, object 3) and /F2 (a
/// FontFile3 without Subtype, outside the subset, object 7).
fn pdf(contents: &[&str], font_dict: &str) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 11 + 2 * i)).collect();
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes(),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [4 0 R] >>".to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X /FontDescriptor 5 0 R /W [1 [1000]] >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /X /Flags 4 /FontFile2 6 0 R >>".to_vec(),
        stream(&square_font(), ""),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Y /Encoding /Identity-H /DescendantFonts [8 0 R] >>".to_vec(),
        b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /Y /FontDescriptor 9 0 R >>".to_vec(),
        b"<< /Type /FontDescriptor /FontName /Y /Flags 4 /FontFile3 10 0 R >>".to_vec(),
        stream(&square_font(), ""),
    ];
    contents.iter().enumerate().for_each(|(i, c)| {
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40] /Resources << /Font {font_dict} >> /Contents {} 0 R >>",
                12 + 2 * i
            )
            .into_bytes(),
        );
        objects.push(stream(c.as_bytes(), ""));
    });
    build_pdf_bytes(&objects, "")
}

const SHARED: &str = "<< /F1 3 0 R /F2 7 0 R >>";
const GOOD: &str = "BT /F1 10 Tf 1 0 0 1 5 5 Tm <0001> Tj ET";
const BAD: &str = "BT /F2 10 Tf 1 0 0 1 5 5 Tm <0001> Tj ET";

fn doc_and_pages(data: &[u8]) -> (Document, Vec<Page>) {
    let doc = Document::parse(data).unwrap();
    let pages = doc.pages().unwrap();
    (doc, pages)
}

#[test]
fn a_font_is_loaded_once_for_every_page_and_for_check_and_render() {
    let (doc, pages) = doc_and_pages(&pdf(&[GOOD, GOOD, GOOD], SHARED));
    let cache = FontCache::new();
    pages
        .iter()
        .for_each(|p| check_page_with(&doc, p, 72, &cache).unwrap());
    assert_eq!(cache.loads(), 1);
    pages.iter().for_each(|p| {
        let with = render_page_with(&doc, p, 72, &cache).unwrap();
        assert_eq!(with, render_page(&doc, p, 72).unwrap());
    });
    assert_eq!(cache.loads(), 1);
}

#[test]
fn a_font_outside_the_subset_is_the_same_error_on_every_page() {
    let (doc, pages) = doc_and_pages(&pdf(&[GOOD, BAD, BAD], SHARED));
    let cache = FontCache::new();
    let errors: Vec<String> = pages
        .iter()
        .filter_map(|p| check_page_with(&doc, p, 72, &cache).err())
        .map(|e| e.to_string())
        .collect();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].starts_with("page 2: "), "{}", errors[0]);
    assert_eq!(errors[0].replacen("page 2", "page 3", 1), errors[1]);
    assert!(errors[0].contains("FontFile3"));
    assert_eq!(cache.loads(), 2);
}

#[test]
fn an_unused_font_outside_the_subset_is_not_read() {
    let (doc, pages) = doc_and_pages(&pdf(&[GOOD], SHARED));
    let cache = FontCache::new();
    assert!(check_page_with(&doc, &pages[0], 72, &cache).is_ok());
    assert_eq!(cache.loads(), 1);
}

#[test]
fn a_direct_font_dictionary_is_read_each_time() {
    let direct = "<< /F1 << /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [4 0 R] >> >>";
    let (doc, pages) = doc_and_pages(&pdf(&[GOOD, GOOD], direct));
    let cache = FontCache::new();
    pages.iter().for_each(|p| {
        let canvas = render_page_with(&doc, p, 72, &cache).unwrap();
        assert_eq!(canvas.pixel(10, 29), [0, 0, 0]);
    });
    assert_eq!(cache.loads(), 0);
}

#[test]
fn threads_share_one_load() {
    let (doc, _) = doc_and_pages(&pdf(&[GOOD], SHARED));
    let cache = FontCache::new();
    let obj = Object::Ref(crate::pdf::Ref {
        num: 3,
        generation: 0,
    });
    let fonts: Vec<Arc<crate::font::Font>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| s.spawn(|| cache.get(&doc, &obj).unwrap()))
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(cache.loads(), 1);
    assert!(fonts.windows(2).all(|w| Arc::ptr_eq(&w[0], &w[1])));
}

#[test]
fn a_glyph_is_interpreted_once() {
    let many = format!(
        "BT /F1 1 Tf {} ET",
        (0..100)
            .map(|i| format!("1 0 0 1 {} 5 Tm <0001> Tj ", i % 30))
            .collect::<String>()
    );
    let (doc, pages) = doc_and_pages(&pdf(&[&many], SHARED));
    let cache = FontCache::new();
    render_page_with(&doc, &pages[0], 72, &cache).unwrap();
    let font = cache
        .get(
            &doc,
            &Object::Ref(crate::pdf::Ref {
                num: 3,
                generation: 0,
            }),
        )
        .unwrap();
    assert_eq!(font.interpretations(), 1);
}

#[test]
fn cached_outlines_equal_the_interpreted_ones() {
    // TrueType
    let (doc, _) = doc_and_pages(&pdf(&[GOOD], SHARED));
    let cache = FontCache::new();
    let font = cache
        .get(
            &doc,
            &Object::Ref(crate::pdf::Ref {
                num: 3,
                generation: 0,
            }),
        )
        .unwrap();
    [0u16, 1].iter().for_each(|&cid| {
        assert_eq!(
            &*font.outline(cid).unwrap(),
            font.interpret_outline(cid).unwrap().as_slice()
        );
        assert_eq!(
            &*font.outline(cid).unwrap(),
            font.interpret_outline(cid).unwrap().as_slice()
        );
    });
    assert!(font.outline(7).is_err());
    // CFF: the Noto Serif JP of the sample fixture, through the font of its first page
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/golem/sample.pdf"
    ))
    .unwrap();
    let (doc, pages) = doc_and_pages(&data);
    let fonts = doc.get_dict(&pages[0].resources, "Font").unwrap().unwrap();
    let noto = cache.get(&doc, &fonts[b"F1".as_slice()]).unwrap();
    let file = {
        let type0 = doc.resolve(&fonts[b"F1".as_slice()]).unwrap();
        let Some(Object::Array(d)) = doc
            .get(type0.as_dict().unwrap(), "DescendantFonts")
            .unwrap()
        else {
            panic!()
        };
        let cid = doc.resolve(&d[0]).unwrap();
        let desc = doc
            .get_dict(cid.as_dict().unwrap(), "FontDescriptor")
            .unwrap()
            .unwrap();
        let Some(Object::Stream(s)) = doc.get(&desc, "FontFile3").unwrap() else {
            panic!()
        };
        doc.stream_data(&s).unwrap()
    };
    let cff = Cff::parse(&file).unwrap();
    [722u16, 1403, 9511].iter().for_each(|&cid| {
        let direct = cff.outline(cff.gid(cid).unwrap()).unwrap();
        assert_eq!(&*noto.outline(cid).unwrap(), direct.as_slice());
    });
}
