use super::*;
use crate::font::Segment;

fn f(v: i32) -> Fixed {
    Fixed::from_int(v.into()).unwrap()
}

fn half(v: i32) -> Fixed {
    // v / 2
    Fixed::from_raw(i64::from(v) << 31)
}

fn be16(v: i32) -> [u8; 2] {
    (v as i16).to_be_bytes()
}

/// A simple glyph from contours of (x, y, on-curve) points, with long coordinates.
pub(crate) fn simple(contours: &[&[(i32, i32, bool)]]) -> Vec<u8> {
    let mut out = be16(contours.len() as i32).to_vec();
    out.extend([0u8; 8]); // the bounding box is not read
    let mut end = -1;
    contours.iter().for_each(|c| {
        end += c.len() as i32;
        out.extend(be16(end));
    });
    out.extend(be16(0)); // no instructions
    let points: Vec<&(i32, i32, bool)> = contours.iter().flat_map(|c| c.iter()).collect();
    points.iter().for_each(|p| out.push(u8::from(p.2)));
    let (mut x, mut y) = (0, 0);
    points.iter().for_each(|p| {
        out.extend(be16(p.0 - x));
        x = p.0;
    });
    points.iter().for_each(|p| {
        out.extend(be16(p.1 - y));
        y = p.1;
    });
    out
}

