use anyhow::anyhow;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct Bitfield {
    bytes: Vec<u8>,
    n_bits: usize,
}

impl Bitfield {
    pub fn new(n_bits: usize) -> Self {
        let len = (n_bits + 8 - 1) / 8;
        Self {
            bytes: vec![0u8; len],
            n_bits,
        }
    }

    pub fn from_payload(data: Vec<u8>) -> Self {
        Self {
            bytes: data,
            n_bits: 0,
        }
    }

    pub(crate) fn set_piece(&mut self, piece_i: usize) -> anyhow::Result<()> {
        let byte_i = piece_i / 8;
        let bit_i = piece_i % 8;
        if byte_i >= self.bytes.len() {
            return Err(anyhow!("piece's byte index is out of range"));
        }
        self.bytes[byte_i] |= 0b1000_0000 >> bit_i;
        Ok(())
    }

    pub(crate) fn has_piece(&self, piece_i: usize) -> bool {
        // 2 = 20 / 8 (2 is third byte)
        let byte_i = piece_i / 8;
        // bit's index from high bit to low
        // 4 = 20 % 8 (4 is 5th bit from left in byte)
        let bit_i = piece_i % 8;
        let Some(byte) = self.bytes.get(byte_i) else {
            return false;
        };
        byte & 0b1000_0000 >> bit_i != 0
    }

    pub(crate) fn pieces(&self) -> impl Iterator<Item = usize> {
        // iterates bytes
        self.bytes.iter().enumerate().flat_map(|(byte_i, byte)| {
            // iterates bits
            // bytes = [0b10101010, 0b01110110]
            // byte_i = 1, byte = 0b01110110
            (0..8).filter_map(move |bit_i| {
                // 14 = 1 * 8 + 6
                let piece_i = byte_i * 8 + bit_i;
                // 0b0000_0010 = b1000_0000 >> 6
                let mask = 0b1000_0000 >> bit_i;
                (byte & mask != 0).then_some(piece_i)
            })
        })
    }

    pub(crate) fn no_pieces(&self) -> impl Iterator<Item = usize> {
        self.bytes.iter().enumerate().flat_map(move |(byte_i, byte)| {
            (0..8).filter_map(move |bit_i| {
                let piece_i = byte_i * 8 + bit_i;
                if piece_i >= self.n_bits {
                    return None;
                }
                let mask = 0b1000_0000 >> bit_i;
                (byte & mask == 0).then_some(piece_i)
            })
        })
    }
}

#[test]
fn bitfield_set_piece() {
    let mut bf = Bitfield::new(35);
    bf.set_piece(35).unwrap();
    assert!(bf.has_piece(35));
}

#[test]
fn bitfield_has_piece() {
    let bf = Bitfield::from_payload(vec![0b10101010, 0b01110110]);
    assert!(bf.has_piece(0));
    assert!(!bf.has_piece(1));
    assert!(!bf.has_piece(7));
    assert!(!bf.has_piece(8));
    assert!(bf.has_piece(14));
}

#[test]
fn bitfield_pieces() {
    let bf = Bitfield::from_payload(vec![0b10101010, 0b01110110]);
    let mut pieces = bf.pieces();
    assert_eq!(pieces.next(), Some(0)); // 0 bit
    assert_eq!(pieces.next(), Some(2));
    assert_eq!(pieces.next(), Some(4));
    assert_eq!(pieces.next(), Some(6));

    assert_eq!(pieces.next(), Some(9));
    assert_eq!(pieces.next(), Some(10));
    assert_eq!(pieces.next(), Some(11));
    assert_eq!(pieces.next(), Some(13));
    assert_eq!(pieces.next(), Some(14));

    assert_eq!(pieces.next(), None);
}

#[test]
fn bitfield_no_pieces() {
    let bf = Bitfield::new(3);
    let mut no_pieces = bf.no_pieces();
    assert_eq!(no_pieces.next(), Some(0));
    assert_eq!(no_pieces.next(), Some(1));
    assert_eq!(no_pieces.next(), Some(2));
    assert_eq!(no_pieces.next(), None);
}