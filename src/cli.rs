//! The command line: `shem render` (docs/adr/0007).

use std::io::Write;
use std::path::{Path, PathBuf};

use std::num::NonZeroUsize;

use crate::error::Error;
use crate::parallel;
use crate::pdf::Document;
use crate::png::{encode_png, encode_ppm, write_ppm};
use crate::render::{PageError, Session};

const USAGE: &str = "usage: shem render [--dpi <N>] [--format png|ppm] [--pages <range>] [--jobs <N>] <input.pdf> <output directory>
       shem diff [--format png|ppm] [--jobs <N>] <before> <after> <output>";

const HELP: &str =
    "shem: renders the pages of a PDF to PNG or PPM images, with the same bytes on every machine.

usage:
  shem render [--dpi <N>] [--format png|ppm] [--pages <range>] [--jobs <N>]
              <input.pdf> <output directory>
  shem diff [--format png|ppm] [--jobs <N>] <before> <after> <output>
  shem --version
  shem --help

render writes each page to <output directory>/page-<number>.<png|ppm>. The
number counts from 1 and has as many digits as the page count, at least 4
(page-0001.png). The directory is made if it does not exist. A file of the
same name is replaced; other files are left alone. Each file is written
under a temporary name and then renamed.

options of render:
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

Exit status of render:
  0   every page was written
  1   the input is outside the subset or broken, or the output failed
  64  the command line is wrong

diff compares images that shem wrote (8-bit RGB PNG or PPM) and writes a
diff image where they differ: red where the after image is darker (ink
added), blue where it is lighter (ink removed), purple where only the color
changed, green where only one image has pixels, and a faded gray where
they are the same, with an orange frame around the differences.

  Two files: <output> is the diff image, written only if they differ, and
  one line goes to standard output:
    differ: <count> pixels in <x0> <y0> <x1> <y1>
  Two directories: the .png and .ppm files directly in them are paired by
  name; a diff image is written to the directory <output> under the name of
  each pair that differs, and lines go to standard output in the order of
  the names:
    differ: <name>: <count> pixels in <x0> <y0> <x1> <y1>
    added: <name>      (only in <after>)
    removed: <name>    (only in <before>)

options of diff:
  --format png|ppm   the format of diff images (default png)
  --jobs <N>         the number of pairs compared at once (default: the
                     number of cores)

Exit status of diff:
  0   no difference
  1   differences
  2   an image cannot be read or the output failed
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
                if jobs.replace(parse_jobs(&value("--jobs")?)?).is_some() {
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
    write_file_with(dir, name, |f| f.write_all(data))
}

/// Writes a file with `write` under a temporary name and renames it.
fn write_file_with(
    dir: &Path,
    name: &str,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let result = std::fs::File::create(&tmp)
        .and_then(|mut f| write(&mut f))
        .and_then(|()| std::fs::rename(&tmp, dir.join(name)));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn render(r: &Render) -> std::result::Result<(), Error> {
    let data = std::fs::read(&r.input).map_err(|e| Error::new(e.to_string()))?;
    let doc = Document::parse(&data)?;
    // The fonts are read once for the check and the rendering of every page
    // (docs/adr/0011), and the pages go to threads (docs/adr/0009, docs/adr/0012).
    let session = Session::new(&doc, r.dpi)?;
    let count = session.page_count();
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
    let indices: Vec<usize> = selected.iter().map(|n| n - 1).collect();
    // Every page is checked before any file is written (docs/adr/0002).
    session.check_pages(&indices, r.jobs)?;
    let dir = PathBuf::from(&r.output);
    let io = |e: std::io::Error| Error::new(format!("{}: {e}", r.output));
    std::fs::create_dir_all(&dir).map_err(io)?;
    let digits = count.to_string().len().max(4);
    let extension = if r.ppm { "ppm" } else { "png" };
    session
        .render_pages(&indices, r.jobs, |page, canvas| {
            let n = page + 1;
            let name = format!("page-{n:0digits$}.{extension}");
            if r.ppm {
                // The pixels go to the file as they are (docs/adr/0013).
                write_file_with(&dir, &name, |f| write_ppm(&canvas, f))
            } else {
                write_file(&dir, &name, &encode_png(&canvas))
            }
        })
        .map_err(|e| match e {
            PageError::Render(e) => e,
            PageError::Consume(_, e) => io(e),
        })
}

/// The options of `diff`.
struct DiffArgs {
    ppm: bool,
    jobs: NonZeroUsize,
    before: String,
    after: String,
    output: String,
}

fn parse_jobs(v: &str) -> std::result::Result<NonZeroUsize, String> {
    v.parse::<usize>()
        .ok()
        .filter(|n| (1..=1024).contains(n) && v.bytes().all(|c| c.is_ascii_digit()))
        .and_then(NonZeroUsize::new)
        .ok_or_else(|| format!("--jobs must be an integer from 1 to 1024: {v}"))
}

fn parse_diff(args: &[String]) -> std::result::Result<DiffArgs, String> {
    let mut format = None;
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
            "--format" => {
                let v = value("--format")?;
                let ppm = match v.as_str() {
                    "png" => false,
                    "ppm" => true,
                    _ => return Err(format!("--format must be png or ppm: {v}")),
                };
                if format.replace(ppm).is_some() {
                    return Err("--format is given twice".into());
                }
            }
            "--jobs" => {
                if jobs.replace(parse_jobs(&value("--jobs")?)?).is_some() {
                    return Err("--jobs is given twice".into());
                }
            }
            a if a.starts_with('-') && a.len() > 1 => return Err(format!("unknown option {a}")),
            _ => positional.push(arg.clone()),
        }
    }
    match positional.as_slice() {
        [before, after, output] => Ok(DiffArgs {
            ppm: format.unwrap_or(false),
            jobs: jobs.unwrap_or_else(parallel::available_jobs),
            before: before.clone(),
            after: after.clone(),
            output: output.clone(),
        }),
        _ => Err("diff takes the before image, the after image and the output".into()),
    }
}