/// A font of `head`, `maxp`, `loca` and `glyf`.
pub(crate) fn font(glyphs: &[Vec<u8>], long_loca: bool, units_per_em: u16) -> Vec<u8> {
    let mut glyf = Vec::new();
    let mut offsets = vec![0usize];
    glyphs.iter().for_each(|g| {
        glyf.extend(g);
        if glyf.len() % 2 == 1 {
            glyf.push(0);
        }
        offsets.push(glyf.len());
    });
    let loca: Vec<u8> = offsets
        .iter()
        .flat_map(|&o| {
            if long_loca {
                (o as u32).to_be_bytes().to_vec()
            } else {
                ((o / 2) as u16).to_be_bytes().to_vec()
            }
        })
        .collect();
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&units_per_em.to_be_bytes());
    head[50..52].copy_from_slice(&(i16::from(long_loca)).to_be_bytes());
    let mut maxp = vec![0, 0, 0x50, 0];
    maxp.extend((glyphs.len() as u16).to_be_bytes());
    let tables: [(&[u8; 4], Vec<u8>); 4] = [
        (b"glyf", glyf),
        (b"head", head),
        (b"loca", loca),
        (b"maxp", maxp),
    ];
    let mut out = vec![0, 1, 0, 0];
    out.extend(4u16.to_be_bytes());
    out.extend([0u8; 6]);
    let mut at = 12 + 16 * tables.len();
    let mut body = Vec::new();
    tables.iter().for_each(|(tag, data)| {
        out.extend(*tag);
        out.extend([0u8; 4]);
        out.extend((at as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        at = 12 + 16 * tables.len() + body.len();
    });
    out.extend(body);
    out
}

fn outline_of(glyph: Vec<u8>) -> Vec<Segment> {
    TrueType::parse(&font(&[vec![], glyph], false, 1000))
        .unwrap()
        .outline(1)
        .unwrap()
}

#[test]
fn a_square_of_points_on_the_curve() {
    let got = outline_of(simple(&[&[
        (0, 0, true),
        (100, 0, true),
        (100, 100, true),
        (0, 100, true),
    ]]));
    assert_eq!(
        got,
        vec![
            Segment::Move(f(0), f(0)),
            Segment::Line(f(100), f(0)),
            Segment::Line(f(100), f(100)),
            Segment::Line(f(0), f(100)),
            Segment::Close
        ]
    );
}

/// The cubic of the quadratic p0, q, p2: control points p0 + 2(q − p0)/3 and p2 + 2(q − p2)/3.
fn cubic(p0: (i32, i32), q: (i32, i32), p2: (i32, i32)) -> Segment {
    let third = |a: i32, b: i32| {
        Fixed::from_raw(
            i64::from(a) * (1 << 32)
                + crate::fixed::div_round((2 * i128::from(b - a)) << 32, 3) as i64,
        )
    };
    Segment::Curve([
        third(p0.0, q.0),
        third(p0.1, q.1),
        third(p2.0, q.0),
        third(p2.1, q.1),
        f(p2.0),
        f(p2.1),
    ])
}

#[test]
fn points_off_the_curve() {
    // on (0,0), off (100,0), on (100,100): one quadratic, then a line back
    let got = outline_of(simple(&[&[
        (0, 0, true),
        (100, 0, false),
        (100, 100, true),
    ]]));
    assert_eq!(
        got,
        vec![
            Segment::Move(f(0), f(0)),
            cubic((0, 0), (100, 0), (100, 100)),
            Segment::Close
        ]
    );
    // two points off the curve in a row have an implied point between them
    let got = outline_of(simple(&[&[
        (0, 0, true),
        (100, 0, false),
        (100, 100, false),
        (0, 100, true),
    ]]));
    assert_eq!(
        got,
        vec![
            Segment::Move(f(0), f(0)),
            cubic((0, 0), (100, 0), (100, 50)),
            cubic((100, 50), (100, 100), (0, 100)),
            Segment::Close
        ]
    );
    // a contour that starts off the curve begins at its first point on the curve
    let got = outline_of(simple(&[&[
        (100, 0, false),
        (100, 100, true),
        (0, 100, true),
        (0, 0, true),
    ]]));
    assert_eq!(got[0], Segment::Move(f(100), f(100)));
    assert_eq!(*got.last().unwrap(), Segment::Close);
    assert_eq!(got[got.len() - 2], cubic((0, 0), (100, 0), (100, 100)));
}

#[test]
fn a_contour_with_no_point_on_the_curve() {
    // four points off the curve: a circle-like shape that starts at the midpoint of the first two
    let got = outline_of(simple(&[&[
        (0, 0, false),
        (100, 0, false),
        (100, 100, false),
        (0, 100, false),
    ]]));
    assert_eq!(got[0], Segment::Move(half(100), f(0)));
    assert_eq!(got.len(), 6);
    let Segment::Curve(last) = got[4] else {
        panic!()
    };
    assert_eq!((last[4], last[5]), (half(100), f(0)));
}

#[test]
fn flags_repeat_and_short_coordinates() {
    // a triangle (0,0) (10,0) (10,10), all on the curve, with REPEAT and short and same coordinates
    let glyph = [
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0, // one contour, no box
        0,
        2, // end point 2
        0,
        0, // no instructions
        // flags: on curve; x short positive; y same: 0x01 | 0x02 | 0x10 | 0x20 = 0x33, repeated twice
        0x01 | 0x10 | 0x20, // (0,0): x same (0), y same (0)
        0x01 | 0x02 | 0x10 | 0x20 | 0x08,
        1, // (10,0) repeated once: x short positive, y same
        // x: 10 for point 1, 0 (short) for point 2
        10,
        0,
        // y: point 2 short? no: point 2 has the same flags as point 1 (y same): so y stays 0
    ];
    let got = outline_of(glyph.to_vec());
    assert_eq!(
        got,
        vec![
            Segment::Move(f(0), f(0)),
            Segment::Line(f(10), f(0)),
            Segment::Line(f(10), f(0)),
            Segment::Close
        ]
    );
    // short negative coordinates
    let glyph = [
        0,
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        0,
        0,
        0x01 | 0x02 | 0x04 | 0x10,
        0x01 | 0x02 | 0x04, // (5, -3) then (5 - 2, -3 - 4)
        5,
        2, // x: +5, -2
        3,
        4, // y: -3, -4
    ];
    let got = outline_of(glyph.to_vec());
    assert_eq!(
        got,
        vec![
            Segment::Move(f(5), f(-3)),
            Segment::Line(f(3), f(-7)),
            Segment::Close
        ]
    );
}

fn component(flags: u16, glyph: u16, args: &[u8], transform: &[u8]) -> Vec<u8> {
    let mut out = flags.to_be_bytes().to_vec();
    out.extend(glyph.to_be_bytes());
    out.extend(args);
    out.extend(transform);
    out
}

fn composite(components: &[Vec<u8>]) -> Vec<u8> {
    let mut out = be16(-1).to_vec();
    out.extend([0u8; 8]);
    components.iter().for_each(|c| out.extend(c));
    out
}

const XY: u16 = 0x0002;
const WORDS: u16 = 0x0001;
const MORE: u16 = 0x0020;
const SCALE: u16 = 0x0008;
const XY_SCALE: u16 = 0x0040;
const TWO_BY_TWO: u16 = 0x0080;

fn unit_square() -> Vec<u8> {
    simple(&[&[(0, 0, true), (10, 0, true), (10, 10, true), (0, 10, true)]])
}

fn composite_outline(glyph: Vec<u8>) -> Vec<Segment> {
    TrueType::parse(&font(&[vec![], unit_square(), glyph], false, 1000))
        .unwrap()
        .outline(2)
        .unwrap()
}

#[test]
fn composite_glyphs() {
    // translated by (5, -3) with byte arguments, then by (300, 0) with words
    let got = composite_outline(composite(&[
        component(XY | MORE, 1, &[5, (-3i8) as u8], &[]),
        component(XY | WORDS, 1, &[1, 44, 0, 0], &[]),
    ]));
    assert_eq!(got[0], Segment::Move(f(5), f(-3)));
    assert_eq!(got[5], Segment::Move(f(300), f(0)));
    // a scale of 2 (F2Dot14 0x8000 is -2; 0x4000 is 1; 2.0 is not representable, so use 0.5 = 0x2000)
    let got = composite_outline(composite(&[component(XY | SCALE, 1, &[0, 0], &[0x20, 0])]));
    assert_eq!(got[2], Segment::Line(f(5), f(5)));
    // x and y scales
    let got = composite_outline(composite(&[component(
        XY | XY_SCALE,
        1,
        &[0, 0],
        &[0x20, 0, 0x40, 0],
    )]));
    assert_eq!(got[2], Segment::Line(f(5), f(10)));
    // a two by two: a rotation by 90 degrees (0 1 −1 0)
    let got = composite_outline(composite(&[component(
        XY | TWO_BY_TWO,
        1,
        &[0, 0],
        &[0, 0, 0x40, 0, 0xC0, 0, 0, 0],
    )]));
    assert_eq!(got[1], Segment::Line(f(0), f(10)));
    // nested composites
    let nested = TrueType::parse(&font(
        &[
            vec![],
            unit_square(),
            composite(&[component(XY, 1, &[1, 1], &[])]),
            composite(&[component(XY, 2, &[1, 1], &[])]),
        ],
        true,
        1000,
    ))
    .unwrap();
    assert_eq!(nested.outline(3).unwrap()[0], Segment::Move(f(2), f(2)));
}

#[test]
fn unsupported_composites() {
    let point_matching = TrueType::parse(&font(
        &[
            vec![],
            unit_square(),
            composite(&[component(0, 1, &[0, 0], &[])]),
        ],
        false,
        1000,
    ))
    .unwrap();
    assert!(
        point_matching
            .outline(2)
            .unwrap_err()
            .to_string()
            .contains("point")
    );
    let scaled_offset = TrueType::parse(&font(
        &[
            vec![],
            unit_square(),
            composite(&[component(XY | 0x0800, 1, &[0, 0], &[])]),
        ],
        false,
        1000,
    ))
    .unwrap();
    assert!(
        scaled_offset
            .outline(2)
            .unwrap_err()
            .to_string()
            .contains("SCALED_COMPONENT_OFFSET")
    );
    // a composite that refers to itself
    let cycle = TrueType::parse(&font(
        &[vec![], composite(&[component(XY, 1, &[0, 0], &[])])],
        false,
        1000,
    ))
    .unwrap();
    assert!(
        cycle
            .outline(1)
            .unwrap_err()
            .to_string()
            .contains("too deep")
    );
}

#[test]
fn loca_formats_and_empty_glyphs() {
    let short = TrueType::parse(&font(&[vec![], unit_square()], false, 2048)).unwrap();
    let long = TrueType::parse(&font(&[vec![], unit_square()], true, 2048)).unwrap();
    assert_eq!(short.outline(1).unwrap(), long.outline(1).unwrap());
    assert_eq!(short.outline(0).unwrap(), vec![]);
    assert_eq!(short.units_per_em(), 2048);
    assert_eq!(short.glyph_count(), 2);
    assert!(short.outline(2).is_err());
}

#[test]
fn rejects_what_is_not_a_truetype_font() {
    assert!(TrueType::parse(b"OTTO\0\0\0\0\0\0\0\0").is_err());
    assert!(TrueType::parse(b"ttcf\0\0\0\0\0\0\0\0").is_err());
    assert!(TrueType::parse(&[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
}
