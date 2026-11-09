use super::*;
use crate::fixed::{Fixed, Matrix};

fn n(s: &str) -> Fixed {
    Fixed::parse(s.as_bytes()).unwrap()
}

fn ops(s: &str) -> Vec<Operator<'_>> {
    parse(s.as_bytes())
        .unwrap()
        .into_iter()
        .map(|op| op.operator)
        .collect()
}

fn err(s: &str) -> String {
    parse(s.as_bytes()).unwrap_err().to_string()
}

#[test]
fn operands_nest_to_64() {
    // docs/adr/0021: the dictionary of BDC is the first level.
    let content = |n: usize| format!("/Tag << /A {}1{} >> BDC EMC", "[".repeat(n), "]".repeat(n));
    assert_eq!(ops(&content(63)).len(), 2);
    let src = content(64);
    let e = parse(src.as_bytes()).unwrap_err();
    assert_eq!(e.message(), "arrays and dictionaries nested deeper than 64");
    assert_eq!(e.offset(), Some(src.find("[[").unwrap() + 63));
}

fn m(v: [&str; 6]) -> Matrix {
    Matrix::new(v.map(n))
}

#[test]
fn every_operator_of_the_table() {
    let src = "q 1 0 0 1 10 20 cm 0.5 w 1 2 m 1 2 3 4 5 6 c h 0 0 10 20 re f Q \
               0 0.1 0.2 1 k 1 0 0 0 K /Reg cs 1 scn \
               BT /F1 9.2126 Tf 50 Tz 1 0 0 1 3 4 Tm 2 Tr <0001> Tj 0 Tr ET \
               /Im1 Do /Span << /ActualText (x) >> BDC EMC /P /MC0 BDC EMC";
    assert_eq!(
        ops(src),
        vec![
            Operator::Save,
            Operator::Concat(m(["1", "0", "0", "1", "10", "20"])),
            Operator::LineWidth(n("0.5")),
            Operator::MoveTo(n("1"), n("2")),
            Operator::CurveTo(["1", "2", "3", "4", "5", "6"].map(n)),
            Operator::ClosePath,
            Operator::Rect(["0", "0", "10", "20"].map(n)),
            Operator::Fill,
            Operator::Restore,
            Operator::FillCmyk(["0", "0.1", "0.2", "1"].map(n)),
            Operator::StrokeCmyk(["1", "0", "0", "0"].map(n)),
            Operator::FillSpace(b"Reg".as_slice().into()),
            Operator::FillColor(n("1")),
            Operator::BeginText,
            Operator::Font(b"F1".as_slice().into(), n("9.2126")),
            Operator::HScale(n("50")),
            Operator::TextMatrix(m(["1", "0", "0", "1", "3", "4"])),
            Operator::RenderMode(2),
            Operator::ShowText(Text::Bytes(vec![0, 1])),
            Operator::RenderMode(0),
            Operator::EndText,
            Operator::Do(b"Im1".as_slice().into()),
            Operator::BeginMarked,
            Operator::EndMarked,
            Operator::BeginMarked,
            Operator::EndMarked,
        ]
    );
}

#[test]
fn offsets_point_at_the_operator() {
    let parsed = parse(b"q  1 w Q").unwrap();
    assert_eq!(
        parsed.iter().map(|op| op.offset).collect::<Vec<_>>(),
        vec![0, 5, 7]
    );
}

#[test]
fn wrong_operands_are_errors() {
    for (src, op) in [
        ("1 2 3 4 5 cm", "cm"),
        ("/A w", "w"),
        ("1 m", "m"),
        ("1 2 3 4 re f", "f"), // f takes nothing: the error is for the extra operand of the next case
        ("1 f", "f"),
        ("1 2 3 k", "k"),
        ("1 cs", "cs"),
        ("/A scn", "scn"),
        ("1 2 scn", "scn"),
        ("BT 1 Tf ET", "Tf"),
        ("BT /F1 /F2 Tf ET", "Tf"),
        ("BT 1 Tj ET", "Tj"),
        ("1 Do", "Do"),
        ("/P 1 BDC EMC", "BDC"),
        ("BDC EMC", "BDC"),
        ("1 q Q", "q"),
    ] {
        if src == "1 2 3 4 re f" {
            assert!(parse(src.as_bytes()).is_ok());
            continue;
        }
        let e = err(src);
        assert!(e.contains(&format!("operands of {op}")), "{src}: {e}");
    }
}

