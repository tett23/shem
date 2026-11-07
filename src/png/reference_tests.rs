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

#[test]
fn runs_around_the_longest_match_are_the_bytes_of_the_reference() {
    // docs/adr/0019: the run at the position is used again after the
    // search, and the candidate just before it takes the run plus one.
    // Runs of 1 to 300 bytes, with 1 to 3 bytes between, around the longest
    // match (258) and the window (32 KiB).
    let mut rng = Rng(0x0019_0001);
    (0..300u64).for_each(|k| {
        let mut data = Vec::new();
        let target = if k % 30 == 0 { 70_000 } else { 3000 };
        while data.len() < target {
            let run = match rng.below(4) {
                0 => 255 + rng.below(8) as usize,
                1 if k % 30 == 0 => 32_760 + rng.below(16) as usize,
                _ => 1 + rng.below(300) as usize,
            };
            data.extend(std::iter::repeat_n((k % 3) as u8, run));
            (0..1 + rng.below(3)).for_each(|_| data.push(rng.below(4) as u8));
        }
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {} bytes", data.len());
    });
}

#[test]
fn lengths_of_matches_and_runs_are_counted_as_bytes() {
    // docs/adr/0019: the tails are compared as one word over equal bytes.
    let mut rng = Rng(0x0019_0002);
    (0..20_000u64).for_each(|k| {
        let len = 1 + rng.below(80) as usize;
        let alphabet = 1 + rng.below(3);
        let data: Vec<u8> = (0..len).map(|_| rng.below(alphabet) as u8).collect();
        let i = rng.below(len as u64) as usize;
        let candidate = rng.below(i as u64 + 1) as usize;
        let max = rng.below((len - i).min(40) as u64 + 1) as usize;
        let expected = (0..max)
            .take_while(|&n| data[candidate + n] == data[i + n])
            .count();
        assert_eq!(match_length(&data, candidate, i, max), expected, "{k}");
        let b = data[i];
        let forward = data[i..].iter().take_while(|&&v| v == b).count();
        assert_eq!(run_length(&data[i..], b), forward, "{k}");
        let low = rng.below(i as u64 + 1) as usize;
        let back = data[low..i].iter().rev().take_while(|&&v| v == b).count();
        assert_eq!(run_back(&data, low, i, b), back, "{k}");
    });
}

#[test]
fn costs_of_filters_are_the_sums_of_the_filtered_bytes() {
    // docs/adr/0019: rows up to 2^24 bytes are summed in u32.
    let mut rng = Rng(0x0019_0003);
    (0..300u64).for_each(|k| {
        let len = rng.below(5000) as usize;
        let row: Vec<u8> = (0..len)
            .map(|_| {
                if k % 5 == 0 {
                    255
                } else {
                    rng.below(256) as u8
                }
            })
            .collect();
        let prev: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
        [None, Some(prev.as_slice())].iter().for_each(|&p| {
            (0..5u8).for_each(|kind| {
                let mut out = vec![0; len];
                filter_into(kind, &row, p, &mut out);
                let sum: u64 = out
                    .iter()
                    .map(|&b| u64::from((b as i8).unsigned_abs()))
                    .sum();
                assert_eq!(filter_cost(kind, &row, p), sum, "{k} {kind}");
            });
        });
    });
    // 2^24 bytes of 128 cost 2^31, which u32 holds.
    assert!((1u64 << 24) * 128 <= u64::from(u32::MAX));
}

#[test]
fn short_data_is_the_bytes_of_the_reference() {
    // docs/adr/0019: the tails of `match_length` and `run_length` compare one
    // word over bytes already found equal, and the run at a position is used
    // again when its positions are added, reading at most two bytes past the
    // longest match; near the end of the data, the longest match is cut by
    // the data. Every byte string of up to 11 bytes over two values and of
    // up to 7 over three: every run and match ends within a word of the end.
    let strings = |values: &'static [u8], up_to: u32| {
        (0..=up_to).flat_map(move |len| {
            let n = values.len() as u64;
            (0..n.pow(len)).map(move |mut code| {
                (0..len)
                    .map(|_| {
                        let b = values[(code % n) as usize];
                        code /= n;
                        b
                    })
                    .collect::<Vec<u8>>()
            })
        })
    };
    strings(&[0, 255], 11)
        .chain(strings(&[0, 1, 255], 7))
        .for_each(|data| assert_eq!(zlib(&data), old::zlib(&data), "{data:?}"));
}

