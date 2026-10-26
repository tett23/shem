use super::*;
use crate::font::Segment;

fn n(s: &str) -> Fixed {
    Fixed::parse(s.as_bytes()).unwrap()
}

/// A number of a Type 2 charstring.
fn num(v: i32) -> Vec<u8> {
    match v {
        -107..=107 => vec![(v + 139) as u8],
        108..=1131 => {
            let w = v - 108;
            vec![(w / 256 + 247) as u8, (w % 256) as u8]
        }
        -1131..=-108 => {
            let w = -v - 108;
            vec![(w / 256 + 251) as u8, (w % 256) as u8]
        }
        _ => {
            let b = (v as i16).to_be_bytes();
            vec![28, b[0], b[1]]
        }
    }
}

/// A charstring from numbers and operators: operators are given as negative
/// values -op (or -(1200 + op) for escaped operators).
fn cs(items: &[i32]) -> Vec<u8> {
    items
        .iter()
        .flat_map(|&v| match v {
            v if v <= -1200 => vec![12, (-v - 1200) as u8],
            v if v < 0 => vec![(-v) as u8],
            _ => num(v),
        })
        .collect()
}

/// A charstring with a number that may be negative: pushes `v`.
fn neg(v: i32) -> Vec<u8> {
    num(v)
}

const RMOVETO: i32 = -21;
const HMOVETO: i32 = -22;
const VMOVETO: i32 = -4;
const RLINETO: i32 = -5;
const HLINETO: i32 = -6;
const VLINETO: i32 = -7;
const RRCURVETO: i32 = -8;
const HHCURVETO: i32 = -27;
const VVCURVETO: i32 = -26;
const HVCURVETO: i32 = -31;
const VHCURVETO: i32 = -30;
const RCURVELINE: i32 = -24;
const RLINECURVE: i32 = -25;
const ENDCHAR: i32 = -14;
const HSTEM: i32 = -1;
const VSTEM: i32 = -3;
const HSTEMHM: i32 = -18;
const HINTMASK: i32 = -19;
const CALLSUBR: i32 = -10;
const CALLGSUBR: i32 = -29;
const RETURN: i32 = -11;

fn outline(charstring: &[u8]) -> Vec<Segment> {
    charstring_outline(charstring, &[], &[]).unwrap()
}

fn p(x: i32, y: i32) -> (Fixed, Fixed) {
    (
        Fixed::from_int(x.into()).unwrap(),
        Fixed::from_int(y.into()).unwrap(),
    )
}

fn mv(x: i32, y: i32) -> Segment {
    let (x, y) = p(x, y);
    Segment::Move(x, y)
}

fn ln(x: i32, y: i32) -> Segment {
    let (x, y) = p(x, y);
    Segment::Line(x, y)
}

fn cv(v: [i32; 6]) -> Segment {
    Segment::Curve(v.map(|c| Fixed::from_int(c.into()).unwrap()))
}

#[test]
fn moves_and_lines() {
    let got = outline(&cs(&[
        10, 20, RMOVETO, 5, 0, 0, 5, RLINETO, 3, 4, HLINETO, 2, VLINETO, ENDCHAR,
    ]));
    assert_eq!(
        got,
        vec![
            mv(10, 20),
            ln(15, 20),
            ln(15, 25),
            ln(18, 25),
            ln(18, 29),
            ln(18, 31),
            Segment::Close
        ]
    );
    let got = outline(&cs(&[
        7, HMOVETO, 1, VLINETO, 8, VMOVETO, 2, HLINETO, ENDCHAR,
    ]));
    assert_eq!(
        got,
        vec![
            mv(7, 0),
            ln(7, 1),
            Segment::Close,
            mv(7, 9),
            ln(9, 9),
            Segment::Close
        ]
    );
}

