use super::*;
use crate::fixed::Fixed;

/// A PDF from object bodies (numbered from 1) and extra trailer entries,
/// with a correct classic xref table.
pub(crate) fn build_pdf(objects: &[&str], trailer_extra: &str) -> Vec<u8> {
    let bytes: Vec<Vec<u8>> = objects.iter().map(|s| s.as_bytes().to_vec()).collect();
    build_pdf_bytes(&bytes, trailer_extra)
}

/// A PDF from object bodies given as bytes.
pub(crate) fn build_pdf_bytes(objects: &[Vec<u8>], trailer_extra: &str) -> Vec<u8> {
    let mut out = b"%PDF-1.6\n%\xE2\xE3\xCF\xD3\n".to_vec();
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
            "trailer\n<< /Size {} /Root 1 0 R {} >>\nstartxref\n{}\n%%EOF\n",
            objects.len() + 1,
            trailer_extra,
            xref
        )
        .into_bytes(),
    );
    out
}

fn stream(content: &str) -> String {
    format!(
        "<< /Length {} >>\nstream\n{}\nendstream",
        content.len(),
        content
    )
}

fn one_page(page_extra: &str, content: &str) -> Vec<u8> {
    build_pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> /Contents 4 0 R {page_extra} >>"
            ),
            &stream(content),
        ],
        "",
    )
}

fn n(s: &str) -> Fixed {
    Fixed::parse(s.as_bytes()).unwrap()
}

fn err_of(data: &[u8]) -> String {
    match Document::parse(data).and_then(|d| d.pages()) {
        Ok(_) => panic!("no error"),
        Err(e) => e.to_string(),
    }
}

// --- the lexer and the objects

#[test]
fn parses_the_kinds_of_objects() {
    let obj = parse_object_bytes(
        b"<< /A 1 /B -2.5 /C (str\\)ing) /D <48656C6C6F> /E [true false null] /F#20G 3 0 R >>",
    )
    .unwrap();
    let d = obj.as_dict().unwrap();
    assert_eq!(d.get(b"A".as_slice()), Some(&Object::Int(1)));
    assert_eq!(d.get(b"B".as_slice()), Some(&Object::Real(n("-2.5"))));
    assert_eq!(
        d.get(b"C".as_slice()),
        Some(&Object::String(b"str)ing".to_vec()))
    );
    assert_eq!(
        d.get(b"D".as_slice()),
        Some(&Object::String(b"Hello".to_vec()))
    );
    assert_eq!(
        d.get(b"E".as_slice()),
        Some(&Object::Array(vec![
            Object::Bool(true),
            Object::Bool(false),
            Object::Null
        ]))
    );
    assert_eq!(
        d.get(b"F G".as_slice()),
        Some(&Object::Ref(Ref {
            num: 3,
            generation: 0
        }))
    );
}

#[test]
fn literal_string_escapes() {
    let cases: [(&[u8], &[u8]); 7] = [
        (b"(a\\nb)", b"a\nb"),
        (b"(\\101\\60\\0)", b"A0\0"),
        (b"(a(b)c)", b"a(b)c"),
        (b"(a\\\nb)", b"ab"),
        (b"(a\r\nb)", b"a\nb"),
        (b"(a\rb)", b"a\nb"),
        (b"(\\q)", b"q"),
    ];
    cases.iter().for_each(|(src, want)| {
        assert_eq!(
            parse_object_bytes(src).unwrap(),
            Object::String(want.to_vec())
        )
    });
}

#[test]
fn hex_strings_ignore_whitespace_and_pad_odd_digits() {
    assert_eq!(
        parse_object_bytes(b"<48 65 6>").unwrap(),
        Object::String(vec![0x48, 0x65, 0x60])
    );
    assert_eq!(parse_object_bytes(b"<>").unwrap(), Object::String(vec![]));
    assert!(parse_object_bytes(b"<4G>").is_err());
}

#[test]
fn comments_are_whitespace() {
    assert_eq!(
        parse_object_bytes(b"% comment\n 42").unwrap(),
        Object::Int(42)
    );
}

