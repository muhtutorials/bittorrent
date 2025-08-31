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

    pub fn from_vec(data: Vec<u8>) -> Self {
        Self {
            bytes: data,
            n_bits: 0,
        }
    }

    pub(crate) fn set(&mut self, index: usize) -> anyhow::Result<()> {
        if index >= self.n_bits {
            return Err(anyhow!("bit index is out of range"));
        }
        let byte_i = index / 8;
        let bit_i = index % 8;
        self.bytes[byte_i] |= 0b1000_0000 >> bit_i;
        Ok(())
    }

    pub(crate) fn has(&self, index: usize) -> bool {
        // 2 = 20 / 8 (2 is third byte)
        let byte_i = index / 8;
        // bit's index from high bit to low
        // 4 = 20 % 8 (4 is 5th bit from left in byte)
        let bit_i = index % 8;
        let Some(byte) = self.bytes.get(byte_i) else {
            return false;
        };
        byte & 0b1000_0000 >> bit_i != 0
    }

    pub(crate) fn set_bits(&self) -> impl Iterator<Item = usize> {
        // iterates bytes
        self.bytes.iter().enumerate().flat_map(|(byte_i, byte)| {
            // iterates bits
            // bytes = [0b10101010, 0b01110110]
            // byte_i = 1, byte = 0b01110110
            (0..8).filter_map(move |bit_i| {
                // 14 = 1 * 8 + 6
                let index = byte_i * 8 + bit_i;
                // 0b0000_0010 = b1000_0000 >> 6
                let mask = 0b1000_0000 >> bit_i;
                (byte & mask != 0).then_some(index)
            })
        })
    }

    pub(crate) fn unset_bits(&self) -> impl Iterator<Item = usize> {
        self.bytes.iter().enumerate().flat_map(move |(byte_i, byte)| {
            (0..8).filter_map(move |bit_i| {
                let index = byte_i * 8 + bit_i;
                if index >= self.n_bits {
                    return None;
                }
                let mask = 0b1000_0000 >> bit_i;
                (byte & mask == 0).then_some(index)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitfield_set() {
        let mut bf = Bitfield::new(35);
        bf.set(34).unwrap();
        assert!(bf.has(34));
    }

    #[test]
    fn bitfield_has() {
        let bf = Bitfield::from_vec(vec![0b10101010, 0b01110110]);
        assert!(bf.has(0));
        assert!(!bf.has(1));
        assert!(!bf.has(7));
        assert!(!bf.has(8));
        assert!(bf.has(14));
    }

    #[test]
    fn bitfield_set_bits() {
        let bf = Bitfield::from_vec(vec![0b10101010, 0b01110110]);
        let mut set_bits = bf.set_bits();
        assert_eq!(set_bits.next(), Some(0)); // 0 bit
        assert_eq!(set_bits.next(), Some(2));
        assert_eq!(set_bits.next(), Some(4));
        assert_eq!(set_bits.next(), Some(6));

        assert_eq!(set_bits.next(), Some(9));
        assert_eq!(set_bits.next(), Some(10));
        assert_eq!(set_bits.next(), Some(11));
        assert_eq!(set_bits.next(), Some(13));
        assert_eq!(set_bits.next(), Some(14));

        assert_eq!(set_bits.next(), None);
    }

    #[test]
    fn bitfield_unset_bits() {
        let bf = Bitfield::new(3);
        let mut unset_bits = bf.unset_bits();
        assert_eq!(unset_bits.next(), Some(0));
        assert_eq!(unset_bits.next(), Some(1));
        assert_eq!(unset_bits.next(), Some(2));
        assert_eq!(unset_bits.next(), None);
    }
}
