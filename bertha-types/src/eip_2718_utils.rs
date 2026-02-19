// Copyright 2026 Sonic Operations Ltd
// This file is part of the Sonic Client
//
// Sonic is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Sonic is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Sonic. If not, see <http://www.gnu.org/licenses/>.

use alloy_rlp::Encodable;
use alloy_trie::{HashBuilder, Nibbles};

use crate::{Hash, VerificationError};

/// A trait for types that can be marshalled according to EIP-2718 specification.
pub trait Eip2718Marshallable {
    /// Marshall the object according to EIP-2718 specification
    /// According to the EIP: if the object refers to a LegacyTransaction, it returns the RLP
    /// encoding of the object, otherwise it returns the concatenation of the type byte and the
    /// RLP encoding of the object.
    fn marshal(&self) -> Vec<u8>;
}

/// A trait for types that can be unmarshaled from EIP-2718 specification.
pub trait EIP2718Unmarshallable: Sized {
    /// Unmarshal an object encoded according to EIP-2718 specification.
    fn unmarshal(data: &mut &[u8]) -> Result<Self, alloy_rlp::Error>;
}

/// Verifies a set of EIP-2718 marshallable data against the expected root hash.
pub fn verify<T: Eip2718Marshallable>(
    data: &[T],
    expected_root: &Hash,
) -> Result<(), VerificationError> {
    if compute_root_hash(data) != *expected_root {
        return Err(VerificationError::TransactionVerificationError);
    }
    Ok(())
}

/// Computes the root hash of a set of EIP-2718 marshallable data.
pub fn compute_root_hash<T: Eip2718Marshallable>(data: &[T]) -> Hash {
    let encode_key = |index: usize| -> Vec<u8> {
        let mut v = Vec::new();
        index.encode(&mut v);
        v
    };
    let mut trie = HashBuilder::default();
    let mut leaves: Vec<_> = data
        .iter()
        .enumerate()
        .map(|(i, r)| (Nibbles::unpack(encode_key(i)), r.marshal()))
        .collect();
    leaves.sort();
    leaves.into_iter().for_each(|l| trie.add_leaf(l.0, &l.1));

    trie.root().into()
}
