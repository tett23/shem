use super::*;
use crate::fixed::Fixed;
use crate::render::tests::{page_pdf, render_pdf};

fn n(s: &str) -> Fixed {
    Fixed::parse(s.as_bytes()).unwrap()
}

#[test]
fn components_round_to_even_and_clamp() {
    assert_eq!(component(n("0")), 0);
    assert_eq!(component(n("1")), 255);
    // 0.5 × 255 = 127.5 rounds to 128
    assert_eq!(component(n("0.5")), 128);
    assert_eq!(component(n("-1")), 0);
    assert_eq!(component(n("2")), 255);
}

#[test]
fn device_gray() {
    assert_eq!(gray(n("0")), [0, 0, 0]);
    assert_eq!(gray(n("1")), [255, 255, 255]);
    assert_eq!(gray(n("0.5")), [128, 128, 128]);
}

#[test]
fn device_rgb() {
    // docs/adr/0027: each component as DeviceGray rounds it.
    let c = |v: [&str; 3]| rgb(v.map(n));
    assert_eq!(c(["0", "0", "0"]), [0, 0, 0]);
    assert_eq!(c(["1", "1", "1"]), [255, 255, 255]);
    assert_eq!(c(["1", "0.5", "0"]), [255, 128, 0]);
    assert_eq!(c(["-1", "2", "0.25"]), [0, 255, 64]);
}

#[test]
fn device_cmyk() {
    let c = |v: [&str; 4]| cmyk(v.map(n));
    assert_eq!(c(["0", "0", "0", "0"]), [255, 255, 255]);
    assert_eq!(c(["0", "0", "0", "1"]), [0, 0, 0]);
    assert_eq!(c(["1", "0", "0", "0"]), [0, 255, 255]);
    assert_eq!(c(["1", "1", "1", "1"]), [0, 0, 0]);
    assert_eq!(c(["0.5", "0", "0", "0.5"]), [0, 127, 127]);
}

const REG: &str = "/ColorSpace << /Reg [/Separation /All /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 1 1 1] /N 1 >>] >>";

#[test]
fn the_separation_of_golem_marks() {
    let black = render_pdf(
        &page_pdf("0 0 4 4", REG, "/Reg cs 1 scn 0 0 4 4 re f", &[]),
        72,
    )
    .unwrap();
    assert_eq!(black.pixel(1, 1), [0, 0, 0]);
    let white = render_pdf(
        &page_pdf("0 0 4 4", REG, "/Reg cs 0 scn 0 0 4 4 re f", &[]),
        72,
    )
    .unwrap();
    assert_eq!(white.pixel(1, 1), [255, 255, 255]);
    // the initial tint after cs is 1
    let initial = render_pdf(&page_pdf("0 0 4 4", REG, "/Reg cs 0 0 4 4 re f", &[]), 72).unwrap();
    assert_eq!(initial.pixel(1, 1), [0, 0, 0]);
}

#[test]
fn a_separation_with_the_defaults_of_type_2() {
    // C0 and C1 default to [0] and [1]; the tint is clamped to the Domain
    let gray_sep = "/ColorSpace << /S [/Separation /Spot /DeviceGray << /FunctionType 2 /Domain [0 0.5] /N 1 >>] >>";
    let canvas = render_pdf(
        &page_pdf("0 0 4 4", gray_sep, "/S cs 1 scn 0 0 4 4 re f", &[]),
        72,
    )
    .unwrap();
    assert_eq!(canvas.pixel(1, 1), [128, 128, 128]);
}

#[test]
fn the_color_before_any_is_black_and_q_restores_it() {
    let canvas = render_pdf(
        &page_pdf(
            "0 0 4 2",
            "",
            "0 0 2 2 re f q 0 0 1 0 k Q 2 0 2 2 re f",
            &[],
        ),
        72,
    )
    .unwrap();
    assert_eq!(canvas.pixel(0, 0), [0, 0, 0]);
    assert_eq!(canvas.pixel(3, 0), [0, 0, 0]);
    let yellow = render_pdf(&page_pdf("0 0 2 2", "", "0 0 1 0 k 0 0 2 2 re f", &[]), 72).unwrap();
    assert_eq!(yellow.pixel(0, 0), [255, 255, 0]);
    // K does not change the fill
    let stroke_only =
        render_pdf(&page_pdf("0 0 2 2", "", "0 0 1 0 K 0 0 2 2 re f", &[]), 72).unwrap();
    assert_eq!(stroke_only.pixel(0, 0), [0, 0, 0]);
}

fn error_of(resources: &str, content: &str) -> String {
    render_pdf(&page_pdf("0 0 4 4", resources, content, &[]), 72)
        .unwrap_err()
        .to_string()
}

