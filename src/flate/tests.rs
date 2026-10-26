use super::*;

/// Writes bits least significant first, as Deflate packs them (RFC 1951 3.1.1).
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    used: u32,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        (0..count).for_each(|i| {
            if self.used == 0 {
                self.bytes.push(0);
            }
            let bit = ((value >> i) & 1) as u8;
            *self.bytes.last_mut().unwrap() |= bit << self.used;
            self.used = (self.used + 1) % 8;
        });
    }

    /// A Huffman code, which is packed most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        (0..len).rev().for_each(|i| self.put((code >> i) & 1, 1));
    }

    fn align(&mut self) {
        self.used = 0;
    }

    fn raw(&mut self, data: &[u8]) {
        self.align();
        self.bytes.extend_from_slice(data);
    }
}

/// The fixed literal/length code of a symbol (RFC 1951 3.2.6).
fn fixed_lit(bits: &mut Bits, sym: u32) {
    match sym {
        0..=143 => bits.code(0x30 + sym, 8),
        144..=255 => bits.code(0x190 + sym - 144, 9),
        256..=279 => bits.code(sym - 256, 7),
        _ => bits.code(0xc0 + sym - 280, 8),
    }
}

fn naive_adler32(data: &[u8]) -> u32 {
    let (a, b) = data.iter().fold((1u32, 0u32), |(a, b), &c| {
        let a = (a + u32::from(c)) % 65521;
        (a, (b + a) % 65521)
    });
    (b << 16) | a
}

fn zlib_wrap(bits: Bits, output: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    z.extend(bits.bytes);
    z.extend(naive_adler32(output).to_be_bytes());
    z
}

fn lcg(n: usize, seed: u32) -> Vec<u8> {
    std::iter::successors(Some(seed), |x| {
        Some(x.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff)
    })
    .skip(1)
    .take(n)
    .map(|x| (x >> 16) as u8)
    .collect()
}

#[test]
fn adler32_of_a_known_string() {
    assert_eq!(adler32(b"123456789"), 0x091E_01DE);
    assert_eq!(adler32(b""), 1);
}

#[test]
fn inflates_the_vectors_of_zlib() {
    let cases: [(&[u8], &[u8]); 5] = [
        (
            include_bytes!("../../tests/fixtures/flate/stored.zz"),
            include_bytes!("../../tests/fixtures/flate/stored.raw"),
        ),
        (
            include_bytes!("../../tests/fixtures/flate/fixed.zz"),
            include_bytes!("../../tests/fixtures/flate/fixed.raw"),
        ),
        (
            include_bytes!("../../tests/fixtures/flate/dynamic.zz"),
            include_bytes!("../../tests/fixtures/flate/dynamic.raw"),
        ),
        (
            include_bytes!("../../tests/fixtures/flate/empty.zz"),
            include_bytes!("../../tests/fixtures/flate/empty.raw"),
        ),
        (
            include_bytes!("../../tests/fixtures/flate/long.zz"),
            include_bytes!("../../tests/fixtures/flate/long.raw"),
        ),
    ];
    cases
        .iter()
        .for_each(|(z, raw)| assert_eq!(inflate_zlib(z).unwrap(), *raw));
}

#[test]
fn length_3_and_distance_1() {
    let mut bits = Bits::default();
    bits.put(1, 1); // BFINAL
    bits.put(1, 2); // BTYPE fixed
    fixed_lit(&mut bits, u32::from(b'x'));
    fixed_lit(&mut bits, 257); // length 3
    bits.code(0, 5); // distance 1
    fixed_lit(&mut bits, 256);
    let z = zlib_wrap(bits, b"xxxx");
    assert_eq!(inflate_zlib(&z).unwrap(), b"xxxx");
}

