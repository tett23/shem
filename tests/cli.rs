//! The command line (docs/adr/0007).

use std::path::{Path, PathBuf};
use std::process::Command;

fn shem(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_shem"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// A new empty directory for a test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shem-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/golem/{name}.pdf",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|r| {
            r.map(|e| e.unwrap().file_name().into_string().unwrap())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// A PDF of pages with the given contents.
fn pdf(contents: &[&str], media_box: &str) -> Vec<u8> {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + 2 * i)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
    ];
    contents.iter().enumerate().for_each(|(i, c)| {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}] /Resources << >> /Contents {} 0 R >>",
            4 + 2 * i
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
    });
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

fn write_pdf(dir: &Path, name: &str, data: &[u8]) -> String {
    let path = dir.join(name);
    std::fs::write(&path, data).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn renders_every_page() {
    let dir = scratch("every");
    let out = dir.join("out");
    let (code, stdout, stderr) = shem(&["render", &fixture("sample"), out.to_str().unwrap()]);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    assert_eq!(
        files(&out),
        vec!["page-0001.png", "page-0002.png", "page-0003.png"]
    );
    let png = std::fs::read(out.join("page-0001.png")).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn page_numbers_have_as_many_digits_as_the_page_count() {
    let dir = scratch("digits");
    let input = write_pdf(&dir, "many.pdf", &pdf(&vec![""; 12345], "0 0 1 1"));
    let out = dir.join("out");
    let (code, _, stderr) = shem(&["render", "--pages", "1-2", &input, out.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(files(&out), vec!["page-00001.png", "page-00002.png"]);
}

#[test]
fn pages_and_their_errors() {
    let dir = scratch("pages");
    let out = dir.join("out");
    let o = out.to_str().unwrap();
    let input = fixture("sample");
    assert_eq!(shem(&["render", "--pages", "2-3", &input, o]).0, 0);
    assert_eq!(files(&out), vec!["page-0002.png", "page-0003.png"]);
    let out2 = dir.join("out2");
    assert_eq!(
        shem(&["render", "--pages", "1,3", &input, out2.to_str().unwrap()]).0,
        0
    );
    assert_eq!(files(&out2), vec!["page-0001.png", "page-0003.png"]);
    for bad in ["0", "3-2", "x", "1-", "-2", ""] {
        assert_eq!(shem(&["render", "--pages", bad, &input, o]).0, 64, "{bad}");
    }
    let (code, _, stderr) = shem(&["render", "--pages", "99", &input, o]);
    assert_eq!(code, 1);
    assert!(stderr.contains("page 99"), "{stderr}");
}

#[test]
fn usage_errors() {
    let input = fixture("sample");
    let dir = scratch("usage");
    let o = dir.join("out");
    let o = o.to_str().unwrap();
    for args in [
        vec!["render", "--dpi", "0", &input, o],
        vec!["render", "--dpi", "1201", &input, o],
        vec!["render", "--dpi", "1.5", &input, o],
        vec!["render", "--dpi", "72", "--dpi", "72", &input, o],
        vec!["render", "--format", "jpg", &input, o],
        vec!["render", "--unknown", &input, o],
        vec!["render", &input],
        vec!["render", &input, o, "extra"],
        vec!["render", "--dpi"],
        vec!["paint", &input, o],
        vec![],
    ] {
        let (code, _, stderr) = shem(&args);
        assert_eq!(code, 64, "{args:?}");
        assert!(stderr.contains("usage"), "{args:?}: {stderr}");
    }
    assert!(files(&dir).is_empty());
}

#[test]
fn options_may_come_anywhere_after_the_subcommand() {
    let dir = scratch("anywhere");
    let out = dir.join("out");
    let (code, _, stderr) = shem(&[
        "render",
        &fixture("sample"),
        "--dpi",
        "36",
        out.to_str().unwrap(),
        "--format",
        "ppm",
    ]);
    assert_eq!(code, 0, "{stderr}");
    let ppm = std::fs::read(out.join("page-0001.ppm")).unwrap();
    assert!(ppm.starts_with(b"P6\n"));
    // The file written as it is rendered is the bytes of encode_ppm (docs/adr/0013).
    let data = std::fs::read(fixture("sample")).unwrap();
    let doc = shem::pdf::Document::parse(&data).unwrap();
    let canvas = shem::render::render_page(&doc, &doc.pages().unwrap()[0], 36).unwrap();
    assert_eq!(ppm, shem::png::encode_ppm(&canvas));
}

#[test]
fn the_output_directory() {
    let dir = scratch("output");
    let out = dir.join("a").join("b");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("page-0001.png"), b"old").unwrap();
    std::fs::write(out.join("other.txt"), b"keep").unwrap();
    assert_eq!(
        shem(&[
            "render",
            "--pages",
            "1",
            &fixture("sample"),
            out.to_str().unwrap()
        ])
        .0,
        0
    );
    assert_ne!(std::fs::read(out.join("page-0001.png")).unwrap(), b"old");
    assert_eq!(std::fs::read(out.join("other.txt")).unwrap(), b"keep");
    // a directory that does not exist is made
    let fresh = dir.join("x").join("y");
    assert_eq!(
        shem(&[
            "render",
            "--pages",
            "1",
            &fixture("sample"),
            fresh.to_str().unwrap()
        ])
        .0,
        0
    );
    assert_eq!(files(&fresh), vec!["page-0001.png"]);
}

#[test]
fn nothing_is_written_when_a_later_page_is_out_of_the_subset() {
    let dir = scratch("later");
    let input = write_pdf(
        &dir,
        "bad.pdf",
        &pdf(&["0 0 1 1 re f", "", "q (x) TJ Q"], "0 0 10 10"),
    );
    let out = dir.join("out");
    let (code, stdout, stderr) = shem(&["render", &input, out.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("shem: {input}: page 3: offset 6: unsupported operator TJ\n")
    );
    assert!(files(&out).is_empty());
}

#[test]
fn errors_of_the_input() {
    let dir = scratch("input");
    let o = dir.join("out");
    let o = o.to_str().unwrap();
    let (code, _, stderr) = shem(&["render", "/nonexistent/x.pdf", o]);
    assert_eq!(code, 1);
    assert!(stderr.starts_with("shem: /nonexistent/x.pdf: "), "{stderr}");
    let huge = write_pdf(&dir, "huge.pdf", &pdf(&[""], "0 0 100000 100000"));
    let (code, _, stderr) = shem(&["render", &huge, o]);
    assert_eq!(code, 1);
    assert!(stderr.contains("too large"), "{stderr}");
}

#[test]
fn version_and_help() {
    let (code, stdout, _) = shem(&["--version"]);
    assert_eq!(
        (code, stdout.as_str()),
        (0, concat!("shem ", env!("CARGO_PKG_VERSION"), "\n"))
    );
    assert_eq!(shem(&["-V"]).1, stdout);
    let (code, help, _) = shem(&["--help"]);
    assert_eq!(code, 0);
    for needle in [
        "render",
        "--dpi",
        "--format",
        "--pages",
        "Exit status",
        "64",
        "unsupported",
    ] {
        assert!(help.contains(needle), "{needle}");
    }
    assert_eq!(shem(&["-h"]).1, help);
}

/// The bytes of every file of a directory, by name.
fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    files(dir)
        .into_iter()
        .map(|n| (n.clone(), std::fs::read(dir.join(&n)).unwrap()))
        .collect()
}

#[test]
fn the_output_does_not_depend_on_jobs() {
    let dir = scratch("jobs-same");
    ["sample", "sample-marks", "figures", "truetype"]
        .iter()
        .for_each(|name| {
            ["png", "ppm"].iter().for_each(|format| {
                let outputs: Vec<_> = ["1", "2", "16"]
                    .iter()
                    .map(|jobs| {
                        let out = dir.join(format!("{name}-{format}-{jobs}"));
                        let (code, _, stderr) = shem(&[
                            "render",
                            "--dpi",
                            "50",
                            "--format",
                            format,
                            "--jobs",
                            jobs,
                            &fixture(name),
                            out.to_str().unwrap(),
                        ]);
                        assert_eq!(code, 0, "{stderr}");
                        contents(&out)
                    })
                    .collect();
                assert!(!outputs[0].is_empty());
                assert_eq!(outputs[0], outputs[1], "{name} {format}");
                assert_eq!(outputs[0], outputs[2], "{name} {format}");
            });
        });
}

#[test]
fn many_pages_with_many_jobs() {
    let dir = scratch("jobs-many");
    let input = write_pdf(
        &dir,
        "many.pdf",
        &pdf(&vec!["0 0 1 1 re f"; 12345], "0 0 2 2"),
    );
    let one = dir.join("one");
    let eight = dir.join("eight");
    assert_eq!(
        shem(&[
            "render",
            "--jobs",
            "1",
            "--pages",
            "1-64",
            &input,
            one.to_str().unwrap()
        ])
        .0,
        0
    );
    assert_eq!(
        shem(&[
            "render",
            "--jobs",
            "8",
            "--pages",
            "1-64",
            &input,
            eight.to_str().unwrap()
        ])
        .0,
        0
    );
    assert_eq!(files(&eight).len(), 64);
    assert_eq!(contents(&one), contents(&eight));
}

#[test]
fn the_error_of_the_first_bad_page_is_reported_whatever_the_jobs() {
    let dir = scratch("jobs-error");
    let input = write_pdf(
        &dir,
        "bad.pdf",
        &pdf(&["", "q (x) TJ Q", "", "q (y) Td Q", ""], "0 0 10 10"),
    );
    let out = dir.join("out");
    for jobs in ["1", "8"] {
        for _ in 0..20 {
            let (code, stdout, stderr) =
                shem(&["render", "--jobs", jobs, &input, out.to_str().unwrap()]);
            assert_eq!(code, 1);
            assert_eq!(stdout, "");
            assert_eq!(
                stderr,
                format!("shem: {input}: page 2: offset 6: unsupported operator TJ\n")
            );
            assert!(files(&out).is_empty());
        }
    }
}

#[test]
fn wrong_jobs_are_usage_errors() {
    let dir = scratch("jobs-usage");
    let o = dir.join("out");
    let o = o.to_str().unwrap();
    let input = fixture("sample");
    for args in [
        vec!["render", "--jobs", "0", &input, o],
        vec!["render", "--jobs", "1025", &input, o],
        vec!["render", "--jobs", "x", &input, o],
        vec!["render", "--jobs", "2", "--jobs", "2", &input, o],
    ] {
        assert_eq!(shem(&args).0, 64, "{args:?}");
    }
    assert!(shem(&["--help"]).1.contains("--jobs"));
}
