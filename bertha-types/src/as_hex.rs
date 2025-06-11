use crate::HexConvert;

/// A utility wrapper type for serializing and deserializing types as hex strings using serde.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AsHex<T: HexConvert>(pub T);

impl<T: HexConvert> serde::Serialize for AsHex<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_hex())
    }
}

impl<'de, T: HexConvert> serde::Deserialize<'de> for AsHex<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let hex_str: &str = serde::Deserialize::deserialize(deserializer)?;
        T::try_from_hex(hex_str)
            .map(AsHex)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
    struct Foo {
        pub a: AsHex<[u8; 4]>,
        pub b: AsHex<u64>,
        pub c: AsHex<Vec<u8>>,
    }

    #[test]
    fn fields_are_serialized_and_deserialized_as_hex() {
        let a = [0x12, 0x34, 0x56, 0x78];
        let b = u64::MAX / 2;
        let c = vec![1, 2, 3];
        let foo = Foo {
            a: AsHex(a),
            b: AsHex(b),
            c: AsHex(c.clone()),
        };

        let json = serde_json::to_string(&foo).unwrap();
        assert_eq!(
            json,
            format!(
                r#"{{"a":"{}","b":"{}","c":"{}"}}"#,
                a.to_hex(),
                b.to_hex(),
                c.to_hex()
            )
        );

        let bar = serde_json::from_str::<Foo>(&json).unwrap();
        assert_eq!(foo, bar);
    }
}
