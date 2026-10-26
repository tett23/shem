//! The command line: `shem render` (docs/adr/0007).

use std::io::Write;
use std::path::{Path, PathBuf};

use std::num::NonZeroUsize;

use crate::error::Error;
use crate::parallel;
use crate::pdf::Document;
use crate::png::{encode_png, encode_ppm};
use crate::render::{check_page, render_page};

const USAGE: &str = "usage: shem render [--dpi <N>] [--format png|ppm] [--pages <range>] [--jobs <N>] <input.pdf> <output directory>";

const HELP: &str =
    "shem: renders the pages of a PDF to PNG or PPM images, with the same bytes on every machine.

usage:
  shem render [--dpi <N>] [--format png|ppm] [--pages <range>] [--jobs <N>]
              <input.pdf> <output directory>
  shem --version
  shem --help

render writes each page to <output directory>/page-<number>.<png|ppm>. The
number counts from 1 and has as many digits as the page count, at least 4
(page-0001.png). The directory is made if it does not exist. A file of the
same name is replaced; other files are left alone. Each file is written
under a temporary name and then renamed.

options:
  --dpi <N>          the resolution, an integer from 1 to 1200 (default 150)
  --format png|ppm   the image format (default png). The bytes of a PPM
                     depend only on the pixels; those of a PNG may change
                     between versions of shem with the same pixels
  --pages <range>    the pages to render, such as 3, 2-5 or 1,3,5-7
                     (default all)
  --jobs <N>         the number of pages rendered at once, an integer from 1
                     to 1024 (default: the number of cores). The output does
                     not depend on it

The size of an image is the size of the MediaBox times dpi / 72, rounded up.

shem reads the subset of PDF that Golem writes. Before anything is
written, every page to render is read and checked; anything outside the
subset stops shem with an error such as

  shem: input.pdf: page 3: offset 120: unsupported operator TJ

and no file is written. Images and forms are drawn as gray boxes with a
cross.

Exit status:
  0   every page was written
  1   the input is outside the subset or broken, or the output failed
  64  the command line is wrong
";

/// The options of `render`.
struct Render {
    dpi: u32,
    jobs: NonZeroUsize,
    ppm: bool,
    pages: Option<Vec<(usize, usize)>>,
    input: String,
    output: String,
}

fn usage_error(message: &str) -> std::result::Result<Render, String> {
    Err(message.to_string())
}

fn parse_pages(s: &str) -> Option<Vec<(usize, usize)>> {
    s.split(',')
        .map(|item| {
            let number = |t: &str| {
                t.parse::<usize>()
                    .ok()
                    .filter(|&n| n >= 1 && t.bytes().all(|c| c.is_ascii_digit()))
            };
            match item.split_once('-') {
                Some((a, b)) => number(a).zip(number(b)).filter(|(a, b)| a <= b),
                None => number(item).map(|n| (n, n)),
            }
        })
        .collect()
}