#[test]
fn unsupported_operators_are_errors() {
    for op in [
        "TJ", "Td", "TD", "T*", "'", "\"", "Tc", "Tw", "TL", "Ts", "v", "y", "s", "f*", "B", "b",
        "W", "W*", "n", "j", "M", "gs", "sh", "BI", "d0", "d1", "g", "G", "rg", "RG", "sc", "SC",
        "SCN", "CS", "BX", "EX", "MP", "DP", "BMC", "ri", "i", "F", "B*", "b*", "Tk",
    ] {
        let e = err(&format!("q {op} Q"));
        assert_eq!(e, format!("offset 2: unsupported operator {op}"), "{op}");
    }
}

// --- the strokes of the paths of a page: l, S, J and d (docs/adr/0025)

#[test]
fn the_operators_of_a_stroke() {
    assert_eq!(
        ops("1 J [3 2] 0 d 1 2 m 3 4 l S [] 0 d 0 J"),
        vec![
            Operator::LineCap(1),
            Operator::Dash([n("3"), n("2")].into(), n("0")),
            Operator::MoveTo(n("1"), n("2")),
            Operator::LineTo(n("3"), n("4")),
            Operator::Stroke,
            Operator::Dash([].into(), n("0")),
            Operator::LineCap(0),
        ]
    );
    for (src, op) in [
        ("1 2 m 1 l S", "l"),
        ("1 2 m /A 1 l S", "l"),
        ("1 2 m 3 4 l 1 S", "S"),
        ("1.0 J", "J"),
        ("/A J", "J"),
        ("J", "J"),
        ("3 d", "d"),
        ("[3] d", "d"),
        ("[3] /A d", "d"),
        ("[3] 1 2 d", "d"),
    ] {
        let e = err(src);
        assert!(e.contains(&format!("operands of {op}")), "{src}: {e}");
    }
}

#[test]
fn line_caps_other_than_butt_and_round() {
    assert!(parse(b"0 J").is_ok());
    assert!(parse(b"1 J").is_ok());
    assert_eq!(err("2 J"), "offset 2: unsupported J 2");
    assert_eq!(err("3 J"), "offset 2: malformed J 3");
    assert_eq!(err("-1 J"), "offset 3: malformed J -1");
}

#[test]
fn dash_arrays() {
    for src in [
        "[] 0 d",
        "[3 2] 0 d",
        "[0 3] 0 d",
        "[3] 1.5 d",
        "[0 0.5] 2 d",
    ] {
        assert!(parse(src.as_bytes()).is_ok(), "{src}");
    }
    assert_eq!(err("[0 0] 0 d"), "offset 8: malformed d");
    assert_eq!(err("[0] 0 d"), "offset 6: malformed d");
    for src in [
        "[-1 2] 0 d",
        "[3 2] -1 d",
        "[/a] 0 d",
        "[3 (x)] 0 d",
        "[[3]] 0 d",
    ] {
        let e = err(src);
        assert!(e.contains("operands of d"), "{src}: {e}");
    }
}

#[test]
fn where_the_operators_of_a_stroke_may_appear() {
    assert!(err("1 2 l").contains("l without a current point"));
    assert!(err("1 2 m 1 J").contains("J inside a path"));
    assert!(err("1 2 m [3] 0 d").contains("d inside a path"));
    assert!(err("BT 1 2 l ET").contains("l without a current point"));
    assert!(err("BT S ET").contains("S inside a text object"));
    assert!(err("1 2 m 3 4 l").contains("path not painted"));
    // the caps and the dash are allowed in a text object, as w is
    assert!(parse(b"BT 1 J [3 2] 0 d ET").is_ok());
    // S ends the path; after h, l starts a new subpath
    assert!(parse(b"1 2 m 3 4 l h 5 6 l S q Q").is_ok());
    assert!(parse(b"0 0 1 1 re S").is_ok());
    assert!(parse(b"1 2 m 1 2 3 4 5 6 c 7 8 l S").is_ok());
    // S with no path paints nothing
    assert!(parse(b"S").is_ok());
}

