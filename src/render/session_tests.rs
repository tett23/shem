//! The parallel API for library users (docs/adr/0012).

use std::num::NonZeroUsize;
use std::sync::Mutex;

use super::{PageError, Session, render_page};
use crate::pdf::Document;
use crate::raster::Canvas;

fn jobs(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/golem/{name}.pdf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn render_pages_gives_the_images_of_render() {
    ["sample", "sample-marks", "figures", "truetype"]
        .iter()
        .for_each(|name| {
            let data = fixture(name);
            let doc = Document::parse(&data).unwrap();
            let session = Session::new(&doc, 36).unwrap();
            let all: Vec<usize> = (0..session.page_count()).collect();
            for j in [1, 4] {
                let got: Mutex<Vec<(usize, Canvas)>> = Mutex::new(Vec::new());
                session
                    .render_pages(&all, jobs(j), |page, canvas| {
                        got.lock().unwrap().push((page, canvas));
                        Ok::<(), ()>(())
                    })
                    .unwrap_or_else(|_| panic!("{name}"));
                let mut got = got.into_inner().unwrap();
                got.sort_by_key(|(p, _)| *p);
                assert_eq!(
                    got.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
                    all,
                    "{name}: each page once"
                );
                got.iter().for_each(|(p, canvas)| {
                    assert_eq!(*canvas, session.render(*p).unwrap());
                    assert_eq!(
                        *canvas,
                        render_page(&doc, &doc.pages().unwrap()[*p], 36).unwrap()
                    );
                });
            }
        });
}

#[test]
fn the_first_consume_error_in_the_order_of_pages() {
    let data = fixture("sample");
    let doc = Document::parse(&data).unwrap();
    let session = Session::new(&doc, 36).unwrap();
    for j in [1, 4] {
        let result = session.render_pages(&[0, 1, 2], jobs(j), |page, _| {
            if page >= 1 { Err(page * 10) } else { Ok(()) }
        });
        match result {
            Err(PageError::Consume(1, 10)) => {}
            Err(PageError::Consume(p, e)) => panic!("jobs {j}: consume error {p} {e}"),
            Err(PageError::Render(e)) => panic!("jobs {j}: {e}"),
            Ok(()) => panic!("jobs {j}: no error"),
        }
    }
}

/// A PDF of pages with the given contents.
fn pdf(contents: &[&str]) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
    ];
    contents.iter().enumerate().for_each(|(i, c)| {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10] /Resources << >> /Contents {} 0 R >>", 4 + 2 * i));
        objects.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
    });
    let refs: Vec<&str> = objects.iter().map(String::as_str).collect();
    crate::pdf::tests::build_pdf(&refs, "")
}

#[test]
fn check_pages_reports_the_first_bad_page() {
    let data = pdf(&["", "q (x) TJ Q", "", "q (y) Td Q", ""]);
    let doc = Document::parse(&data).unwrap();
    let session = Session::new(&doc, 72).unwrap();
    for j in [1, 8] {
        for _ in 0..20 {
            let e = session.check_pages(&[0, 1, 2, 3, 4], jobs(j)).unwrap_err();
            assert_eq!(e.to_string(), "page 2: offset 6: unsupported operator TJ");
        }
    }
    assert!(session.check_pages(&[0, 2, 4], jobs(4)).is_ok());
}

#[test]
fn pages_out_of_range_are_errors() {
    let data = pdf(&["", ""]);
    let doc = Document::parse(&data).unwrap();
    let session = Session::new(&doc, 72).unwrap();
    assert_eq!(session.page_count(), 2);
    assert!(session.check(2).unwrap_err().to_string().contains("page 3"));
    assert!(session.render(5).is_err());
    assert!(session.check_pages(&[0, 7], jobs(2)).is_err());
    assert!(matches!(
        session.render_pages(&[9], jobs(2), |_, _| Ok::<(), ()>(())),
        Err(PageError::Render(_))
    ));
}

#[test]
fn a_session_is_shared_by_threads() {
    fn assert_sync<T: Sync>() {}
    fn assert_send<T: Send>() {}
    assert_sync::<Session<'static>>();
    assert_send::<Canvas>();
}