#[test]
fn the_curve_operators_and_their_optional_arguments() {
    // rrcurveto
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, 5, 6, RRCURVETO, ENDCHAR]))[1],
        cv([1, 2, 4, 6, 9, 12])
    );
    // hhcurveto without and with the leading dy1
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, HHCURVETO, ENDCHAR]))[1],
        cv([1, 0, 3, 3, 7, 3])
    );
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 9, 1, 2, 3, 4, HHCURVETO, ENDCHAR]))[1],
        cv([1, 9, 3, 12, 7, 12])
    );
    // vvcurveto without and with the leading dx1
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, VVCURVETO, ENDCHAR]))[1],
        cv([0, 1, 2, 4, 2, 8])
    );
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 9, 1, 2, 3, 4, VVCURVETO, ENDCHAR]))[1],
        cv([9, 1, 11, 4, 11, 8])
    );
    // hvcurveto: horizontal start, vertical end, and the final dx when there are 5 arguments
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, HVCURVETO, ENDCHAR]))[1],
        cv([1, 0, 3, 3, 3, 7])
    );
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, 5, HVCURVETO, ENDCHAR]))[1],
        cv([1, 0, 3, 3, 8, 7])
    );
    // vhcurveto, and two curves that alternate
    assert_eq!(
        outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, VHCURVETO, ENDCHAR]))[1],
        cv([0, 1, 2, 4, 6, 4])
    );
    let two = outline(&cs(&[
        0, 0, RMOVETO, 1, 2, 3, 4, 5, 6, 7, 8, VHCURVETO, ENDCHAR,
    ]));
    assert_eq!(two[1], cv([0, 1, 2, 4, 6, 4]));
    assert_eq!(two[2], cv([11, 4, 17, 11, 17, 19]));
    // rcurveline and rlinecurve
    let cl = outline(&cs(&[
        0, 0, RMOVETO, 1, 2, 3, 4, 5, 6, 7, 8, RCURVELINE, ENDCHAR,
    ]));
    assert_eq!(cl[1..3], [cv([1, 2, 4, 6, 9, 12]), ln(16, 20)]);
    let lc = outline(&cs(&[
        0, 0, RMOVETO, 7, 8, 1, 2, 3, 4, 5, 6, RLINECURVE, ENDCHAR,
    ]));
    assert_eq!(lc[1..3], [ln(7, 8), cv([8, 10, 11, 14, 16, 20])]);
}

#[test]
fn flex_operators_are_two_curves() {
    let flex = outline(&cs(&[
        0, 0, RMOVETO, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 50, -1235, ENDCHAR,
    ]));
    assert_eq!(
        flex[1..3],
        [cv([1, 2, 4, 6, 9, 12]), cv([16, 20, 25, 30, 36, 42])]
    );
    let hflex = outline(&cs(&[0, 0, RMOVETO, 1, 2, 3, 4, 5, 6, 7, -1234, ENDCHAR]));
    assert_eq!(
        hflex[1..3],
        [cv([1, 0, 3, 3, 7, 3]), cv([12, 3, 18, 0, 25, 0])]
    );
    let hflex1 = outline(&cs(&[
        0, 0, RMOVETO, 1, 2, 3, 4, 5, 6, 7, 8, 9, -1236, ENDCHAR,
    ]));
    assert_eq!(
        hflex1[1..3],
        [cv([1, 2, 4, 6, 9, 6]), cv([15, 6, 22, 14, 31, 0])]
    );
    // flex1: the last argument is dx because |dx| > |dy| in total
    let flex1 = outline(&cs(&[
        0, 0, RMOVETO, 10, 1, 10, 1, 10, 1, 10, 1, 10, 1, 10, -1237, ENDCHAR,
    ]));
    assert_eq!(
        flex1[1..3],
        [cv([10, 1, 20, 2, 30, 3]), cv([40, 4, 50, 5, 60, 0])]
    );
}

#[test]
fn the_width_is_read_and_ignored() {
    let without = outline(&cs(&[10, 20, RMOVETO, 5, 5, RLINETO, ENDCHAR]));
    let with = outline(&cs(&[500, 10, 20, RMOVETO, 5, 5, RLINETO, ENDCHAR]));
    assert_eq!(with, without);
    let with_hmoveto = outline(&cs(&[500, 10, HMOVETO, ENDCHAR]));
    assert_eq!(with_hmoveto, vec![mv(10, 0), Segment::Close]);
    let with_stem = outline(&cs(&[500, 0, 10, HSTEM, 10, 20, RMOVETO, ENDCHAR]));
    assert_eq!(with_stem, vec![mv(10, 20), Segment::Close]);
    assert_eq!(outline(&cs(&[500, ENDCHAR])), vec![]);
}