#[test]
fn the_strokes_that_golem_writes() {
    // The three outputs of the test of Golem's Writer (docs/adr/0025).
    for src in [
        "0.5 w\n0 0 m\n1 0 l\nS\n",
        "1 w [3 2] 0 d\n0 0 m\n1 0 l\n1 1 l\nh S\n",
        "1 w 1 J [0 3] 0 d\n0 0 m\n1 0 l\nS\n",
    ] {
        assert!(parse(src.as_bytes()).is_ok(), "{src}");
    }
}

#[test]
fn text_render_modes_other_than_0_and_2() {
    assert!(err("1 Tr").contains("unsupported Tr 1"));
    assert!(err("3 Tr").contains("unsupported Tr 3"));
    assert!(err("0.5 Tr").contains("operands of Tr"));
}

#[test]
fn the_stack_of_graphics_states() {
    assert!(err("Q").contains("Q without q"));
    assert!(err("q q Q").contains("q not closed"));
    assert!(parse(b"q q Q Q").is_ok());
}

#[test]
fn text_objects() {
    assert!(err("BT BT ET ET").contains("BT inside a text object"));
    assert!(err("ET").contains("ET outside a text object"));
    assert!(err("1 0 0 1 0 0 Tm").contains("Tm outside a text object"));
    assert!(err("<0001> Tj").contains("Tj outside a text object"));
    assert!(err("BT").contains("BT not closed"));
    for op in ["q", "Q", "1 0 0 1 0 0 cm", "0 0 m", "0 0 1 1 re", "/Im1 Do"] {
        let e = err(&format!("BT {op} ET"));
        assert!(e.contains("inside a text object"), "{op}: {e}");
    }
    // the text state, the colors and the line width are allowed in a text object
    assert!(
        parse(b"BT /F1 1 Tf 100 Tz 0 Tr 1 w 0 0 0 1 k 0 0 0 1 K /Reg cs 1 scn /P /MC0 BDC EMC ET")
            .is_ok()
    );
    // and the text state outside of it
    assert!(parse(b"/F1 1 Tf 100 Tz 2 Tr").is_ok());
}

#[test]
fn paths() {
    assert!(err("1 2 3 4 5 6 c").contains("c without a current point"));
    assert!(err("h").contains("h without a current point"));
    assert!(err("0 0 m q").contains("q inside a path"));
    assert!(err("0 0 m BT ET").contains("BT inside a path"));
    assert!(err("0 0 m").contains("path not painted"));
    assert!(parse(b"0 0 m 1 2 3 4 5 6 c h f").is_ok());
    assert!(parse(b"0 0 1 1 re 1 2 3 4 5 6 c f").is_ok());
    // f with no path paints nothing
    assert!(parse(b"f").is_ok());
}

#[test]
fn marked_content() {
    assert!(err("EMC").contains("EMC without BDC"));
    assert!(err("/P << >> BDC").contains("BDC not closed"));
    assert!(err("/P BDC EMC").contains("operands of BDC"));
    assert!(parse(b"/P /MC0 BDC q /P << /MCID 1 >> BDC EMC Q EMC").is_ok());
}

#[test]
fn comments_and_line_ends() {
    assert_eq!(
        ops("q % a comment Q\r\nQ"),
        vec![Operator::Save, Operator::Restore]
    );
}

#[test]
fn operands_left_at_the_end_are_errors() {
    assert!(err("q Q 1 2").contains("operands without an operator"));
}

#[test]
fn numbers_as_operands() {
    // docs/adr/0019: numbers stay as tokens on the stack of operands; the
    // values and the errors are those of objects.
    assert_eq!(
        ops("-0 2147483647 m -2147483647 .5 -.25 +3 re f"),
        vec![
            Operator::MoveTo(n("0"), n("2147483647")),
            Operator::Rect(["-2147483647", "0.5", "-0.25", "3"].map(n)),
            Operator::Fill,
        ]
    );
    assert_eq!(err("2147483648 0 m f"), "offset 13: wrong operands of m");
    assert_eq!(err("-2147483648 0 m f"), "offset 14: wrong operands of m");
    assert_eq!(err("2.0 Tr"), "offset 4: wrong operands of Tr");
    assert_eq!(err("/N Tr"), "offset 3: wrong operands of Tr");
    assert!(parse(b"2 Tr").is_ok());
}

// --- the operands kept as tokens (docs/adr/0019)