#[test]
fn length_258_and_distance_32768() {
    let first = lcg(32768, 11);
    let mut bits = Bits::default();
    bits.put(0, 1);
    bits.put(0, 2); // stored
    bits.align();
    bits.raw(&32768u16.to_le_bytes());
    bits.raw(&(!32768u16).to_le_bytes());
    bits.raw(&first);
    bits.put(1, 1);
    bits.put(1, 2); // fixed
    fixed_lit(&mut bits, 285); // length 258
    bits.code(29, 5); // distances 24577..32768
    bits.put(8191, 13); // distance 32768
    fixed_lit(&mut bits, 256);
    let mut want = first.clone();
    want.extend_from_slice(&first[..258]);
    let z = zlib_wrap(bits, &want);
    assert_eq!(inflate_zlib(&z).unwrap(), want);
}

#[test]
fn a_copy_may_overlap_its_own_output() {
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(1, 2);
    fixed_lit(&mut bits, u32::from(b'a'));
    fixed_lit(&mut bits, u32::from(b'b'));
    fixed_lit(&mut bits, 258); // length 4
    bits.code(1, 5); // distance 2
    fixed_lit(&mut bits, 256);
    let z = zlib_wrap(bits, b"ababab");
    assert_eq!(inflate_zlib(&z).unwrap(), b"ababab");
}

#[test]
fn rejects_a_wrong_adler32() {
    let mut z = include_bytes!("../../tests/fixtures/flate/dynamic.zz").to_vec();
    *z.last_mut().unwrap() ^= 1;
    assert!(inflate_zlib(&z).unwrap_err().message.contains("Adler-32"));
}

#[test]
fn rejects_truncated_data() {
    let z = include_bytes!("../../tests/fixtures/flate/dynamic.zz");
    [z.len() - 2, z.len() / 2, 5, 2, 1, 0]
        .iter()
        .for_each(|&n| assert!(inflate_zlib(&z[..n]).is_err(), "{n}"));
}

#[test]
fn rejects_a_distance_beyond_the_output() {
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(1, 2);
    fixed_lit(&mut bits, u32::from(b'x'));
    fixed_lit(&mut bits, 257);
    bits.code(1, 5); // distance 2, but only 1 byte is out
    fixed_lit(&mut bits, 256);
    let z = zlib_wrap(bits, b"");
    assert!(inflate_zlib(&z).is_err());
}

#[test]
fn rejects_invalid_symbols_and_block_types() {
    // literal/length 286 (fixed code 11000110)
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(1, 2);
    fixed_lit(&mut bits, 286);
    assert!(inflate_zlib(&zlib_wrap(bits, b"")).is_err());
    // distance code 30
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(1, 2);
    fixed_lit(&mut bits, u32::from(b'x'));
    fixed_lit(&mut bits, 257);
    bits.code(30, 5);
    assert!(inflate_zlib(&zlib_wrap(bits, b"")).is_err());
    // block type 3
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(3, 2);
    assert!(inflate_zlib(&zlib_wrap(bits, b"")).is_err());
}

#[test]
fn rejects_a_stored_block_with_a_wrong_nlen() {
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(0, 2);
    bits.raw(&[1, 0, 0, 0, b'x']);
    assert!(inflate_zlib(&zlib_wrap(bits, b"x")).is_err());
}

#[test]
fn rejects_an_over_subscribed_code() {
    // A dynamic block whose code length code gives length 1 to all 19 symbols
    let mut bits = Bits::default();
    bits.put(1, 1);
    bits.put(2, 2);
    bits.put(0, 5); // HLIT 257
    bits.put(0, 5); // HDIST 1
    bits.put(15, 4); // HCLEN 19
    (0..19).for_each(|_| bits.put(1, 3));
    assert!(inflate_zlib(&zlib_wrap(bits, b"")).is_err());
}

#[test]
fn rejects_bad_zlib_headers() {
    let z = include_bytes!("../../tests/fixtures/flate/fixed.zz");
    let mut bad_check = z.to_vec();
    bad_check[1] ^= 1;
    assert!(inflate_zlib(&bad_check).is_err());
    // a preset dictionary (FDICT)
    let fdict = [0x78, 0xBB, 0, 0, 0, 1];
    assert!(inflate_zlib(&fdict).is_err());
    // a method other than 8
    let method = [0x77, 0x85];
    assert!(inflate_zlib(&method).is_err());
}