/// The number of pixels that differ and the box around them.
type Difference = (usize, (usize, usize, usize, usize));

/// An error of diff: the path it is about, and what happened.
struct DiffError {
    path: String,
    message: String,
}

fn diff_error(path: &Path, message: impl std::fmt::Display) -> DiffError {
    DiffError {
        path: path.display().to_string(),
        message: message.to_string(),
    }
}

fn read_image(path: &Path) -> std::result::Result<crate::raster::Canvas, DiffError> {
    let data = std::fs::read(path).map_err(|e| diff_error(path, e))?;
    crate::image::decode(&data).map_err(|e| diff_error(path, e))
}

/// Compares two images and writes the diff image to `dir/name` if they differ.
fn diff_pair(
    before: &Path,
    after: &Path,
    dir: &Path,
    name: &str,
    ppm: bool,
) -> std::result::Result<Option<Difference>, DiffError> {
    let (a, b) = (read_image(before)?, read_image(after)?);
    match crate::diff::diff(&a, &b) {
        None => Ok(None),
        Some(d) => {
            let bytes = if ppm {
                encode_ppm(&d.image)
            } else {
                encode_png(&d.image)
            };
            write_file(dir, name, &bytes).map_err(|e| diff_error(&dir.join(name), e))?;
            Ok(Some((d.count, d.bbox)))
        }
    }
}

fn differ_line(count: usize, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> String {
    format!("{count} pixels in {x0} {y0} {x1} {y1}")
}

/// The names of the images directly in a directory: regular files that end
/// with .png or .ppm and do not begin with a dot.
fn image_names(dir: &Path) -> std::result::Result<std::collections::BTreeSet<String>, DiffError> {
    let entries = std::fs::read_dir(dir).map_err(|e| diff_error(dir, e))?;
    entries
        .map(|entry| entry.map_err(|e| diff_error(dir, e)))
        .filter_map(|entry| {
            entry
                .map(|e| {
                    let name = e.file_name().into_string().ok()?;
                    let is_file = std::fs::metadata(e.path()).is_ok_and(|m| m.is_file());
                    let image = name.ends_with(".png") || name.ends_with(".ppm");
                    (is_file && image && !name.starts_with('.')).then_some(name)
                })
                .transpose()
        })
        .collect()
}

/// Runs diff and returns the lines for standard output and whether anything differs.
fn diff(d: &DiffArgs) -> std::result::Result<(Vec<String>, bool), DiffError> {
    let (before, after) = (Path::new(&d.before), Path::new(&d.after));
    let extension = if d.ppm { "ppm" } else { "png" };
    if before.is_file() {
        let output = Path::new(&d.output);
        let dir = match output.parent() {
            Some(p) if !p.as_os_str().is_empty() => p,
            _ => Path::new("."),
        };
        let name = output
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| diff_error(output, "not a file name"))?;
        return Ok(match diff_pair(before, after, dir, name, d.ppm)? {
            None => (Vec::new(), false),
            Some((count, bbox)) => (vec![format!("differ: {}", differ_line(count, bbox))], true),
        });
    }
    let (names_before, names_after) = (image_names(before)?, image_names(after)?);
    let pairs: Vec<&String> = names_before.intersection(&names_after).collect();
    let out = Path::new(&d.output);
    std::fs::create_dir_all(out).map_err(|e| diff_error(out, e))?;
    let results = std::sync::Mutex::new(std::collections::BTreeMap::new());
    parallel::run(pairs.len(), d.jobs, |k| {
        let name = pairs[k];
        let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
        let result = diff_pair(
            &before.join(name),
            &after.join(name),
            out,
            &format!("{stem}.{extension}"),
            d.ppm,
        )?;
        if let Some((count, bbox)) = result {
            let line = format!("differ: {name}: {}", differ_line(count, bbox));
            results
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(name.clone(), line);
        }
        Ok(())
    })
    .map_err(|(_, e)| e)?;
    let mut lines = results.into_inner().unwrap_or_else(|p| p.into_inner());
    names_after.difference(&names_before).for_each(|n| {
        lines.insert(n.clone(), format!("added: {n}"));
    });
    names_before.difference(&names_after).for_each(|n| {
        lines.insert(n.clone(), format!("removed: {n}"));
    });
    let differs = !lines.is_empty();
    Ok((lines.into_values().collect(), differs))
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
        Some("diff") => match parse_diff(&args[1..]) {
            Err(message) => usage(err, &message),
            Ok(d) if Path::new(&d.before).is_dir() != Path::new(&d.after).is_dir() => usage(
                err,
                "diff compares two files or two directories, not a file and a directory",
            ),
            Ok(d) => match diff(&d) {
                Ok((lines, differs)) => {
                    lines.iter().for_each(|l| {
                        let _ = writeln!(out, "{l}");
                    });
                    i32::from(differs)
                }
                Err(e) => {
                    let _ = writeln!(err, "shem: {}: {}", e.path, e.message);
                    2
                }
            },
        },
        Some(other) => usage(err, &format!("unknown command {other}")),
        None => usage(err, "no command"),
    }
}