#[test]
fn hintmask_bytes_follow_the_number_of_stems() {
    // 8 stems: one byte of mask; the byte 0xFF would be a wrong operator
    let mut eight = cs(&[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, HSTEMHM, HINTMASK,
    ]);
    eight.push(0xff);
    eight.extend(cs(&[1, 1, RMOVETO, ENDCHAR]));
    assert_eq!(outline(&eight), vec![mv(1, 1), Segment::Close]);
    // 9 stems: two bytes
    let mut nine = cs(&[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, HSTEMHM, HINTMASK,
    ]);
    nine.extend([0xff, 0x80]);
    nine.extend(cs(&[1, 1, RMOVETO, ENDCHAR]));
    assert_eq!(outline(&nine), vec![mv(1, 1), Segment::Close]);
    // the operands before hintmask are an implicit vstem: 8 + 1 = 9 stems
    let mut implicit = cs(&[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, HSTEMHM, 0, 1, HINTMASK,
    ]);
    implicit.extend([0xff, 0x80]);
    implicit.extend(cs(&[1, 1, RMOVETO, ENDCHAR]));
    assert_eq!(outline(&implicit), vec![mv(1, 1), Segment::Close]);
    let _ = VSTEM;
}

#[test]
fn subroutine_biases() {
    assert_eq!(bias(0), 107);
    assert_eq!(bias(1239), 107);
    assert_eq!(bias(1240), 1131);
    assert_eq!(bias(33899), 1131);
    assert_eq!(bias(33900), 32768);
}

#[test]
fn calls_to_local_and_global_subroutines() {
    let local: Vec<Vec<u8>> = (0..3).map(|i| cs(&[i + 1, 0, RLINETO, RETURN])).collect();
    let global: Vec<Vec<u8>> = vec![cs(&[0, 5, RLINETO, RETURN])];
    // subroutine 1 of 3 is called with 1 - 107; global 0 with 0 - 107
    let mut charstring = cs(&[0, 0, RMOVETO]);
    charstring.extend(neg(1 - 107));
    charstring.extend(cs(&[CALLSUBR]));
    charstring.extend(neg(-107));
    charstring.extend(cs(&[CALLGSUBR, ENDCHAR]));
    let got = charstring_outline(&charstring, &global, &local).unwrap();
    assert_eq!(got, vec![mv(0, 0), ln(2, 0), ln(2, 5), Segment::Close]);
    // a subroutine that ends the glyph
    let ending = vec![cs(&[3, 3, RLINETO, ENDCHAR])];
    let mut charstring = cs(&[0, 0, RMOVETO]);
    charstring.extend(neg(-107));
    charstring.extend(cs(&[CALLSUBR]));
    assert_eq!(
        charstring_outline(&charstring, &[], &ending).unwrap(),
        vec![mv(0, 0), ln(3, 3), Segment::Close]
    );
}

#[test]
fn errors() {
    let e = |c: &[u8]| charstring_outline(c, &[], &[]).unwrap_err().to_string();
    // arithmetic and storage operators
    for op in [
        3, 4, 5, 9, 10, 11, 12, 14, 15, 18, 20, 21, 22, 23, 24, 26, 27, 28, 29, 30,
    ] {
        assert!(
            e(&cs(&[1, 2, -(1200 + op), ENDCHAR])).contains("unsupported charstring operator"),
            "12 {op}"
        );
    }
    assert!(e(&cs(&[-1223, ENDCHAR])).contains("12 23"));
    // endchar with four arguments (seac)
    assert!(e(&cs(&[0, 0, 65, 66, ENDCHAR])).contains("seac"));
    // a subroutine that calls itself
    let me = vec![{
        let mut c = neg(-107);
        c.extend(cs(&[CALLSUBR]));
        c
    }];
    let mut charstring = neg(-107);
    charstring.extend(cs(&[CALLSUBR]));
    assert!(
        charstring_outline(&charstring, &[], &me)
            .unwrap_err()
            .to_string()
            .contains("too deep")
    );
    // a missing subroutine
    let mut missing = neg(5 - 107);
    missing.extend(cs(&[CALLSUBR]));
    assert!(e(&missing).contains("subroutine"));
    // more than 48 arguments
    let many: Vec<i32> = (0..49).collect();
    assert!(e(&cs(&many)).contains("stack"));
    // no endchar
    assert!(e(&cs(&[0, 0, RMOVETO])).contains("endchar"));
}

