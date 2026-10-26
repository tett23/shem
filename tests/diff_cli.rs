//! shem diff (docs/adr/0010).

use std::path::{Path, PathBuf};
use std::process::Command;

use shem::image::decode;
use shem::png::{encode_png, encode_ppm};
use shem::raster::Canvas;

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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shem-diff-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

fn white(width: usize, height: usize) -> Canvas {
    Canvas {
        width,
        height,
        pixels: vec![255; width * height * 3],
    }
}

/// A white image with one black pixel at (x, y).
fn dot(x: usize, y: usize) -> Canvas {
    let mut c = white(20, 20);
    let i = (y * 20 + x) * 3;
    c.pixels[i..i + 3].copy_from_slice(&[0, 0, 0]);
    c
}

fn write(path: &Path, c: &Canvas) {
    let bytes = if path.extension().is_some_and(|e| e == "ppm") {
        encode_ppm(c)
    } else {
        encode_png(c)
    };
    std::fs::write(path, bytes).unwrap();
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn two_same_files() {
    let dir = scratch("same-files");
    let (a, b, out) = (dir.join("a.png"), dir.join("b.ppm"), dir.join("out.png"));
    write(&a, &white(20, 20));
    write(&b, &white(20, 20));
    assert_eq!(
        shem(&["diff", s(&a), s(&b), s(&out)]),
        (0, String::new(), String::new())
    );
    assert!(!out.exists());
    std::fs::write(&out, b"old").unwrap();
    assert_eq!(shem(&["diff", s(&a), s(&b), s(&out)]).0, 0);
    assert_eq!(std::fs::read(&out).unwrap(), b"old");
}

#[test]
fn two_different_files() {
    let dir = scratch("different-files");
    let (a, b, out) = (dir.join("a.png"), dir.join("b.png"), dir.join("out.png"));
    write(&a, &white(20, 20));
    write(&b, &dot(10, 10));
    let (code, stdout, stderr) = shem(&["diff", s(&a), s(&b), s(&out)]);
    assert_eq!(
        (code, stdout.as_str(), stderr.as_str()),
        (1, "differ: 1 pixels in 10 10 11 11\n", "")
    );
    let image = decode(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(image.pixel(10, 10), shem::diff::ADDED);
    let ppm = dir.join("out.ppm");
    assert_eq!(
        shem(&["diff", "--format", "ppm", s(&a), s(&b), s(&ppm)]).0,
        1
    );
    assert!(std::fs::read(&ppm).unwrap().starts_with(b"P6\n"));
    assert_eq!(files(&dir), vec!["a.png", "b.png", "out.png", "out.ppm"]);
}

#[test]
fn errors_of_files() {
    let dir = scratch("file-errors");
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    write(&a, &white(20, 20));
    write(&b, &dot(1, 1));
    let missing_dir = dir.join("no").join("out.png");
    let (code, _, stderr) = shem(&["diff", s(&a), s(&b), s(&missing_dir)]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.starts_with("shem: "), "{stderr}");
    let gif = dir.join("x.gif");
    std::fs::write(&gif, b"GIF89a").unwrap();
    let (code, _, stderr) = shem(&["diff", s(&gif), s(&b), s(&dir.join("o.png"))]);
    assert_eq!(code, 2);
    assert_eq!(
        stderr,
        format!("shem: {}: the image is neither PNG nor PPM\n", s(&gif))
    );
    assert_eq!(files(&dir), vec!["a.png", "b.png", "x.gif"]);
}

#[test]
fn two_directories() {
    let dir = scratch("dirs");
    let (before, after, out) = (
        dir.join("before"),
        dir.join("after"),
        dir.join("out").join("diff"),
    );
    std::fs::create_dir_all(&before).unwrap();
    std::fs::create_dir_all(&after).unwrap();
    write(&before.join("page-0001.png"), &white(20, 20));
    write(&before.join("page-0002.png"), &white(20, 20));
    write(&before.join("page-0003.png"), &white(20, 20));
    write(&after.join("page-0001.png"), &white(20, 20));
    write(&after.join("page-0002.png"), &dot(3, 4));
    write(&after.join("page-0004.png"), &white(20, 20));
    // ignored: hidden files, other extensions, subdirectories
    std::fs::write(before.join(".page-0005.png.tmp"), b"x").unwrap();
    std::fs::write(after.join("notes.txt"), b"x").unwrap();
    std::fs::create_dir_all(after.join("sub.png")).unwrap();
    let (code, stdout, stderr) = shem(&["diff", s(&before), s(&after), s(&out)]);
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(
        stdout,
        "differ: page-0002.png: 1 pixels in 3 4 4 5\nremoved: page-0003.png\nadded: page-0004.png\n"
    );
    assert_eq!(files(&out), vec!["page-0002.png"]);
}

#[test]
fn same_directories_and_extensions() {
    let dir = scratch("dirs-same");
    let (before, after, out) = (dir.join("b"), dir.join("a"), dir.join("o"));
    std::fs::create_dir_all(&before).unwrap();
    std::fs::create_dir_all(&after).unwrap();
    write(&before.join("page-0001.png"), &white(5, 5));
    write(&after.join("page-0001.png"), &white(5, 5));
    assert_eq!(
        shem(&["diff", s(&before), s(&after), s(&out)]),
        (0, String::new(), String::new())
    );
    assert!(files(&out).is_empty());
    // a PNG and a PPM of the same name are not a pair
    write(&after.join("page-0002.ppm"), &white(5, 5));
    write(&before.join("page-0002.png"), &white(5, 5));
    std::fs::write(out.join("keep.txt"), b"keep").unwrap();
    let (code, stdout, _) = shem(&["diff", s(&before), s(&after), s(&out)]);
    assert_eq!(code, 1);
    assert_eq!(stdout, "removed: page-0002.png\nadded: page-0002.ppm\n");
    assert_eq!(files(&out), vec!["keep.txt"]);
}

#[test]
fn directories_do_not_depend_on_jobs() {
    let dir = scratch("dirs-jobs");
    let (before, after) = (dir.join("b"), dir.join("a"));
    std::fs::create_dir_all(&before).unwrap();
    std::fs::create_dir_all(&after).unwrap();
    (1..=12).for_each(|n| {
        write(&before.join(format!("page-{n:04}.png")), &white(20, 20));
        let c = if n % 2 == 0 { dot(n, n) } else { white(20, 20) };
        write(&after.join(format!("page-{n:04}.png")), &c);
    });
    let run = |jobs: &str, name: &str| {
        let out = dir.join(name);
        let (code, stdout, _) = shem(&["diff", "--jobs", jobs, s(&before), s(&after), s(&out)]);
        assert_eq!(code, 1);
        let contents: Vec<(String, Vec<u8>)> = files(&out)
            .into_iter()
            .map(|f| (f.clone(), std::fs::read(out.join(&f)).unwrap()))
            .collect();
        (stdout, contents)
    };
    let first = run("1", "one");
    assert_eq!(first.1.len(), 6);
    (0..20).for_each(|k| assert_eq!(run("8", &format!("eight-{k}")), first));
}

#[test]
fn the_first_broken_pair_is_reported() {
    let dir = scratch("dirs-broken");
    let (before, after) = (dir.join("b"), dir.join("a"));
    std::fs::create_dir_all(&before).unwrap();
    std::fs::create_dir_all(&after).unwrap();
    (1..=6).for_each(|n| {
        write(&before.join(format!("page-{n:04}.png")), &white(4, 4));
        write(&after.join(format!("page-{n:04}.png")), &white(4, 4));
    });
    std::fs::write(after.join("page-0003.png"), b"broken").unwrap();
    std::fs::write(after.join("page-0005.png"), b"broken").unwrap();
    let broken = after.join("page-0003.png");
    for jobs in ["1", "8"] {
        for _ in 0..10 {
            let (code, _, stderr) = shem(&[
                "diff",
                "--jobs",
                jobs,
                s(&before),
                s(&after),
                s(&dir.join("o")),
            ]);
            assert_eq!(code, 2);
            assert_eq!(
                stderr,
                format!("shem: {}: the image is neither PNG nor PPM\n", s(&broken))
            );
        }
    }
}

#[test]
fn usage_errors() {
    let dir = scratch("usage");
    let f = dir.join("a.png");
    write(&f, &white(2, 2));
    let d = dir.join("d");
    std::fs::create_dir_all(&d).unwrap();
    let o = dir.join("o.png");
    for args in [
        vec!["diff", s(&f), s(&f)],
        vec!["diff", s(&f), s(&f), s(&o), "x"],
        vec!["diff", "--unknown", s(&f), s(&f), s(&o)],
        vec!["diff", "--format", "jpg", s(&f), s(&f), s(&o)],
        vec![
            "diff",
            "--format",
            "png",
            "--format",
            "png",
            s(&f),
            s(&f),
            s(&o),
        ],
        vec!["diff", "--jobs", "0", s(&f), s(&f), s(&o)],
        vec!["diff", s(&f), s(&d), s(&o)],
        vec!["diff", s(&d), s(&f), s(&o)],
    ] {
        let (code, _, stderr) = shem(&args);
        assert_eq!(code, 64, "{args:?}");
        assert!(stderr.contains("usage"), "{stderr}");
    }
    let help = shem(&["--help"]).1;
    for needle in ["diff", "directories", "differ:", "added:", "removed:"] {
        assert!(help.contains(needle), "{needle}");
    }
}
