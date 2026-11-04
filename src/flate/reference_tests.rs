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
    let (new, before) = (inflate_zlib(data), old::inflate_zlib(data));
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