#[test]
fn what_v0_1_does_not_read() {
    assert!(error_of("", "/DeviceRGB cs").contains("no color space DeviceRGB"));
    assert!(error_of("", "/DeviceCMYK cs").contains("no color space DeviceCMYK"));
    assert!(error_of("", "1 scn").contains("unsupported scn"));
    assert!(
        error_of(
            "/ColorSpace << /C [/Indexed /DeviceGray 1 <0001>] >>",
            "/C cs"
        )
        .contains("unsupported ColorSpace Indexed")
    );
    assert!(
        error_of(
            "/ColorSpace << /S [/Separation /X /DeviceRGB << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [1 1 1] /N 1 >>] >>",
            "/S cs"
        )
        .contains("unsupported alternate color space DeviceRGB")
    );
    assert!(
        error_of(
            "/ColorSpace << /S [/Separation /X /DeviceGray << /FunctionType 0 /Domain [0 1] >>] >>",
            "/S cs"
        )
        .contains("unsupported FunctionType 0")
    );
    assert!(
        error_of(
            "/ColorSpace << /S [/Separation /X /DeviceGray << /FunctionType 2 /Domain [0 1] /N 2 >>] >>",
            "/S cs"
        )
        .contains("unsupported N 2")
    );
    assert!(
        error_of(
            "/ColorSpace << /S [/Separation /X /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >>] >>",
            "/S cs"
        )
        .contains("C0")
    );
}

// --- ICCBased by the number of components, sc and SC (docs/adr/0027)

/// A page with an ICCBased color space of `n` components as /C, whose
/// stream is object 5.
fn icc_pdf(n: &str, content: &str) -> Vec<u8> {
    page_pdf(
        "0 0 4 4",
        "/ColorSpace << /C [/ICCBased 5 0 R] >>",
        content,
        &[&format!("<< /N {n} /Length 0 >>\nstream\n\nendstream")],
    )
}

fn icc_pixel(n: &str, content: &str) -> [u8; 3] {
    render_pdf(&icc_pdf(n, content), 72).unwrap().pixel(1, 1)
}

fn icc_error(n: &str, content: &str) -> String {
    render_pdf(&icc_pdf(n, content), 72)
        .unwrap_err()
        .message()
        .to_string()
}

#[test]
fn icc_based_colors_are_those_of_the_device_space_of_their_components() {
    assert_eq!(icc_pixel("1", "/C cs 0.5 sc 0 0 4 4 re f"), gray(n("0.5")));
    assert_eq!(
        icc_pixel("3", "/C cs 1 0.5 0 sc 0 0 4 4 re f"),
        rgb(["1", "0.5", "0"].map(n))
    );
    assert_eq!(
        icc_pixel("4", "/C cs 0.5 0 0 0.5 sc 0 0 4 4 re f"),
        cmyk(["0.5", "0", "0", "0.5"].map(n))
    );
    // The same for the stroke: a wide line across the page.
    assert_eq!(
        icc_pixel("3", "/C CS 1 0.5 0 SC 8 w 0 2 m 4 2 l S"),
        rgb(["1", "0.5", "0"].map(n))
    );
    // The color after cs and CS is black.
    assert_eq!(icc_pixel("3", "/C cs 0 0 4 4 re f"), [0, 0, 0]);
    assert_eq!(icc_pixel("4", "/C CS 8 w 0 2 m 4 2 l S"), [0, 0, 0]);
}

#[test]
fn sc_and_sc_take_the_components_of_the_space() {
    assert_eq!(
        icc_error("3", "/C cs 1 sc"),
        "sc with 1 components for a color space of 3"
    );
    assert_eq!(
        icc_error("1", "/C CS 1 0 0 SC"),
        "SC with 3 components for a color space of 1"
    );
    // The initial space is DeviceGray; k and K select DeviceCMYK.
    let pixel = |content: &str| {
        render_pdf(&page_pdf("0 0 4 4", "", content, &[]), 72)
            .unwrap()
            .pixel(1, 1)
    };
    assert_eq!(pixel("0.5 sc 0 0 4 4 re f"), [128, 128, 128]);
    assert_eq!(pixel("0.5 SC 8 w 0 2 m 4 2 l S"), [128, 128, 128]);
    assert_eq!(pixel("0 0 0 1 k 0 0 1 0 sc 0 0 4 4 re f"), [255, 255, 0]);
    assert_eq!(
        pixel("0 0 0 1 K 0 0 1 0 SC 8 w 0 2 m 4 2 l S"),
        [255, 255, 0]
    );
    assert_eq!(
        error_of("", "0 0 0 1 k 1 sc"),
        "page 1: offset 12: sc with 1 components for a color space of 4"
    );
    // A Separation takes scn, not sc.
    assert!(error_of(REG, "/Reg cs 1 sc").contains("unsupported sc with a Separation"));
    // An ICCBased space takes sc, not scn.
    assert!(icc_error("3", "/C cs 1 scn").contains("unsupported scn"));
}

#[test]
fn icc_based_with_other_components_is_an_error() {
    assert_eq!(icc_error("2", "/C cs"), "unsupported ICCBased with N 2");
    assert_eq!(icc_error("5", "/C cs"), "unsupported ICCBased with N 5");
    assert!(error_of("/ColorSpace << /C [/ICCBased] >>", "/C cs").contains("malformed ICCBased"));
    assert!(
        error_of("/ColorSpace << /C [/ICCBased /DeviceRGB] >>", "/C cs")
            .contains("malformed ICCBased")
    );
}

#[test]
fn q_and_q_restore_the_stroke_color_space() {
    let pixel = |content: &str| render_pdf(&icc_pdf("3", content), 72).unwrap().pixel(1, 1);
    assert_eq!(pixel("q /C CS 1 0 0 SC Q 8 w 0 2 m 4 2 l S"), [0, 0, 0]);
    assert_eq!(pixel("q /C CS Q 0.5 SC 8 w 0 2 m 4 2 l S"), [128, 128, 128]);
}