/// A [`Group`] as a tuple: `(done, len, dist, best, last, count)`.
type Flat = (bool, usize, usize, Option<(usize, usize)>, usize, usize);

/// The candidates `c`, `c − 1`, … of the run of `data[c]` down to the
/// lowest the tries and the window allow, taken one by one as the search
/// did before docs/adr/0015, and how many they are; None if the four bytes
/// around `c` are not a run.
fn one_by_one(
    data: &[u8],
    i: usize,
    c: usize,
    max: usize,
    tries: usize,
    best_len: usize,
) -> Option<(Flat, usize)> {
    let b = data[c];
    if c == 0 || data[c - 1..c + 3] != [b; 4] {
        return None;
    }
    let low = c
        .saturating_sub(MAX_CHAIN - tries - 1)
        .max(i.saturating_sub(WINDOW));
    let last = (low..=c)
        .rev()
        .take_while(|&p| data[p] == b)
        .last()
        .expect("c itself is b");
    let count = c - last + 1;
    let mut best = None;
    let mut best_len = best_len;
    for p in (last..=c).rev() {
        let len = (0..max).take_while(|&n| data[p + n] == data[i + n]).count();
        if len > best_len {
            best_len = len;
            best = Some((len, i - p));
            if len == max {
                return Some(((true, max, i - p, None, 0, 0), count));
            }
        }
    }
    Some(((false, 0, 0, best, last, count), count))
}

#[test]
fn groups_of_runs_are_the_candidates_one_by_one() {
    // docs/adr/0015 and 0019: `run_group` takes the candidates of a run at
    // once, with the lengths that follow from the lengths of the runs, the
    // candidate just before the position taking the run plus one, and the
    // run at the position kept between candidates. Its result is the one of
    // the candidates taken one by one: the first that reaches `max`, or the
    // best among them if longer than before, with the last candidate and
    // the count of them; it leaves a lone candidate to the search, except
    // the one just before a run of `max`, which it ends. The data are runs
    // of 0 (or 255) among 1 and 32; the triple 0 1 32 hashes as 0 0 0, so
    // it is in the chain of the runs of 0, as a candidate (its four bytes
    // are not a run) and as the position (its run is 0 or 1).
    let mut rng = Rng(0x0019_0005);
    let mut groups = 0;
    (0..6000u64).for_each(|k| {
        let b = if k % 5 == 4 { 255 } else { 0 };
        let target = 20 + rng.below(if k % 4 == 0 { 700 } else { 120 }) as usize;
        let mut data = Vec::new();
        while data.len() < target {
            match rng.below(3) {
                0 => {
                    let n = 1 + rng.below(if k % 4 == 0 { 300 } else { 12 }) as usize;
                    data.extend(std::iter::repeat_n(b, n));
                }
                1 => data.extend([0, 1, 32]),
                _ => (0..1 + rng.below(4)).for_each(|_| data.push([1, 32][rng.below(2) as usize])),
            }
        }
        let positions: Vec<usize> = (0..data.len() - 2)
            .filter(|&p| hash(&data, p) == hash3(b, b, b))
            .collect();
        if positions.len() < 2 {
            return;
        }
        let ci = rng.below(positions.len() as u64 - 1) as usize;
        let c = positions[ci];
        let i = positions[ci + 1 + rng.below((positions.len() - ci - 1).min(400) as u64) as usize];
        let max = MAX_MATCH.min(data.len() - i);
        let tries = rng.below(MAX_CHAIN as u64) as usize;
        let best_len = rng.below(max as u64) as usize;
        let r = data[i..i + max].iter().take_while(|&&v| v == b).count();
        // The run at the position is None before the first run of the
        // search, and what the search found after it.
        let mut run_at_i = if rng.below(2) == 0 {
            None
        } else {
            Some((b, r))
        };
        let group = run_group(&data, i, c, max, tries, best_len, &mut run_at_i).map(|g| match g {
            Group::Done(len, dist) => (true, len, dist, None, 0, 0),
            Group::Next { best, last, count } => (false, 0, 0, best, last, count),
        });
        let expected = one_by_one(&data, i, c, max, tries, best_len);
        let what = format!("{k}: i {i} c {c} max {max} tries {tries} best {best_len}");
        match (group, expected) {
            (Some(g), Some((e, _))) => assert_eq!(g, e, "{what}"),
            (None, Some((_, count))) => assert!(count < 2, "{what}"),
            (Some(g), None) => panic!("{what}: {g:?} without a run"),
            (None, None) => {}
        }
        if group.is_some() {
            groups += 1;
            assert_eq!(run_at_i, Some((b, r)), "{what}");
        }
    });
    assert!(groups > 2000, "{groups} groups");
}

