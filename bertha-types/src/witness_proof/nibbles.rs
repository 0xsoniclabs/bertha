use std::{fmt::Display, ops::Index};

/// A half byte (an integer in the range 0..16).
#[derive(PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub struct Nibble(u8);

impl Nibble {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);
    pub const TWO: Self = Self(2);
    pub const THREE: Self = Self(3);
    pub const FOUR: Self = Self(4);
    pub const FIVE: Self = Self(5);
    pub const SIX: Self = Self(6);
    pub const SEVEN: Self = Self(7);
    pub const EIGHT: Self = Self(8);
    pub const NINE: Self = Self(9);
    pub const TEN: Self = Self(10);
    pub const ELEVEN: Self = Self(11);
    pub const TWELVE: Self = Self(12);
    pub const THIRTEEN: Self = Self(13);
    pub const FOURTEEN: Self = Self(14);
    pub const FIFTEEN: Self = Self(15);

    pub fn from_lower_bits(byte: u8) -> Self {
        Self(byte & 0x0f)
    }

    pub fn from_higher_bits(byte: u8) -> Self {
        Self(byte >> 4)
    }

    pub fn as_byte(self) -> u8 {
        self.0
    }
}

impl PartialEq<u8> for Nibble {
    fn eq(&self, other: &u8) -> bool {
        self.0.eq(other)
    }
}

/// Utility type for working with individual nibbles (4-bit words) within a byte string.
/// Each nibble is encoded as a single byte.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct NibbleSequence(Vec<Nibble>);

impl NibbleSequence {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Creates [NibbleSequence] from a byte slice.
    /// The first half byte of the first byte can be ignored to create an odd number of nibbles.
    pub fn from_bytes(mut bytes: &[u8], ignore_first_half_byte: bool) -> NibbleSequence {
        let mut nibbles = Vec::with_capacity(2 * bytes.len());
        if ignore_first_half_byte && !bytes.is_empty() {
            nibbles.push(Nibble::from_lower_bits(bytes[0]));
            bytes = &bytes[1..];
        }
        for b in bytes {
            nibbles.push(Nibble::from_higher_bits(*b));
            nibbles.push(Nibble::from_lower_bits(*b));
        }
        Self(nibbles)
    }

    /// Converts the nibbles back to a sequence of bytes, with each byte containing two nibbles.
    /// If the number of nibbles is odd, the high-nibble of the first byte is set to zero.
    pub fn to_bytes(&self) -> Vec<u8> {
        let num_bytes = self.0.len() / 2 + self.0.len() % 2;
        let mut bytes = vec![0; num_bytes];
        let offset = if self.0.len() % 2 == 0 { 0 } else { 1 };
        for i in 0..self.0.len() {
            let byte_index = (offset + i) / 2;
            let nibble_index = (offset + i) % 2;
            if nibble_index == 0 {
                bytes[byte_index] |= self.0[i].as_byte() << 4;
            } else {
                bytes[byte_index] |= self.0[i].as_byte();
            }
        }
        bytes
    }

    pub fn try_from_hex(value: &str) -> Result<Self, std::num::ParseIntError> {
        let value = value.trim_start_matches("0x");
        let mut bytes = Vec::with_capacity(value.len());
        for i in 0..value.len() {
            let nibble = u8::from_str_radix(&value[i..i + 1], 16)?;
            bytes.push(Nibble::from_lower_bits(nibble));
        }
        Ok(Self(bytes))
    }
}

impl Index<usize> for NibbleSequence {
    type Output = Nibble;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}

