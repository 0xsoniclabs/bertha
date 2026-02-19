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

/// The hash of the RLP encoding of the empty list of ommers as a hex string.
/// The Ommers Hash field is now deprecated and is always the constant KEC(RLP(()))
pub const EMPTY_OMMERS_HASH_STR: &str =
    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347";

/// The hash of the RLP encoding of the empty list of ommers.
/// The Ommers Hash field is now deprecated and is always the constant KEC(RLP(()))
pub const EMPTY_OMMERS_HASH: [u8; 32] =
    match const_hex::const_decode_to_array(EMPTY_OMMERS_HASH_STR.as_bytes()) {
        Ok(hash) => hash,
        Err(_) => panic!("failed to parse EMPTY_OMMERS_HASH"),
    };

/// The hash of the root node of an empty Merkle-Patricia Trie as a hex string.
pub const EMPTY_TREE_ROOT_HASH_STR: &str =
    "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421";

/// The hash of the root node of an empty Merkle-Patricia Trie.
pub const EMPTY_TREE_ROOT_HASH: [u8; 32] =
    match const_hex::const_decode_to_array(EMPTY_TREE_ROOT_HASH_STR.as_bytes()) {
        Ok(hash) => hash,
        Err(_) => panic!("failed to parse EMPTY_TREE_ROOT_HASH"),
    };