#[test]
fn runs_at_the_edge_of_the_window_are_the_bytes_of_the_reference() {
    // docs/adr/0015 and 0019: a run of candidates is taken down to the
    // lowest the window allows, and the entries inside a run are not
    // written; the entry read at the lowest candidate is left from before
    // or points before the window. A run of 0, then about 32 KiB of 1 to 31
    // (no three of which hash as 0 0 0, their first byte filling bits 10 to
    // 14 of the hash), then a run of 0 again: the first run lies at the
    // edge of the window of the second, which cuts it, ends at its last
    // candidate (a distance of 32768), or leaves it out (32769).
    let mut rng = Rng(0x0019_0007);
    (0..60u64).for_each(|k| {
        let mut data = Vec::new();
        (0..2 + rng.below(3)).for_each(|_| {
            let run = [
                3, 4, 5, 8, 9, 127, 128, 129, 130, 257, 258, 259, 260, 600, 40_000,
            ][rng.below(15) as usize];
            data.extend(std::iter::repeat_n(0u8, run));
            let filler = match rng.below(5) {
                0 => 32_763,
                1 => 32_764,
                2 => 32_765,
                3 => 32_766,
                _ => 32_600 + rng.below(180) as usize,
            };
            data.extend((0..filler).map(|_| 1 + rng.below(31) as u8));
        });
        data.extend(std::iter::repeat_n(0u8, 3 + rng.below(300) as usize));
        (0..rng.below(40)).for_each(|_| data.push(rng.below(4) as u8));
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {} bytes", data.len());
    });
}

#[test]
fn copies_at_the_distance_of_the_window_are_the_bytes_of_the_reference() {
    // docs/adr/0015: a candidate at 32768 is in the window and one at 32769
    // is not. A block of 32766 to 32770 bytes, then its first 600 bytes
    // again, so that the candidates of the copy are at the length of the
    // block; the block starts with a run, whose candidates are then at the
    // edge, and goes on as runs and noise over two values, or as noise.
    let mut rng = Rng(0x0019_000a);
    (0..5usize).for_each(|d| {
        [2u64, 256].iter().for_each(|&alphabet| {
            let len = 32_766 + d;
            let mut data: Vec<u8> = std::iter::repeat_n(0u8, 10)
                .chain((10..len).map(|_| rng.below(alphabet) as u8))
                .collect();
            data.extend_from_within(..600);
            (0..rng.below(40)).for_each(|_| data.push(rng.below(alphabet) as u8));
            assert_eq!(zlib(&data), old::zlib(&data), "{d} {alphabet}");
        });
    });
}

