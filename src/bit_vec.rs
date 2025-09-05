use anyhow::anyhow;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct BitVec {
    bytes: Vec<u8>,
    n_bits: usize,
}

impl BitVec {
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

    pub(crate) fn unset(&mut self, index: usize) -> anyhow::Result<()> {
        if index >= self.n_bits {
            return Err(anyhow!("bit index is out of range"));
        }
        let byte_i = index / 8;
        let bit_i = index % 8;
        self.bytes[byte_i] &= !(0b1000_0000 >> bit_i);
        Ok(())
    }

    pub(crate) fn toggle(&mut self, index: usize) -> anyhow::Result<()> {
        if index >= self.n_bits {
            return Err(anyhow!("bit index is out of range"));
        }
        let byte_i = index / 8;
        let bit_i = index % 8;
        self.bytes[byte_i] ^= 0b1000_0000 >> bit_i;
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

    pub(crate) fn ones(&self) -> impl Iterator<Item = usize> {
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

    pub(crate) fn zeros(&self) -> impl Iterator<Item = usize> {
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

    pub(crate) fn is_full(&self) -> bool {
        if let None = self.zeros().next() {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_vec_set_toggle_unset() {
        let mut bv = BitVec::new(35);
        bv.set(34).unwrap();
        assert!(bv.has(34));
        bv.toggle(34).unwrap();
        assert!(!bv.has(34));
        bv.toggle(34).unwrap();
        assert!(bv.has(34));
        bv.unset(34).unwrap();
        assert!(!bv.has(34));
        bv.unset(34).unwrap();
        assert!(!bv.has(34));
    }

    #[test]
    fn bit_vec_has() {
        let bv = BitVec::from_vec(vec![0b10101010, 0b01110110]);
        assert!(bv.has(0));
        assert!(!bv.has(1));
        assert!(!bv.has(7));
        assert!(!bv.has(8));
        assert!(bv.has(14));
    }

    #[test]
    fn bit_vec_ones() {
        let bv = BitVec::from_vec(vec![0b10101010, 0b01110110]);
        let mut ones = bv.ones();
        assert_eq!(ones.next(), Some(0)); // 0 bit
        assert_eq!(ones.next(), Some(2));
        assert_eq!(ones.next(), Some(4));
        assert_eq!(ones.next(), Some(6));

        assert_eq!(ones.next(), Some(9));
        assert_eq!(ones.next(), Some(10));
        assert_eq!(ones.next(), Some(11));
        assert_eq!(ones.next(), Some(13));
        assert_eq!(ones.next(), Some(14));

        assert_eq!(ones.next(), None);
    }

    #[test]
    fn bit_vec_zeros() {
        let bv = BitVec::new(3);
        let mut zeros = bv.zeros();
        assert_eq!(zeros.next(), Some(0));
        assert_eq!(zeros.next(), Some(1));
        assert_eq!(zeros.next(), Some(2));
        assert_eq!(zeros.next(), None);
    }
}
