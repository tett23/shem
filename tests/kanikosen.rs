//! The images of the kanikosen fixture are the bytes that shem made before
//! the changes of docs/adr/0013, which must not change them. The SHA-256 of
//! every file is in tests/fixtures/golem/kanikosen.sha256, made with
//! `shem render --dpi <72|150> --format <png|ppm>` of v0.1.0-rc2. To write
//! the list again after an intended change of the pixels, run this test
//! with SHEM_UPDATE_GOLDEN=1.

mod common;

use shem::pdf::Document;
use shem::png::{encode_png, encode_ppm};
use shem::render::Session;
use std::num::NonZeroUsize;
use std::sync::Mutex;

fn list_path() -> String {
    format!(
        "{}/tests/fixtures/golem/kanikosen.sha256",
        env!("CARGO_MANIFEST_DIR")
    )
}

#[test]
fn every_image_is_the_same_bytes_as_before() {
    let data = std::fs::read(common::golem_fixture("kanikosen")).unwrap();
    let doc = Document::parse(&data).unwrap();
    let jobs = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let lines = Mutex::new(Vec::new());
    [72u32, 150].iter().for_each(|&dpi| {
        let session = Session::new(&doc, dpi).unwrap();
        let pages: Vec<usize> = (0..session.page_count()).collect();
        assert_eq!(pages.len(), 129);
        session
            .render_pages(&pages, jobs, |page, canvas| {
                let n = page + 1;
                let png = common::sha256_hex(&encode_png(&canvas));
                let ppm = common::sha256_hex(&encode_ppm(&canvas));
                let mut lines = lines.lock().unwrap();
                lines.push(format!("{png}  {dpi}-png/page-{n:04}.png"));
                lines.push(format!("{ppm}  {dpi}-ppm/page-{n:04}.ppm"));
                Ok::<(), ()>(())
            })
            .unwrap();
    });
    let mut lines = lines.into_inner().unwrap();
    // The order of `shasum */*`: by the name of the file.
    lines.sort_by(|a, b| a[66..].cmp(&b[66..]));
    let actual = lines.join("\n") + "\n";
    if common::update() {
        std::fs::write(list_path(), &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(list_path()).unwrap();
    let differ: Vec<&str> = actual
        .lines()
        .zip(expected.lines())
        .filter(|(a, e)| a != e)
        .map(|(a, _)| &a[66..])
        .collect();
    assert!(differ.is_empty(), "the images differ: {differ:?}");
    assert_eq!(actual.lines().count(), expected.lines().count());
}
