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
            Operator::ShowText(vec![0, 1]),
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
        "TJ", "Td", "TD", "T*", "'", "\"", "Tc", "Tw", "TL", "Ts", "l", "v", "y", "S", "s", "f*",
        "B", "b", "W", "W*", "n", "J", "j", "M", "d", "gs", "sh", "BI", "d0", "d1", "g", "G", "rg",
        "RG", "sc", "SC", "SCN", "CS", "BX", "EX", "MP", "DP", "BMC", "ri", "i", "F", "B*", "b*",
        "Tk",
    ] {
        let e = err(&format!("q {op} Q"));
        assert_eq!(e, format!("offset 2: unsupported operator {op}"), "{op}");
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