impl<'a> IntoIterator for &'a NibbleSequence {
    type Item = &'a Nibble;
    type IntoIter = std::slice::Iter<'a, Nibble>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Display for NibbleSequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x")?;
        for nibble in &self.0 {
            write!(f, "{:x}", nibble.0)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nibble_can_be_constructed_from_lower_bits() {
        for i in 0..16 {
            let n = Nibble::from_lower_bits(i);
            assert_eq!(n, Nibble(i));
        }
        for i in 16..=255 {
            let n = Nibble::from_lower_bits(i);
            assert_eq!(n, Nibble(i & 0x0f));
        }
    }

    #[test]
    fn nibble_can_be_constructed_from_higher_bits() {
        for i in 0..16 {
            let n = Nibble::from_higher_bits(i << 4);
            assert_eq!(n, Nibble(i));
        }
        for i in 0..16 {
            for j in 1..16 {
                let n = Nibble::from_higher_bits((i << 4) + j);
                assert_eq!(n, Nibble(i));
            }
        }
    }

    #[test]
    fn nibble_as_byte_returns_inner_value() {
        for i in 0..16 {
            assert_eq!(Nibble(i).as_byte(), i);
        }
    }

    #[test]
    fn nibble_can_be_compared_with_u8() {
        for i in 0..16 {
            assert_eq!(Nibble(i), i);
        }
        assert_ne!(Nibble(0), 1);
    }

    #[test]
    fn can_be_constructed_from_hex_string() {
        let n = NibbleSequence::try_from_hex("0x").unwrap();
        assert_eq!(n.len(), 0);

        let n = NibbleSequence::try_from_hex("0x1").unwrap();
        assert_eq!(n[0], 0x01);

        let n = NibbleSequence::try_from_hex("0x1a").unwrap();
        assert_eq!(n[0], 0x01);
        assert_eq!(n[1], 0x0a);

        let n = NibbleSequence::try_from_hex("0x1a2").unwrap();
        assert_eq!(n[0], 0x01);
        assert_eq!(n[1], 0x0a);
        assert_eq!(n[2], 0x02);

        // 0x prefix is optional
        let n = NibbleSequence::try_from_hex("1a2b").unwrap();
        assert_eq!(n[0], 0x01);
        assert_eq!(n[1], 0x0a);
        assert_eq!(n[2], 0x02);
        assert_eq!(n[3], 0x0b);
    }

    #[test]
    fn malformed_hex_string_produces_error() {
        let err = NibbleSequence::try_from_hex("xyz");
        assert!(err.is_err());
        assert_eq!(
            *err.unwrap_err().kind(),
            std::num::IntErrorKind::InvalidDigit
        );
    }

    #[test]
    fn can_be_constructed_from_bytes() {
        // even number of nibbles
        let n = NibbleSequence::from_bytes(&[0x1a, 0x2b, 0x3c], false);
        assert_eq!(n[0], 0x1);
        assert_eq!(n[1], 0xa);
        assert_eq!(n[2], 0x2);
        assert_eq!(n[3], 0xb);
        assert_eq!(n[4], 0x3);
        assert_eq!(n[5], 0xc);

        assert_eq!(n, NibbleSequence::try_from_hex("0x1a2b3c").unwrap());

        // odd number of nibbles (skip first half byte)
        let n = NibbleSequence::from_bytes(&[0x01, 0xa2, 0xb3], true);
        assert_eq!(n[0], 0x1);
        assert_eq!(n[1], 0xa);
        assert_eq!(n[2], 0x2);
        assert_eq!(n[3], 0xb);
        assert_eq!(n[4], 0x3);

        assert_eq!(n, NibbleSequence::try_from_hex("0x1a2b3").unwrap());

        // empty byte slice
        let n = NibbleSequence::from_bytes(&[], false);
        assert_eq!(n.len(), 0);

        // empty byte slice (skip first half byte)
        let n = NibbleSequence::from_bytes(&[], true);
        assert_eq!(n.len(), 0);
    }

    #[test]
    fn len_returns_number_of_nibbles() {
        assert_eq!(NibbleSequence::try_from_hex("").unwrap().len(), 0);
        assert_eq!(NibbleSequence::try_from_hex("0x1").unwrap().len(), 1);
        assert_eq!(NibbleSequence::try_from_hex("0x1a").unwrap().len(), 2);
        assert_eq!(NibbleSequence::try_from_hex("0x1a2").unwrap().len(), 3);
        assert_eq!(NibbleSequence::try_from_hex("0x1a2b").unwrap().len(), 4);
    }

    #[test]
    fn can_be_converted_to_bytes() {
        assert_eq!(
            NibbleSequence::try_from_hex("0x").unwrap().to_bytes(),
            vec![0u8; 0]
        );
        assert_eq!(
            NibbleSequence::try_from_hex("0x12").unwrap().to_bytes(),
            vec![0x12]
        );
        assert_eq!(
            NibbleSequence::try_from_hex("0x123").unwrap().to_bytes(),
            vec![0x01, 0x23]
        );
        assert_eq!(
            NibbleSequence::try_from_hex("0x1234").unwrap().to_bytes(),
            vec![0x12, 0x34]
        );
    }

    #[test]
    fn can_be_accessed_by_index() {
        let n = NibbleSequence::try_from_hex("0x123").unwrap();
        assert_eq!(n[0], 0x1);
        assert_eq!(n[1], 0x2);
        assert_eq!(n[2], 0x3);

        // Out of bounds access
        let result = std::panic::catch_unwind(|| n[42]);
        assert!(result.is_err());
    }

    #[test]
    fn is_iterable() {
        let n = NibbleSequence::try_from_hex("0x123").unwrap();
        let mut iter = n.into_iter();
        assert_eq!(iter.next(), Some(&Nibble::ONE));
        assert_eq!(iter.next(), Some(&Nibble::TWO));
        assert_eq!(iter.next(), Some(&Nibble::THREE));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn can_be_printed() {
        let n = NibbleSequence::try_from_hex("0x123deadbeef").unwrap();
        assert_eq!(format!("{n}"), "0x123deadbeef");
    }
}