#[test]
fn fixed_numbers_and_short_integers() {
    // 255 introduces a 16.16 number: 1.5 is 0x00018000
    let mut c = vec![255, 0, 1, 0x80, 0];
    c.extend(cs(&[0, RMOVETO, ENDCHAR]));
    assert_eq!(
        outline(&c),
        vec![Segment::Move(n("1.5"), n("0")), Segment::Close]
    );
    let mut c = vec![28, 0x07, 0xD0];
    c.extend(cs(&[0, RMOVETO, ENDCHAR]));
    assert_eq!(outline(&c), vec![mv(2000, 0), Segment::Close]);
}

// --- the tables

#[test]
fn indexes() {
    let data = [0, 2, 1, 1, 3, 4, b'a', b'b', b'c', 0xEE];
    let (items, end) = parse_index(&data, 0).unwrap();
    assert_eq!(items, vec![&b"ab"[..], &b"c"[..]]);
    assert_eq!(end, 9);
    let (empty, end) = parse_index(&[0, 0], 0).unwrap();
    assert!(empty.is_empty());
    assert_eq!(end, 2);
    assert!(parse_index(&[0, 2, 1, 1, 9, 4], 0).is_err());
}

#[test]
fn dict_numbers_and_operators() {
    // 0.001 as a real (1e-3: nibbles 1 c 3 f) and -2.25 (e 2 a 2 5 f)
    let data = [30, 0x1c, 0x3f, 30, 0xe2, 0xa2, 0x5f, 12, 7, 139, 17];
    let dict = parse_dict(&data).unwrap();
    assert_eq!(dict.get(&1207).unwrap(), &vec![n("0.001"), n("-2.25")]);
    assert_eq!(dict.get(&17).unwrap(), &vec![n("0")]);
    // integers of every size
    let data = [28, 0x03, 0xe8, 29, 0, 1, 0, 0, 247, 0, 251, 0, 15];
    assert_eq!(
        parse_dict(&data).unwrap().get(&15).unwrap(),
        &vec![n("1000"), n("65536"), n("108"), n("-108")]
    );
    // 1.5E2
    let data = [30, 0x1a, 0x5b, 0x2f, 0];
    assert_eq!(parse_dict(&data).unwrap().get(&0).unwrap(), &vec![n("150")]);
}

#[test]
fn charsets_of_every_format() {
    // format 0: the SIDs or CIDs of glyphs 1..n
    let f0 = [0, 0, 10, 0, 20, 0, 30];
    assert_eq!(parse_charset(&f0, 0, 4).unwrap(), vec![0, 10, 20, 30]);
    // format 1: ranges with a one-byte count
    let f1 = [1, 0, 100, 2, 1, 0, 0];
    assert_eq!(
        parse_charset(&f1, 0, 5).unwrap(),
        vec![0, 100, 101, 102, 256]
    );
    // format 2: ranges with a two-byte count
    let f2 = [2, 0, 5, 0, 3];
    assert_eq!(parse_charset(&f2, 0, 5).unwrap(), vec![0, 5, 6, 7, 8]);
    assert!(parse_charset(&[3], 0, 2).is_err());
}

#[test]
fn fd_selects_of_formats_0_and_3() {
    assert_eq!(
        parse_fd_select(&[0, 0, 1, 1, 2], 0, 4).unwrap(),
        vec![0, 1, 1, 2]
    );
    // format 3: two ranges [0, 2) -> 5 and [2, 4) -> 7, then the sentinel 4
    let f3 = [3, 0, 2, 0, 0, 5, 0, 2, 7, 0, 4];
    assert_eq!(parse_fd_select(&f3, 0, 4).unwrap(), vec![5, 5, 7, 7]);
    assert!(parse_fd_select(&[1, 0], 0, 1).is_err());
}
