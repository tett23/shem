//! The inflation of docs/adr/0015 gives the bytes, or the error, of the one
//! before it, kept in `reference`.

use super::reference as old;
use super::*;

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

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/flate/{name}.zz",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn same(data: &[u8]) {
    let (new, before) = (inflate_zlib(data, 1 << 26), old::inflate_zlib(data));
    match (&new, &before) {
        (Ok(a), Ok(b)) => assert_eq!(a, b),
        (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
        _ => panic!("{new:?} {before:?}"),
    }
}

#[test]
fn streams_and_their_corruptions_inflate_as_before() {
    let mut rng = Rng(0x0015_0007);
    let mut streams: Vec<Vec<u8>> = ["stored", "fixed", "dynamic", "empty", "long"]
        .iter()
        .map(|n| fixture(n))
        .collect();
    // The content streams of Golem's PDFs, with dynamic codes of their own.
    let doc = crate::pdf::Document::parse(
        &std::fs::read(format!(
            "{}/tests/fixtures/golem/sample.pdf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap();
    doc.object_numbers().for_each(|num| {
        if let Ok(crate::pdf::Object::Stream(s)) = doc.load(num) {
            streams.push(s.data.clone());
        }
    });
    // The streams of shem's PNG writer, with long matches.
    (0..20).for_each(|k| {
        let data: Vec<u8> = (0..rng.below(20000))
            .map(|i| {
                if k % 2 == 0 {
                    (i / 300) as u8
                } else {
                    rng.below(4) as u8
                }
            })
            .collect();
        streams.push(crate::png::zlib(&data));
    });
    streams.iter().for_each(|s| {
        same(s);
        // Truncations, and flips of one bit.
        (0..30).for_each(|_| {
            if s.is_empty() {
                return;
            }
            same(&s[..rng.below(s.len() as u64) as usize]);
            let mut flipped = s.clone();
            let i = rng.below(s.len() as u64) as usize;
            flipped[i] ^= 1 << rng.below(8);
            same(&flipped);
        });
    });
}

#[test]
fn adler32_is_the_reference() {
    let mut rng = Rng(0x0015_000b);
    (0..300).for_each(|k| {
        let len = rng.below(if k % 10 == 0 { 40_000 } else { 100 }) as usize;
        let data: Vec<u8> = (0..len)
            .map(|_| {
                if k % 3 == 0 {
                    255
                } else {
                    rng.below(256) as u8
                }
            })
            .collect();
        assert_eq!(adler32(&data), old::adler32(&data), "{len}");
    });
}

#[test]
fn adler32_around_its_parts_is_the_reference() {
    // docs/adr/0019: the lanes are summed in u16 over parts of 256 blocks.
    let mut rng = Rng(0x0019_0004);
    let edges = [
        0, 1, 15, 16, 17, 255, 256, 257, 4095, 4096, 4097, 5551, 5552, 5553,
    ];
    (0..300).for_each(|k| {
        let len = match k % 3 {
            0 => edges[k / 3 % edges.len()] + 16 * (k / 3 / edges.len()) * 256,
            _ => rng.below(20_000) as usize,
        };
        let data: Vec<u8> = (0..len)
            .map(|_| {
                if k % 2 == 0 {
                    255
                } else {
                    rng.below(256) as u8
                }
            })
            .collect();
        assert_eq!(adler32(&data), old::adler32(&data), "{len}");
    });
}

#[test]
fn adler32_at_the_edges_of_its_chunks_and_parts_is_the_reference() {
    // docs/adr/0019: the 16 lanes are summed in u16 over parts of 256 blocks
    // (4096 bytes), inside chunks of 5552 bytes; a part of 255s sums to
    // 65280 in a lane, the most u16 is asked to hold. Lengths at the edges
    // of the parts and the chunks, over up to four chunks, of 255s and of
    // random bytes.
    let mut rng = Rng(0x0019_000b);
    let edges = [
        0, 1, 15, 16, 17, 255, 256, 257, 4095, 4096, 4097, 4111, 4112, 4113, 5551,
    ];
    (0..4usize).for_each(|m| {
        edges.iter().for_each(|&e| {
            let len = 5552 * m + e;
            [true, false].iter().for_each(|&flat| {
                let data: Vec<u8> = (0..len)
                    .map(|_| if flat { 255 } else { rng.below(256) as u8 })
                    .collect();
                assert_eq!(adler32(&data), old::adler32(&data), "{len} {flat}");
            });
        });
    });
}