fn parse_render(args: &[String]) -> std::result::Result<Render, String> {
    let mut dpi = None;
    let mut format = None;
    let mut pages = None;
    let mut jobs = None;
    let mut positional = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--dpi" => {
                let v = value("--dpi")?;
                let n = v
                    .parse::<u32>()
                    .ok()
                    .filter(|n| (1..=1200).contains(n) && v.bytes().all(|c| c.is_ascii_digit()))
                    .ok_or_else(|| format!("--dpi must be an integer from 1 to 1200: {v}"))?;
                if dpi.replace(n).is_some() {
                    return usage_error("--dpi is given twice");
                }
            }
            "--format" => {
                let v = value("--format")?;
                let ppm = match v.as_str() {
                    "png" => false,
                    "ppm" => true,
                    _ => return usage_error(&format!("--format must be png or ppm: {v}")),
                };
                if format.replace(ppm).is_some() {
                    return usage_error("--format is given twice");
                }
            }
            "--pages" => {
                let v = value("--pages")?;
                let ranges = parse_pages(&v).ok_or_else(|| format!("malformed --pages: {v}"))?;
                if pages.replace(ranges).is_some() {
                    return usage_error("--pages is given twice");
                }
            }
            "--jobs" => {
                let v = value("--jobs")?;
                let n = v
                    .parse::<usize>()
                    .ok()
                    .filter(|n| (1..=1024).contains(n) && v.bytes().all(|c| c.is_ascii_digit()))
                    .and_then(NonZeroUsize::new)
                    .ok_or_else(|| format!("--jobs must be an integer from 1 to 1024: {v}"))?;
                if jobs.replace(n).is_some() {
                    return usage_error("--jobs is given twice");
                }
            }
            a if a.starts_with('-') && a.len() > 1 => {
                return usage_error(&format!("unknown option {a}"));
            }
            _ => positional.push(arg.clone()),
        }
    }
    match positional.as_slice() {
        [input, output] => Ok(Render {
            dpi: dpi.unwrap_or(150),
            jobs: jobs.unwrap_or_else(parallel::available_jobs),
            ppm: format.unwrap_or(false),
            pages,
            input: input.clone(),
            output: output.clone(),
        }),
        _ => usage_error("render takes an input PDF and an output directory"),
    }
}

/// Writes a file under a temporary name and renames it.
fn write_file(dir: &Path, name: &str, data: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let result = std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, dir.join(name)));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn render(r: &Render) -> std::result::Result<(), Error> {
    let data = std::fs::read(&r.input).map_err(|e| Error::new(e.to_string()))?;
    let doc = Document::parse(&data)?;
    let pages = doc.pages()?;
    let count = pages.len();
    let selected: Vec<usize> = match &r.pages {
        None => (1..=count).collect(),
        Some(ranges) => {
            let mut numbers: Vec<usize> = ranges.iter().flat_map(|&(a, b)| a..=b).collect();
            numbers.sort_unstable();
            numbers.dedup();
            if let Some(&n) = numbers.iter().find(|&&n| n > count) {
                return Err(Error::new(format!(
                    "page {n} does not exist: the document has {count} pages"
                )));
            }
            numbers
        }
    };
    // Every page is checked before any file is written (docs/adr/0002). The
    // pages are checked and then rendered in parallel; the error reported is
    // the one of the first page that fails (docs/adr/0009).
    parallel::run(selected.len(), r.jobs, |k| {
        check_page(&doc, &pages[selected[k] - 1], r.dpi)
    })
    .map_err(|(_, e)| e)?;
    let dir = PathBuf::from(&r.output);
    let io = |e: std::io::Error| Error::new(format!("{}: {e}", r.output));
    std::fs::create_dir_all(&dir).map_err(io)?;
    let digits = count.to_string().len().max(4);
    let extension = if r.ppm { "ppm" } else { "png" };
    parallel::run(selected.len(), r.jobs, |k| {
        let n = selected[k];
        let canvas = render_page(&doc, &pages[n - 1], r.dpi)?;
        let bytes = if r.ppm {
            encode_ppm(&canvas)
        } else {
            encode_png(&canvas)
        };
        write_file(&dir, &format!("page-{n:0digits$}.{extension}"), &bytes).map_err(io)
    })
    .map_err(|(_, e)| e)
}

/// Runs the command line and returns the exit status.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let usage = |err: &mut dyn Write, message: &str| {
        let _ = writeln!(err, "shem: {message}\n{USAGE}");
        64
    };
    match args.first().map(String::as_str) {
        Some("--version" | "-V") if args.len() == 1 => {
            let _ = writeln!(out, "shem {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--help" | "-h") if args.len() == 1 => {
            let _ = write!(out, "{HELP}");
            0
        }
        Some("render") => match parse_render(&args[1..]) {
            Err(message) => usage(err, &message),
            Ok(r) => match render(&r) {
                Ok(()) => 0,
                Err(e) => {
                    let _ = writeln!(err, "shem: {}: {e}", r.input);
                    1
                }
            },
        },
        Some(other) => usage(err, &format!("unknown command {other}")),
        None => usage(err, "no command"),
    }
}
