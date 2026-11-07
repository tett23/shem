//! The functions of the wasm module, called natively (docs/adr/0018).

use super::*;
use crate::pdf::Document;
use crate::render::Session;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/golem/{name}.pdf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Copies `data` into a new input of the module, and returns its pointer.
fn put(m: &mut Module, data: &[u8]) -> usize {
    let ptr = m.alloc(data.len());
    m.input_mut(ptr).unwrap().copy_from_slice(data);
    ptr
}

#[test]
fn documents_render_as_a_session_does() {
    ["sample", "kanikosen"].iter().for_each(|name| {
        let data = fixture(name);
        let mut m = Module::new();
        let ptr = put(&mut m, &data);
        let doc = m.open(ptr, data.len(), 36);
        assert!(doc >= 1, "{}", m.error());
        // The input may be freed once the document is open.
        m.free(ptr);
        let reference = Document::parse(&data).unwrap();
        let session = Session::new(&reference, 36).unwrap();
        assert_eq!(m.page_count(doc) as usize, session.page_count());
        let pages: Vec<u32> = (0..m.page_count(doc)).take(12).collect();
        pages.iter().chain(pages.iter().rev()).for_each(|&p| {
            assert_eq!(m.render(doc, p), 1, "{name} page {p}: {}", m.error());
            let canvas = session.render(p as usize).unwrap();
            let (rgba, width, height) = m.image(doc).unwrap();
            assert_eq!(
                (width as usize, height as usize),
                (canvas.width, canvas.height)
            );
            assert_eq!(rgba.len(), canvas.pixels.len() / 3 * 4);
            assert!(
                rgba.chunks(4)
                    .zip(canvas.pixels.chunks(3))
                    .all(|(a, b)| a[..3] == *b && a[3] == 255),
                "{name} page {p}"
            );
        });
        m.close(doc);
        assert_eq!(m.page_count(doc), 0);
    });
}

#[test]
fn wrong_arguments_fail_with_a_message() {
    let data = fixture("sample");
    let mut m = Module::new();
    let ptr = put(&mut m, &data);
    let doc = m.open(ptr, data.len(), 36);
    assert!(doc >= 1);
    let fails = |m: &mut Module, result: u32, message: &str| {
        assert_eq!(result, 0, "{message}");
        assert_eq!(m.error(), message);
    };
    let r = m.render(doc + 1, 0);
    fails(&mut m, r, &format!("no such document: {}", doc + 1));
    let r = m.render(doc, 3);
    fails(&mut m, r, "page 4 does not exist: the document has 3 pages");
    let r = m.open(ptr, data.len(), 0);
    fails(&mut m, r, "dpi must be an integer from 1 to 1200: 0");
    let r = m.open(ptr, data.len(), 1201);
    fails(&mut m, r, "dpi must be an integer from 1 to 1200: 1201");
    let r = m.open(ptr + 1, 10, 36);
    fails(&mut m, r, &format!("no such input: {}", ptr + 1));
    let r = m.open(ptr, data.len() + 1, 36);
    fails(
        &mut m,
        r,
        &format!(
            "length {} is more than the input of {} bytes",
            data.len() + 1,
            data.len()
        ),
    );
    let r = m.open(ptr, 5, 36);
    assert_eq!(r, 0);
    assert!(!m.error().is_empty());
    // The document opened before still renders.
    assert_eq!(m.render(doc, 0), 1, "{}", m.error());
    assert!(m.image(doc).is_some());
    m.close(doc);
    let r = m.render(doc, 0);
    fails(&mut m, r, &format!("no such document: {doc}"));
    assert_eq!(m.page_count(doc), 0);
    assert!(m.image(doc).is_none());
    m.free(ptr);
    let r = m.open(ptr, 1, 36);
    fails(&mut m, r, &format!("no such input: {ptr}"));
}

