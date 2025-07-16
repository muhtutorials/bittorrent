use std::slice;

pub fn percent_decode(input: &[u8]) -> PercentDecoder<'_> {
    PercentDecoder {
        bytes: input.iter(),
    }
}

pub struct PercentDecoder<'a> {
    bytes: slice::Iter<'a, u8>,
}

impl<'a> Iterator for PercentDecoder<'a> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.bytes.next().map(|&byte| {
            if byte == b'%' {
                parse_hex(&mut self.bytes).unwrap_or(byte)
            } else {
                byte
            }
        })
    }
}

fn parse_hex(iter: &mut slice::Iter<'_, u8>) -> Option<u8> {
    let mut cloned_iter = iter.clone();
    let a = char::from(*cloned_iter.next()?).to_digit(16)? as u8;
    let b = char::from(*cloned_iter.next()?).to_digit(16)? as u8;
    *iter = cloned_iter;
    Some(a << 4 | b) // or Some(a * 0x10 + b) where 0x10 = 0b1111
}