#[test]
fn malformed_objects_are_errors() {
    for src in [&b"<< /A >>"[..], b"[1 2", b"(abc", b"<< 1 2 >>", b"]", b"{"] {
        assert!(
            parse_object_bytes(src).is_err(),
            "{}",
            String::from_utf8_lossy(src)
        );
    }
}

// --- the file structure

#[test]
fn reads_a_minimal_page() {
    let doc = Document::parse(&one_page("", "")).unwrap();
    let pages = doc.pages().unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].media_box, [n("0"), n("0"), n("200"), n("100")]);
    assert_eq!(pages[0].object, 3);
}

#[test]
fn rejects_what_v0_1_does_not_read() {
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            build_pdf(
                &[
                    "<< /Type /Catalog /Pages 2 0 R >>",
                    "<< /Type /Pages /Kids [] /Count 0 >>",
                ],
                "/Prev 100",
            ),
            "unsupported Prev",
        ),
        (
            build_pdf(
                &[
                    "<< /Type /Catalog /Pages 2 0 R >>",
                    "<< /Type /Pages /Kids [] /Count 0 >>",
                ],
                "/Encrypt 2 0 R",
            ),
            "unsupported Encrypt",
        ),
        (
            build_pdf(
                &[
                    "<< /Type /Catalog /Pages 2 0 R >>",
                    "<< /Type /Pages /Kids [] /Count 0 >>",
                ],
                "/XRefStm 100",
            ),
            "unsupported XRefStm",
        ),
        (
            build_pdf(
                &[
                    "<< /Type /Catalog /Pages 2 1 R >>",
                    "<< /Type /Pages /Kids [] /Count 0 >>",
                ],
                "",
            ),
            "generation",
        ),
        (
            build_pdf(
                &[
                    "<< /Type /Catalog /Pages 2 0 R >>",
                    "<< /Type /ObjStm /N 0 /First 0 /Length 0 >>\nstream\n\nendstream",
                ],
                "",
            ),
            "unsupported Type ObjStm",
        ),
    ];
    cases.iter().for_each(|(data, want)| {
        let e = err_of(data);
        assert!(e.contains(want), "{e} does not contain {want}");
    });
}

#[test]
fn rejects_an_xref_stream() {
    let mut data = b"%PDF-1.6\n".to_vec();
    let at = data.len();
    data.extend(
        b"1 0 obj\n<< /Type /XRef /Size 2 /W [1 1 1] /Length 0 >>\nstream\n\nendstream\nendobj\n",
    );
    data.extend(format!("startxref\n{at}\n%%EOF\n").into_bytes());
    assert!(err_of(&data).contains("unsupported xref stream"));
}

#[test]
fn rejects_a_broken_xref() {
    let mut data = one_page("", "");
    // shift every object by inserting a character after the header
    data.insert(15, b'x');
    assert!(err_of(&data).contains("object"));
    let data = b"%PDF-1.6\nno startxref here\n".to_vec();
    assert!(err_of(&data).contains("startxref"));
    let data = b"not a pdf".to_vec();
    assert!(err_of(&data).contains("%PDF-"));
}

#[test]
fn page_tree_nodes_are_inherited() {
    let data = build_pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 2 /MediaBox [0 0 50 60] /Resources << /Font << /F1 7 0 R >> >> >>",
            "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /Count 2 >>",
            "<< /Type /Page /Parent 3 0 R /Contents 6 0 R >>",
            "<< /Type /Page /Parent 3 0 R /Contents 6 0 R /MediaBox [0 0 10 20] /Resources << >> >>",
            &stream(""),
            "<< /Type /Font >>",
        ],
        "",
    );
    let pages = Document::parse(&data).unwrap().pages().unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].media_box[2], n("50"));
    assert!(pages[0].resources.contains_key(b"Font".as_slice()));
    assert_eq!(pages[1].media_box[2], n("10"));
    assert!(!pages[1].resources.contains_key(b"Font".as_slice()));
}

