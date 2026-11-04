//! The PNG writer of docs/adr/0015 writes the bytes of the one before it,
//! kept in `reference`, on images and data from a fixed seed.

use super::reference as old;
use super::*;

/// A xorshift64* generator, for inputs that are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// An image like a page: white, with dark marks, gray boxes, gradients or
/// noise, depending on `kind`.
fn page_like(rng: &mut Rng, width: usize, height: usize, kind: u64) -> Canvas {
    let mut pixels = vec![255u8; width * height * 3];
    let marks = (width * height / 20).max(1);
    (0..marks).for_each(|_| {
        let (x, y) = (
            rng.below(width as u64) as usize,
            rng.below(height as u64) as usize,
        );
        let i = (y * width + x) * 3;
        let v = rng.below(256) as u8;
        pixels[i..i + 3].copy_from_slice(&[v, v, v]);
    });
    match kind % 4 {
        0 => {}
        1 => pixels
            .iter_mut()
            .enumerate()
            .for_each(|(i, p)| *p = (i / 3 % 251) as u8),
        2 => pixels.iter_mut().for_each(|p| *p = rng.below(256) as u8),
        _ => {
            let (x0, y0) = (width / 4, height / 4);
            (y0..height - y0).for_each(|y| {
                (x0..width - x0).for_each(|x| {
                    let i = (y * width + x) * 3;
                    pixels[i..i + 3].copy_from_slice(&[192, 192, 192]);
                })
            });
        }
    }
    Canvas {
        width,
        height,
        pixels,
    }
}

#[test]
fn pngs_are_the_bytes_of_the_reference() {
    let mut rng = Rng(0x0015_0001);
    (0..120u64).for_each(|k| {
        let (w, h) = (1 + rng.below(90) as usize, 1 + rng.below(70) as usize);
        let canvas = page_like(&mut rng, w, h, k);
        assert_eq!(
            encode_png(&canvas),
            old::encode_png(&canvas),
            "{k}: {w} × {h}"
        );
    });
    // A page of A5 at 72 dpi, larger than the window of 32 KiB.
    let canvas = page_like(&mut rng, 420, 595, 0);
    assert_eq!(encode_png(&canvas), old::encode_png(&canvas));
}

#[test]
fn filters_are_the_choice_of_the_reference() {
    let mut rng = Rng(0x0015_0002);
    (0..2000).for_each(|k| {
        let len = 1 + rng.below(40) as usize;
        let row: Vec<u8> = (0..len)
            .map(|_| match k % 3 {
                0 => rng.below(256) as u8,
                1 => [0, 255, 254][rng.below(3) as usize],
                _ => 255,
            })
            .collect();
        let prev: Vec<u8> = (0..len)
            .map(|i| {
                if k % 5 == 0 {
                    row[i]
                } else {
                    rng.below(256) as u8
                }
            })
            .collect();
        [None, Some(prev.as_slice())].iter().for_each(|&p| {
            assert_eq!(choose_filter(&row, p), old::choose_filter(&row, p), "{k}");
        });
    });
}

#[test]
fn zlib_streams_are_the_bytes_of_the_reference() {
    let mut rng = Rng(0x0015_0003);
    (0..60u64).for_each(|k| {
        let len = rng.below(if k % 6 == 0 { 120_000 } else { 3000 }) as usize;
        // Runs, repeats at short and long distances, and noise.
        let alphabet = 1 + rng.below(if k % 2 == 0 { 4 } else { 256 });
        let mut data: Vec<u8> = Vec::with_capacity(len);
        while data.len() < len {
            match rng.below(4) {
                0 => {
                    let b = rng.below(alphabet) as u8;
                    let n = rng.below(600) as usize;
                    data.extend(std::iter::repeat_n(b, n));
                }
                1 if data.len() > 10 => {
                    let d = 1 + rng.below(data.len().min(40_000) as u64) as usize;
                    let n = rng.below(300) as usize;
                    (0..n).for_each(|_| data.push(data[data.len() - d]));
                }
                _ => (0..rng.below(50)).for_each(|_| data.push(rng.below(alphabet) as u8)),
            }
        }
        data.truncate(len);
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {len} bytes");
    });
}

#[test]
fn crc32_is_the_reference() {
    let mut rng = Rng(0x0015_0004);
    (0..200).for_each(|_| {
        let data: Vec<u8> = (0..rng.below(5000)).map(|_| rng.below(256) as u8).collect();
        assert_eq!(crc32(&data), old::crc32(&data));
    });
}

#[test]
fn streams_of_runs_are_the_bytes_of_the_reference() {
    // docs/adr/0015: the candidates of a run are taken at once. Rows of
    // runs with sparse bytes, as filtered pages are, at all lengths of runs
    // around the limits of a match (3, 258) and of the tries (128).
    let mut rng = Rng(0x0015_000a);
    (0..200u64).for_each(|k| {
        let width = 1 + rng.below(400) as usize;
        let rows = 1 + rng.below(if k % 10 == 0 { 300 } else { 40 }) as usize;
        let alphabet = [1u64, 2, 3, 255][(k % 4) as usize];
        let mut data = Vec::with_capacity(width * rows);
        let mut row: Vec<u8> = vec![0; width];
        (0..rows).for_each(|_| {
            // A row is often the one before, changed in a few places.
            if rng.below(3) == 0 {
                row.iter_mut().for_each(|b| *b = 0);
            }
            (0..rng.below(6)).for_each(|_| {
                let at = rng.below(width as u64) as usize;
                let len = [1, 2, 3, 4, 127, 128, 129, 257, 258, 259][rng.below(10) as usize];
                let v = rng.below(alphabet + 1) as u8;
                row[at..(at + len).min(width)]
                    .iter_mut()
                    .for_each(|b| *b = v);
            });
            data.push(rng.below(5) as u8);
            data.extend_from_slice(&row);
        });
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {} bytes", data.len());
    });
}

#[test]
fn runs_longer_than_the_window_are_the_bytes_of_the_reference() {
    // docs/adr/0015: the entries inside a run are not written; runs longer
    // than the window, and patterns at the distance of the window, read the
    // entries left from before.
    let mut rng = Rng(0x0015_000e);
    (0..12u64).for_each(|k| {
        let mut data = Vec::new();
        (0..3 + rng.below(4)).for_each(|_| {
            let run = [32767, 32768, 32769, 40000, 70000, 200][rng.below(6) as usize];
            data.extend(std::iter::repeat_n((k % 3) as u8, run));
            (0..rng.below(300)).for_each(|_| data.push(rng.below(4) as u8));
        });
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {} bytes", data.len());
    });
}
