//! The images of Golem's PDFs are the same bytes on every machine
//! (docs/adr/0001). CI runs this test on Linux x86-64, Linux aarch64 and
//! macOS aarch64. To make the goldens again after an intended change of
//! the pixels or of the PNG, run it with SHEM_UPDATE_GOLDEN=1.

use shem::pdf::Document;
use shem::png::encode_png;
use shem::render::render_page;

const DPI: u32 = 100;
const FIXTURES: [&str; 4] = ["sample", "sample-marks", "figures", "truetype"];

#[test]
fn goldens() {
    let root = env!("CARGO_MANIFEST_DIR");
    let update = std::env::var_os("SHEM_UPDATE_GOLDEN").is_some();
    let mismatches: Vec<String> = FIXTURES
        .iter()
        .flat_map(|name| {
            let data = std::fs::read(format!("{root}/tests/fixtures/golem/{name}.pdf")).unwrap();
            let doc = Document::parse(&data).unwrap();
            doc.pages()
                .unwrap()
                .iter()
                .filter_map(|page| {
                    let png = encode_png(&render_page(&doc, page, DPI).unwrap());
                    let path = format!("{root}/tests/golden/{name}-{}.png", page.index + 1);
                    if update {
                        std::fs::write(&path, &png).unwrap();
                        return None;
                    }
                    let golden = std::fs::read(&path).unwrap_or_default();
                    (golden != png).then_some(path)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "differ from the goldens: {mismatches:?}"
    );
}