#[test]
fn a_cycle_in_the_page_tree_is_an_error() {
    let data = build_pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Pages /Kids [2 0 R] /Count 1 >>",
        ],
        "",
    );
    assert!(err_of(&data).contains("cycle"));
}

#[test]
fn rotate_and_user_unit() {
    assert!(err_of(&one_page("/Rotate 90", "")).contains("unsupported Rotate 90"));
    assert!(err_of(&one_page("/UserUnit 2", "")).contains("unsupported UserUnit"));
    assert!(
        Document::parse(&one_page("/Rotate 0", ""))
            .unwrap()
            .pages()
            .is_ok()
    );
}

#[test]
fn contents_arrays_are_joined_with_a_space() {
    let data = build_pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] /Resources << >> /Contents [4 0 R 5 0 R] >>",
            "<< /Length 1 >>\nstream\nq\nendstream",
            "<< /Length 1 >>\nstream\nQ\nendstream",
        ],
        "",
    );
    let pages = Document::parse(&data).unwrap().pages().unwrap();
    assert_eq!(pages[0].contents, b"q Q");
}

#[test]
fn flate_contents_and_unsupported_filters() {
    let z = include_bytes!("../../tests/fixtures/flate/fixed.zz");
    let mut data = build_pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] /Resources << >> /Contents 4 0 R >>",
            "PLACEHOLDER",
        ],
        "",
    );
    let at = data.windows(11).position(|w| w == b"PLACEHOLDER").unwrap();
    let mut stream_bytes =
        format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", z.len()).into_bytes();
    stream_bytes.extend_from_slice(z);
    stream_bytes.extend(b"\nendstream");
    data.splice(at..at + 11, stream_bytes);
    // the splice moves no object before it, but the xref table after it
    let fixed_xref = rebuild_xref(&data);
    let pages = Document::parse(&fixed_xref).unwrap().pages().unwrap();
    assert_eq!(
        pages[0].contents,
        include_bytes!("../../tests/fixtures/flate/fixed.raw")
    );

    let lzw = replace_bytes(
        &one_page("", ""),
        "<< /Length 0 >>",
        "<< /Length 0 /Filter /LZWDecode >>",
    );
    assert!(err_of(&rebuild_xref(&lzw)).contains("unsupported Filter LZWDecode"));
    let parms = replace_bytes(
        &one_page("", ""),
        "<< /Length 0 >>",
        "<< /Length 0 /Filter /FlateDecode /DecodeParms << /Predictor 12 >> >>",
    );
    assert!(err_of(&rebuild_xref(&parms)).contains("unsupported DecodeParms"));
}

/// The file with its xref table rebuilt from the "n 0 obj" lines and the
/// startxref pointing at it.
fn rebuild_xref(data: &[u8]) -> Vec<u8> {
    let xref_at = data.windows(5).rposition(|w| w == b"xref\n").unwrap();
    let body = &data[..xref_at];
    let offsets: Vec<usize> = (1..)
        .map_while(|i| {
            let tag = format!("\n{i} 0 obj\n").into_bytes();
            body.windows(tag.len())
                .position(|w| w == tag.as_slice())
                .map(|p| p + 1)
        })
        .collect();
    let mut out = body.to_vec();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).into_bytes());
    offsets
        .iter()
        .for_each(|o| out.extend(format!("{o:010} 00000 n \n").into_bytes()));
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            offsets.len() + 1,
            xref_at
        )
        .into_bytes(),
    );
    out
}

#[test]
fn length_may_be_an_indirect_integer() {
    let data = build_pdf(
        &[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] /Resources << >> /Contents 4 0 R >>",
            "<< /Length 5 0 R >>\nstream\nq Q\nendstream",
            "3",
        ],
        "",
    );
    assert_eq!(
        Document::parse(&data).unwrap().pages().unwrap()[0].contents,
        b"q Q"
    );
}

fn replace_bytes(data: &[u8], from: &str, to: &str) -> Vec<u8> {
    let at = data
        .windows(from.len())
        .position(|w| w == from.as_bytes())
        .unwrap();
    [&data[..at], to.as_bytes(), &data[at + from.len()..]].concat()
}
