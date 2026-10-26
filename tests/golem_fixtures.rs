//! The PDFs that Golem writes are in the subset of v0.1 (docs/adr/0002).

use shem::content;
use shem::pdf::{Document, Object};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/golem/{name}.pdf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

const FIXTURES: [(&str, usize); 4] = [
    ("sample", 3),
    ("sample-marks", 3),
    ("figures", 2),
    ("truetype", 2),
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
