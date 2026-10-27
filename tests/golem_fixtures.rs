//! The PDFs that Golem writes are in the subset of v0.1 (docs/adr/0002).

mod common;

use shem::content;
use shem::pdf::{Document, Object};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/golem/{name}.pdf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

const FIXTURES: [(&str, usize); 5] = [
    ("sample", 3),
    ("sample-marks", 3),
    ("figures", 2),
    ("truetype", 2),
    ("kanikosen", 129),
];

#[test]
fn every_page_parses_and_its_resources_exist() {
    FIXTURES.iter().for_each(|&(name, count)| {
        let doc = Document::parse(&fixture(name)).unwrap();
        let pages = doc.pages().unwrap();
        assert_eq!(pages.len(), count, "{name}");
        pages.iter().for_each(|page| {
            let ops = content::parse(&page.contents)
                .unwrap_or_else(|e| panic!("{name}: page {}: {e}", page.index + 1));
            assert!(!ops.is_empty());
            content::check_resources(&doc, &page.resources, &ops).unwrap();
        });
    });
}

/// The operators of every page are those that shem read before the changes
/// of docs/adr/0013: the SHA-256 of their `Debug` form is in
/// tests/fixtures/golem/operators.sha256. To write the list again after an
/// intended change, run this test with SHEM_UPDATE_GOLDEN=1.
#[test]
fn every_page_has_the_same_operators_as_before() {
    let path = format!(
        "{}/tests/fixtures/golem/operators.sha256",
        env!("CARGO_MANIFEST_DIR")
    );
    let actual: String = FIXTURES
        .iter()
        .flat_map(|&(name, _)| {
            let doc = Document::parse(&fixture(name)).unwrap();
            doc.pages()
                .unwrap()
                .iter()
                .map(|page| {
                    let ops = content::parse(&page.contents).unwrap();
                    let digest = common::sha256_hex(format!("{ops:?}").as_bytes());
                    format!("{digest}  {name} page {}\n", page.index + 1)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    if common::update() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    assert_eq!(actual, std::fs::read_to_string(&path).unwrap());
}

#[test]
fn every_flate_stream_inflates() {
    FIXTURES.iter().for_each(|&(name, _)| {
        let doc = Document::parse(&fixture(name)).unwrap();
        let streams: Vec<_> = doc
            .object_numbers()
            .filter_map(|num| match doc.load(num).unwrap() {
                Object::Stream(s) => Some(s),
                _ => None,
            })
            .collect();
        assert!(!streams.is_empty());
        streams
            .iter()
            .filter(|s| {
                s.dict.get(b"Filter".as_slice()).and_then(Object::as_name) == Some(b"FlateDecode")
            })
            .for_each(|s| {
                doc.stream_data(s).unwrap_or_else(|e| panic!("{name}: {e}"));
            });
    });
}

#[test]
fn every_page_renders() {
    FIXTURES.iter().for_each(|&(name, _)| {
        let doc = Document::parse(&fixture(name)).unwrap();
        doc.pages().unwrap().iter().for_each(|page| {
            let canvas =
                shem::render::render_page(&doc, page, 72).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                canvas.pixels.iter().any(|&v| v != 255),
                "{name}: page {} is blank",
                page.index + 1
            );
        });
    });
}

#[test]
fn a_cid_of_noto_serif_jp_is_mapped_by_the_charset() {
    // golem-pdf ADR 0011: in Noto Serif JP, the glyph 466 has the CID 722
    let doc = Document::parse(&fixture("sample")).unwrap();
    let page = &doc.pages().unwrap()[0];
    let font = doc.get_dict(&page.resources, "Font").unwrap().unwrap();
    let type0 = doc.resolve(&font[b"F1".as_slice()]).unwrap();
    let cidfont = match doc
        .get(type0.as_dict().unwrap(), "DescendantFonts")
        .unwrap()
        .unwrap()
    {
        Object::Array(a) => doc.resolve(&a[0]).unwrap(),
        other => panic!("{other:?}"),
    };
    let descriptor = doc
        .get_dict(cidfont.as_dict().unwrap(), "FontDescriptor")
        .unwrap()
        .unwrap();
    let Some(Object::Stream(file)) = doc.get(&descriptor, "FontFile3").unwrap() else {
        panic!()
    };
    let cff = shem::font::cff::Cff::parse(&doc.stream_data(&file).unwrap()).unwrap();
    assert!(cff.is_cid_keyed());
    assert_eq!(cff.gid(722).unwrap(), 466);
}
