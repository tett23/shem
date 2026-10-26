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
        error_of("/ColorSpace << /C [/ICCBased 5 0 R] >>", "/C cs")
            .contains("unsupported ColorSpace ICCBased")
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