#[test]
fn the_errors_are_those_of_the_command() {
    // A content with an operator out of the subset: the message is the one
    // that `shem render` writes after the path.
    let data = fixture("sample");
    let doc = Document::parse(&data).unwrap();
    let expected = crate::render::render_page(&doc, &doc.pages().unwrap()[0], 36)
        .err()
        .map(|e| e.to_string());
    assert!(expected.is_none());
    let mut m = Module::new();
    let ptr = put(&mut m, b"%PDF-1.6\nnot a PDF");
    assert_eq!(m.open(ptr, 18, 36), 0);
    assert_eq!(
        m.error(),
        Document::parse(b"%PDF-1.6\nnot a PDF")
            .err()
            .unwrap()
            .to_string()
    );
}

/// A PDF of one page with the given content and no resources.
fn one_page(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents 4 0 R >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut out = b"%PDF-1.6\n".to_vec();
    let offsets: Vec<usize> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let at = out.len();
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).into_bytes());
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

#[test]
fn the_errors_of_a_content_are_those_of_the_command() {
    // docs/adr/0019: the message of a broken content is the one that
    // `shem render` writes after the path, with the page and the offset.
    [
        ("q /A w Q", "page 1: offset 5: wrong operands of w"),
        (
            "q 2147483648 0 m f Q",
            "page 1: offset 15: wrong operands of m",
        ),
        (
            "q 9223372036854775808 w Q",
            "page 1: offset 2: integer out of range: 9223372036854775808",
        ),
        ("BT 2.0 Tr ET", "page 1: offset 7: wrong operands of Tr"),
        ("BT 1 Tr ET", "page 1: offset 5: unsupported Tr 1"),
        ("q (x) TJ Q", "page 1: offset 6: unsupported operator TJ"),
        (
            "q /F1 1 Tf Q",
            "page 1: offset 8: no font F1 in the resources",
        ),
    ]
    .iter()
    .for_each(|&(content, message)| {
        let data = one_page(content);
        let mut m = Module::new();
        let ptr = put(&mut m, &data);
        let doc = m.open(ptr, data.len(), 36);
        assert!(doc >= 1, "{content}: {}", m.error());
        assert_eq!(m.render(doc, 0), 0, "{content}");
        assert_eq!(m.error(), message, "{content}");
        assert!(m.image(doc).is_some_and(|(rgba, _, _)| rgba.is_empty()));
    });
    // A content that is well formed renders.
    let data = one_page("q 0 0 5 5 re f Q");
    let mut m = Module::new();
    let ptr = put(&mut m, &data);
    let doc = m.open(ptr, data.len(), 36);
    assert_eq!(m.render(doc, 0), 1, "{}", m.error());
}

#[test]
fn broken_pdfs_fail_without_a_panic() {
    // docs/adr/0018: in wasm a panic is a trap, so every input must give a
    // result or an error.
    let mut state = 0x0018_0001u64;
    let mut next = |n: u64| {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d) % n.max(1)
    };
    let mut panics = Vec::new();
    [
        ("sample", 1000),
        ("figures", 1000),
        ("truetype", 1000),
        ("sample-marks", 1000),
        ("kanikosen", 1000),
    ]
    .iter()
    .for_each(|&(name, count)| {
        let data = fixture(name);
        (0..count).for_each(|k| {
            let mut d = data.clone();
            match k % 3 {
                0 => d.truncate(next(d.len() as u64) as usize),
                1 => {
                    let i = next(d.len() as u64) as usize;
                    d[i] ^= 1 << next(8);
                }
                _ => (0..1 + next(8)).for_each(|_| {
                    let i = next(d.len() as u64) as usize;
                    d[i] = next(256) as u8;
                }),
            }
            let result = std::panic::catch_unwind(|| {
                let mut m = Module::new();
                let ptr = put(&mut m, &d);
                let doc = m.open(ptr, d.len(), 18);
                if doc == 0 {
                    assert!(!m.error().is_empty());
                    return;
                }
                let n = m.page_count(doc).min(5);
                (0..n).for_each(|p| {
                    if m.render(doc, p) == 0 {
                        assert!(!m.error().is_empty());
                    }
                });
            });
            if result.is_err() {
                panics.push(format!("{name} {k}"));
            }
        });
    });
    assert!(panics.is_empty(), "{panics:?}");
}