#[test]
fn periodic_data_is_the_bytes_of_the_reference() {
    // docs/adr/0015 and 0019: in data of a short period, every candidate
    // matches to `max` or the 128 candidates of the chain run out at 128
    // periods; in `0 0 0 1` and the like, the four-byte check of a run
    // fails at the start of each run, and the groups have two or three
    // candidates. Periods 1 to 10, of random bytes or of a run of 0 and a
    // separator, at lengths around multiples of 258 and of 128 periods.
    let mut rng = Rng(0x0019_0008);
    (0..400u64).for_each(|k| {
        let period = 1 + rng.below(10) as usize;
        let pattern: Vec<u8> = (0..period)
            .map(|j| match (k % 2, j + 1 < period) {
                (0, _) => rng.below(4) as u8,
                (_, true) => 0,
                (_, false) => 1 + rng.below(3) as u8,
            })
            .collect();
        let len = match rng.below(3) {
            0 => rng.below(1300) as usize,
            1 => 256 + 258 * rng.below(5) as usize + rng.below(5) as usize,
            _ => 128 * period + rng.below(7) as usize,
        };
        let data: Vec<u8> = (0..len).map(|x| pattern[x % period]).collect();
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {pattern:?} × {len}");
    });
}

#[test]
fn runs_split_by_matches_are_the_bytes_of_the_reference() {
    // docs/adr/0015 and 0019: a match that ends inside a run splits the run
    // between two additions of positions; the chain of the run then passes
    // the split through the entry written where the later part starts, and
    // `run_back` counts the run across it. Lines of a separator and a run of
    // 0 (or 255) whose length changes by a few bytes from line to line, so
    // that the match of the line before ends inside the run.
    let mut rng = Rng(0x0019_0009);
    (0..40u64).for_each(|k| {
        let mut data = Vec::new();
        let mut len = 3 + rng.below(300) as usize;
        (0..80).for_each(|_| {
            (0..1 + rng.below(3)).for_each(|_| data.push(1 + rng.below(2) as u8));
            len = match rng.below(6) {
                0 => 3 + rng.below(600) as usize,
                _ => (len + rng.below(7) as usize).saturating_sub(3).max(1),
            };
            let b = if k % 3 == 2 && rng.below(2) == 0 {
                255
            } else {
                0
            };
            data.extend(std::iter::repeat_n(b, len));
        });
        assert_eq!(zlib(&data), old::zlib(&data), "{k}: {} bytes", data.len());
    });
}

#[test]
fn costs_of_long_rows_are_the_sums_of_the_filtered_bytes() {
    // docs/adr/0019: rows of at most 2^24 bytes are summed in u32, longer
    // rows in u64. A row of 2^24 bytes of 128 costs 2^31 unfiltered, the
    // most a row of the u32 path can; a row of 2^24 + 1 bytes takes the u64
    // path; a row of 2^25 bytes of 128 costs 2^32, which u32 does not hold.
    let mut rng = Rng(0x0019_0006);
    let row: Vec<u8> = (0..(1usize << 24) + 1)
        .map(|x| {
            if x % 1000 == 999 {
                rng.below(256) as u8
            } else {
                128
            }
        })
        .collect();
    let prev: Vec<u8> = (0..(1usize << 24) + 1)
        .map(|_| rng.below(256) as u8)
        .collect();
    [1 << 24, (1 << 24) + 1].iter().for_each(|&len| {
        let row = &row[..len];
        [None, Some(&prev[..len])].iter().for_each(|&p| {
            (0..5u8).for_each(|kind| {
                let mut out = vec![0; len];
                filter_into(kind, row, p, &mut out);
                let sum: u64 = out
                    .iter()
                    .map(|&b| u64::from((b as i8).unsigned_abs()))
                    .sum();
                assert_eq!(filter_cost(kind, row, p), sum, "{len} {kind}");
            });
        });
    });
    assert_eq!(filter_cost(0, &vec![128; 1 << 24], None), 1 << 31);
    assert_eq!(filter_cost(0, &vec![128; 1 << 25], None), 1 << 32);
}
