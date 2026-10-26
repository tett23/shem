//! The images of Golem's PDFs are the same bytes on every machine
//! (docs/adr/0001). CI runs this test on Linux x86-64, Linux aarch64 and
//! macOS aarch64. To make the goldens again after an intended change of
//! the pixels or of the PNG, run it with SHEM_UPDATE_GOLDEN=1.

use shem::image::decode;
use shem::pdf::Document;
use shem::png::encode_png;
use shem::render::render_page;

const DPI: u32 = 100;
const FIXTURES: [&str; 4] = ["sample", "sample-marks", "figures", "truetype"];

fn root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn update() -> bool {
    std::env::var_os("SHEM_UPDATE_GOLDEN").is_some()
}

/// Compares a PNG with its golden, or writes it with SHEM_UPDATE_GOLDEN.
fn check(name: &str, png: &[u8]) -> Option<String> {
    let path = format!("{}/tests/golden/{name}.png", root());
    if update() {
        std::fs::write(&path, png).unwrap();
        return None;
    }
    let golden = std::fs::read(&path).unwrap_or_default();
    (golden != png).then_some(path)
}

#[test]
fn goldens() {
    let mismatches: Vec<String> = FIXTURES
        .iter()
        .flat_map(|name| {
            let data =
                std::fs::read(format!("{}/tests/fixtures/golem/{name}.pdf", root())).unwrap();
            let doc = Document::parse(&data).unwrap();
            doc.pages()
                .unwrap()
                .iter()
                .filter_map(|page| {
                    let canvas = render_page(&doc, page, DPI).unwrap();
                    let png = encode_png(&canvas);
                    // shem reads back the PNG that it wrote (docs/adr/0010)
                    assert_eq!(decode(&png).unwrap(), canvas);
                    check(&format!("{name}-{}", page.index + 1), &png)
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "differ from the goldens: {mismatches:?}"
    );
}

/// The diff image of the first pages of sample and sample-marks, which also
/// differ in size (docs/adr/0010).
#[test]
fn diff_golden() {
    let read = |name: &str| {
        decode(&std::fs::read(format!("{}/tests/golden/{name}.png", root())).unwrap()).unwrap()
    };
    let d = shem::diff::diff(&read("sample-1"), &read("sample-marks-1")).unwrap();
    assert_eq!(check("diff-sample-1", &encode_png(&d.image)), None);
}