/// The operators in a compact form, with the numbers as `Fixed` displays
/// them: the shape of what reached each operator, not the exact value,
/// which the tests of the lexer and of `Fixed::parse` cover.
fn show(ops: &[Op]) -> String {
    use Operator::*;
    let nums = |v: &[Fixed]| {
        v.iter()
            .map(|f| f.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let name = |n: &[u8]| format!("/{}", String::from_utf8_lossy(n));
    let matrix = |m: &Matrix| nums(&[m.a, m.b, m.c, m.d, m.e, m.f]);
    let show_op = |op: &Operator| match op {
        Save => "q".into(),
        Restore => "Q".into(),
        Concat(m) => format!("cm({})", matrix(m)),
        LineWidth(v) => format!("w({v})"),
        MoveTo(x, y) => format!("m({x},{y})"),
        LineTo(x, y) => format!("l({x},{y})"),
        CurveTo(v) => format!("c({})", nums(v)),
        ClosePath => "h".into(),
        Rect(v) => format!("re({})", nums(v)),
        Fill => "f".into(),
        Stroke => "S".into(),
        LineCap(c) => format!("J({c})"),
        Dash(a, p) => format!("d([{}],{p})", nums(a)),
        FillCmyk(v) => format!("k({})", nums(v)),
        StrokeCmyk(v) => format!("K({})", nums(v)),
        FillSpace(n) => format!("cs({})", name(n)),
        FillColor(v) => format!("scn({v})"),
        BeginText => "BT".into(),
        EndText => "ET".into(),
        Font(n, s) => format!("Tf({},{s})", name(n)),
        HScale(v) => format!("Tz({v})"),
        TextMatrix(m) => format!("Tm({})", matrix(m)),
        RenderMode(m) => format!("Tr({m})"),
        ShowText(t) => format!(
            "Tj(<{}>)",
            t.bytes().map(|b| format!("{b:02x}")).collect::<String>()
        ),
        Do(n) => format!("Do({})", name(n)),
        BeginMarked => "BDC".into(),
        EndMarked => "EMC".into(),
    };
    ops.iter()
        .map(|op| show_op(&op.operator))
        .collect::<Vec<_>>()
        .join(" ")
}

fn result(s: &str) -> String {
    match parse(s.as_bytes()) {
        Ok(ops) => show(&ops),
        Err(e) => format!("error: {e}"),
    }
}

/// Numbers of every shape as the operands of every operator of the table,
/// and the operands that are not numbers where numbers are expected, with
/// the results that the parser gave before docs/adr/0019 (f60c40c): the
/// operator or the message of the error, with its offset.
///
/// The cases are those of a differential run of the parsers of f60c40c
/// and of docs/adr/0019 on 1,200,000 generated content streams (three
/// kinds: random tokens, operators with mostly fitting operands, and the
/// whole table with numbers of every shape), which gave the same
/// operators, offsets and messages for every case. The expected values
/// here were written by the parser of f60c40c.
const CASES_OF_OPERANDS: &[(&str, &str)] = &[
    ("-0 +3 m .5 5. -.25 007 re f", "m(0,3) re(0.5,5,-0.25,7) f"),
    (
        "2.0 1.5 0.1 9.2126 k 100 50 2147483647 -2147483647 K",
        "k(2,1.5,0.1,9.212599) K(100,50,2147483647,-2147483647)",
    ),
    ("2147483647.5 w", "w(2147483647.5)"),
    ("-2147483647.5 w", "error: offset 14: wrong operands of w"),
    (
        "2147483647.5 w -2147483647.5 w",
        "error: offset 29: wrong operands of w",
    ),
    ("0.00000000023283064365386962890625 w", "w(0)"),
    ("0.000000000116415321826934814453125 w", "w(0)"),
    ("00000000000000000000001 w", "w(1)"),
    ("1.00000000000000000000001 w", "w(1)"),
    ("-00.000 w", "w(0)"),
    ("-.0 w", "w(0)"),
    ("-1 w", "error: offset 3: wrong operands of w"),
    ("-0 w", "w(0)"),
    ("+0.0 w", "w(0)"),
    ("2147483648 0 m f", "error: offset 13: wrong operands of m"),
    ("-2147483648 0 m f", "error: offset 14: wrong operands of m"),
    ("0 2147483648 m f", "error: offset 13: wrong operands of m"),
    (
        "2147483647.9999999999 w",
        "error: offset 0: number out of range: 2147483647.9999999999",
    ),
    (
        "-2147483647.9999999999 w",
        "error: offset 0: number out of range: -2147483647.9999999999",
    ),
    (
        "9223372036854775807 w",
        "error: offset 20: wrong operands of w",
    ),
    (
        "9223372036854775808 w",
        "error: offset 0: integer out of range: 9223372036854775808",
    ),
    (
        "-9223372036854775808 w",
        "error: offset 21: wrong operands of w",
    ),
    (
        "-9223372036854775809 w",
        "error: offset 0: integer out of range: -9223372036854775809",
    ),
    (
        "123456789012345678901234567890.5 w",
        "error: offset 0: number out of range: 123456789012345678901234567890.5",
    ),
    ("1e5 w", "error: offset 0: unsupported operator 1e5"),
    ("1.2.3 w", "error: offset 0: unsupported operator 1.2.3"),
    ("+ w", "error: offset 0: unsupported operator +"),
    ("- w", "error: offset 0: unsupported operator -"),
    (". w", "error: offset 0: unsupported operator ."),
    ("--1 w", "error: offset 0: unsupported operator --1"),
    ("+-1 w", "error: offset 0: unsupported operator +-1"),
    ("1-2 w", "error: offset 0: unsupported operator 1-2"),
    ("1 0 R", "error: offset 4: unsupported operator R"),
    ("1 cs", "error: offset 2: wrong operands of cs"),
    ("1.0 cs", "error: offset 4: wrong operands of cs"),
    ("1 Do", "error: offset 2: wrong operands of Do"),
    ("2.5 Do", "error: offset 4: wrong operands of Do"),
    ("BT 1 Tj ET", "error: offset 5: wrong operands of Tj"),
    ("BT 1.5 Tj ET", "error: offset 7: wrong operands of Tj"),
    ("BT /F1 Tj ET", "error: offset 7: wrong operands of Tj"),
    ("/F1 /F2 Tf", "error: offset 8: wrong operands of Tf"),
    ("1 2 Tf", "error: offset 4: wrong operands of Tf"),
    ("/F1 (1) Tf", "error: offset 8: wrong operands of Tf"),
    ("/F1 <01> Tf", "error: offset 9: wrong operands of Tf"),
    ("/F1 [1] Tf", "error: offset 8: wrong operands of Tf"),
    (
        "/F1 2147483648 Tf",
        "error: offset 15: wrong operands of Tf",
    ),
    ("/F1 -1 Tf", "Tf(/F1,-1)"),
    ("/F1 +1.5 Tf", "Tf(/F1,1.5)"),
    ("1 /P BDC EMC", "error: offset 5: wrong operands of BDC"),
    ("/P 1 BDC EMC", "error: offset 5: wrong operands of BDC"),
    ("/P 1.0 BDC EMC", "error: offset 7: wrong operands of BDC"),
    ("/P [1] BDC EMC", "error: offset 7: wrong operands of BDC"),
    ("/P (x) BDC EMC", "error: offset 7: wrong operands of BDC"),
    ("/P <01> BDC EMC", "error: offset 8: wrong operands of BDC"),
    ("/P << /MCID 1 >> BDC EMC", "BDC EMC"),
    ("/P << /MCID 2147483648 >> BDC EMC", "BDC EMC"),
    (
        "/P << /MCID 9223372036854775808 >> BDC EMC",
        "error: offset 12: integer out of range: 9223372036854775808",
    ),
    (
        "/P << /A 2147483647.9999999999 >> BDC EMC",
        "error: offset 9: number out of range: 2147483647.9999999999",
    ),
    (
        "/P << /A 1 0 R >> BDC EMC",
        "error: offset 11: dictionary key is not a name",
    ),
    ("/P << /A [1 2.0 /B] >> BDC EMC", "BDC EMC"),
    (
        "/P << /A 1.2.3 >> BDC EMC",
        "error: offset 9: unexpected keyword 1.2.3",
    ),
    (
        "/P << /A q >> BDC EMC",
        "error: offset 9: unexpected keyword q",
    ),
    (
        "/P << /A >> BDC EMC",
        "error: offset 9: dictionary key without a value",
    ),
    (
        "/P << 1 2 >> BDC EMC",
        "error: offset 6: dictionary key is not a name",
    ),
    ("/P [1 2] BDC EMC", "error: offset 9: wrong operands of BDC"),
    ("[1 2] w", "error: offset 6: wrong operands of w"),
    ("<< /A 1 >> w", "error: offset 11: wrong operands of w"),
    ("[1] 0 m f", "error: offset 6: wrong operands of m"),
    ("true w", "error: offset 5: wrong operands of w"),
    ("null 0 m f", "error: offset 7: wrong operands of m"),
    ("false Tr", "error: offset 6: wrong operands of Tr"),
    ("/F1 true Tf", "error: offset 9: wrong operands of Tf"),
    ("true cs", "error: offset 5: wrong operands of cs"),
    ("null Do", "error: offset 5: wrong operands of Do"),
    ("0 Tr", "Tr(0)"),
    ("2 Tr", "Tr(2)"),
    ("+2 Tr", "Tr(2)"),
    ("-0 Tr", "Tr(0)"),
    ("002 Tr", "Tr(2)"),
    ("2.0 Tr", "error: offset 4: wrong operands of Tr"),
    ("0.0 Tr", "error: offset 4: wrong operands of Tr"),
    ("2. Tr", "error: offset 3: wrong operands of Tr"),
    ("1 Tr", "error: offset 2: unsupported Tr 1"),
    ("-1 Tr", "error: offset 3: unsupported Tr -1"),
    ("3 Tr", "error: offset 2: unsupported Tr 3"),
    (
        "2147483648 Tr",
        "error: offset 11: unsupported Tr 2147483648",
    ),
    (
        "-2147483648 Tr",
        "error: offset 12: unsupported Tr -2147483648",
    ),
    (
        "9223372036854775807 Tr",
        "error: offset 20: unsupported Tr 9223372036854775807",
    ),
    (
        "-9223372036854775808 Tr",
        "error: offset 21: unsupported Tr -9223372036854775808",
    ),
    (
        "9223372036854775808 Tr",
        "error: offset 0: integer out of range: 9223372036854775808",
    ),
    ("1 2 Tr", "error: offset 4: wrong operands of Tr"),
    ("2 2 Tr", "error: offset 4: wrong operands of Tr"),
    ("Tr", "error: offset 0: wrong operands of Tr"),
    ("true Tr", "error: offset 5: wrong operands of Tr"),
    ("null Tr", "error: offset 5: wrong operands of Tr"),
    ("(2) Tr", "error: offset 4: wrong operands of Tr"),
    ("<02> Tr", "error: offset 5: wrong operands of Tr"),
    ("[2] Tr", "error: offset 4: wrong operands of Tr"),
    ("<< >> Tr", "error: offset 6: wrong operands of Tr"),
    ("/2 Tr", "error: offset 3: wrong operands of Tr"),
    ("1 2 3 m f", "error: offset 6: wrong operands of m"),
    ("1 m f", "error: offset 2: wrong operands of m"),
    ("1 2 3 4 5 6 7 cm", "error: offset 14: wrong operands of cm"),
    ("1 2 3 4 5 cm", "error: offset 10: wrong operands of cm"),
    ("cm", "error: offset 0: wrong operands of cm"),
    ("w", "error: offset 0: wrong operands of w"),
    ("1 2 w", "error: offset 4: wrong operands of w"),
    ("q 1 Q", "error: offset 4: wrong operands of Q"),
    ("0 0 m 1 h f", "error: offset 8: wrong operands of h"),
    ("0 0 m 1 f", "error: offset 8: wrong operands of f"),
    ("1 BT ET", "error: offset 2: wrong operands of BT"),
    ("BT 1 ET", "error: offset 5: wrong operands of ET"),
    (
        "/P /MC0 BDC 1 EMC",
        "error: offset 14: wrong operands of EMC",
    ),
    (
        "1 2",
        "error: offset 3: operands without an operator at the end of the content",
    ),
    (
        "q Q 1.5",
        "error: offset 7: operands without an operator at the end of the content",
    ),
    (
        "q Q /A",
        "error: offset 6: operands without an operator at the end of the content",
    ),
    (
        "q Q (x)",
        "error: offset 7: operands without an operator at the end of the content",
    ),
    (
        "q Q <0001>",
        "error: offset 10: operands without an operator at the end of the content",
    ),
    (
        "q Q [1]",
        "error: offset 7: operands without an operator at the end of the content",
    ),
    (
        "q Q << >>",
        "error: offset 9: operands without an operator at the end of the content",
    ),
    (
        "q Q true",
        "error: offset 8: operands without an operator at the end of the content",
    ),
    ("BT/F1 1 Tf(x)Tj ET", "BT Tf(/F1,1) Tj(<78>) ET"),
    ("BT /F1 1 Tf <0001>Tj ET", "BT Tf(/F1,1) Tj(<0001>) ET"),
    ("1/Reg cs", "error: offset 6: wrong operands of cs"),
    (
        "0 0 10 20 re 1 2 3 4 5 6 c h f",
        "re(0,0,10,20) c(1,2,3,4,5,6) h f",
    ),
    (
        "1 0 0 1 10 20 cm BT 1 0 0 1 3 4 Tm ET",
        "cm(1,0,0,1,10,20) BT Tm(1,0,0,1,3,4) ET",
    ),
    ("BT 1 0 0 1 3 4.5 Tm ET", "BT Tm(1,0,0,1,3,4.5) ET"),
    (
        "+1 -0 +0 1. .0 -.0 Tm",
        "error: offset 19: Tm outside a text object",
    ),
    ("BT +1 -0 +0 1. .0 -.0 Tm ET", "BT Tm(1,0,0,1,0,0) ET"),
    ("1 2 3 4 k 1 2 3 4 K", "k(1,2,3,4) K(1,2,3,4)"),
    ("1 2 3 k", "error: offset 6: wrong operands of k"),
    ("1 2 3 4 5 K", "error: offset 10: wrong operands of K"),
    ("0 scn", "scn(0)"),
    ("0.5 scn", "scn(0.5)"),
    ("-0.5 scn", "scn(-0.5)"),
    ("2147483648 scn", "error: offset 11: wrong operands of scn"),
    ("1 2 scn", "error: offset 4: wrong operands of scn"),
    ("100 Tz", "Tz(100)"),
    ("-100 Tz", "Tz(-100)"),
    ("2147483648 Tz", "error: offset 11: wrong operands of Tz"),
    ("q 1 2 3 4 5 6 cm Q", "q cm(1,2,3,4,5,6) Q"),
    (
        "q 2147483648 0 0 1 0 0 cm Q",
        "error: offset 23: wrong operands of cm",
    ),
    (
        "q 1 0 0 1 0 2147483647.9999999999 cm Q",
        "error: offset 12: number out of range: 2147483647.9999999999",
    ),
];

#[test]
fn operands_of_every_shape_give_what_they_gave_before() {
    CASES_OF_OPERANDS.iter().for_each(|&(content, expected)| {
        assert_eq!(result(content), expected, "{content}");
    });
}

#[test]
fn a_number_token_is_the_fixed_of_its_text() {
    // docs/adr/0019: an integer token is `Fixed::from_int` of its i64, and
    // a real token is `Fixed::parse` of its text; both are what
    // `Fixed::parse` makes of the token.
    [
        "0",
        "-0",
        "+3",
        "007",
        "-2147483647",
        "2147483647",
        "00000000000000000000001",
        ".5",
        "5.",
        "-.25",
        "2.0",
        "0.1",
        "9.2126",
        "2147483647.5",
        "0.00000000023283064365386962890625",
        "0.000000000116415321826934814453125",
    ]
    .iter()
    .for_each(|t| {
        let v = n(t);
        assert_eq!(
            ops(&format!("{t} {t} m f")),
            vec![Operator::MoveTo(v, v), Operator::Fill],
            "{t}"
        );
    });
}

#[test]
fn an_integer_is_an_operand_when_it_is_a_fixed() {
    // docs/adr/0019: an integer that `Fixed::from_int` refuses is a wrong
    // operand, as the Int object was; `Tr` takes any i64.
    [
        2147483647i64,
        2147483648,
        -2147483647,
        -2147483648,
        i64::MAX,
        i64::MIN,
    ]
    .iter()
    .for_each(|&i| {
        let fits = Fixed::from_int(i).is_ok();
        assert_eq!(parse(format!("{i} 0 m f").as_bytes()).is_ok(), fits, "{i}");
        assert_eq!(
            result(&format!("{i} Tr")),
            format!(
                "error: offset {}: unsupported Tr {i}",
                i.to_string().len() + 1
            )
        );
    });
}
