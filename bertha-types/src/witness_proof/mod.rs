mod known_hashes;
mod nibbles;

use std::marker::PhantomData;

use alloy_rlp::{RlpDecodable, RlpDecodableWrapper, RlpEncodable, RlpEncodableWrapper};
use sha3::Digest;
use thiserror::Error;

use crate::types::{
    Address, Hash, Nonce, Wei,
    serializable_byte_vec::SerializableByteVec,
    u256::U256,
    witness_proof::{
        known_hashes::EMPTY_TREE_ROOT_HASH,
        nibbles::{Nibble, NibbleSequence},
    },
};

/// The state of an account in Ethereum-compatible blockchains, decoded from a [WitnessProof].
///
/// If an account does not exist within the blockchain (that is, it has never been used in a
/// transaction), it has the value returned by `AccountState::default()`.
#[derive(PartialEq, Eq, Debug, RlpEncodable, RlpDecodable)]
struct AccountState {
    nonce: Nonce,
    balance: Wei,
    storage_hash: Hash,
    code_hash: Hash,
}

impl Default for AccountState {
    fn default() -> Self {
        Self {
            nonce: Nonce::default(),
            balance: Wei::default(),
            // Geth returns an all-zero hash for non-existing accounts, whereas Sonic
            // currently returns the empty tree root hash. This will be changed in the future.
            // TODO: Revisit this once Sonic returns the all-zero hash.
            //       https://github.com/0xsoniclabs/carmen/pull/23
            storage_hash: Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            code_hash: Hash::default(),
        }
    }
}

/// Verified storage data for an account in Ethereum-compatible blockchains, decoded from a
/// [WitnessProof].
///
/// TODO: Revisit this type if / once we support querying storage values:
///  - Data is not actually a U256
///  - Consider making this a struct with a named field
#[derive(PartialEq, Eq, Debug, Default, RlpDecodableWrapper, RlpEncodableWrapper)]
struct AccountStorageEntry(U256);

/// A trait that defines the key type used for indexing leaves in the Merkle-Patricia Trie.
pub trait HasKey {
    type Key: AsRef<[u8]>;
}

impl HasKey for AccountStorageEntry {
    type Key = Hash;
}

impl HasKey for AccountState {
    type Key = Address;
}

/// An enum describing the different ways in which the structure of a Merkle-Patricia Trie proof can
/// be invalid.
#[derive(PartialEq, Eq, Debug)]
pub enum InvalidStructureKind {
    /// Child has an unexpected size of > 32 bytes
    UnexpectedSize,
    /// Leaf/extension node has invalid hex-prefix encoding
    InvalidHexPrefixEncoding,
    /// Node with number of items that is neither 2 nor 17
    UnknownNodeType,
    /// Leaf/extension node has too many or few nibbles
    UnexpectedNumberOfNibbles,
    /// Extension node without a child
    MissingChild,
}

#[derive(PartialEq, Eq, Error, Debug)]
pub enum ProofError {
    #[error("root hash mismatch: expected {0}, received {1}")]
    RootHashMismatch(Hash, Hash),

    #[error("node hash mismatch: expected {0}, received {1}")]
    NodeHashMismatch(Hash, Hash),

    #[error("proof has invalid structure")]
    InvalidStructure(InvalidStructureKind),

    #[error("proof does not contain enough nodes to show existence/non-existence")]
    Incomplete,

    #[error("invalid RLP encoding: {0}")]
    InvalidRlpEncoding(#[from] alloy_rlp::Error),
}

/// The child of a node in a Merkle-Patricia Trie.
/// It can either be the empty node, the hash of a child node or directly embedded into the parent.
#[derive(PartialEq, Eq, Debug)]
enum Child<'a> {
    Empty,
    Hash(&'a [u8]),
    Embedded(&'a [u8]),
}

impl<'a> TryFrom<&'a [u8]> for Child<'a> {
    type Error = ProofError;

    fn try_from(value: &'a [u8]) -> Result<Child<'a>, ProofError> {
        match value.len() {
            0 => Ok(Child::Empty),
            1..=31 => Ok(Child::Embedded(value)),
            32 => Ok(Child::Hash(value)),
            n => Err(ProofError::InvalidStructure(
                InvalidStructureKind::UnexpectedSize,
            )),
        }
    }
}

/// A branch node of a Merkle-Patricia Trie.
///
/// A branch node has one child for each possible nibble (16), as well as an additional
/// "terminator" field, which is unused in Ethereum MPTs (because all keys have the same
/// length, paths always terminate at leaf nodes).
#[derive(Debug)]
struct BranchNode<'a> {
    children: [Child<'a>; 17],
}

impl<'a> BranchNode<'a> {
    fn try_new(items: Vec<&'a [u8]>) -> Result<Self, ProofError> {
        let children: [Child; 17] = items
            .into_iter()
            .map(Child::try_from)
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            // If try_into fails the number of children is not 17 and this is not a branch.
            .map_err(|_| ProofError::InvalidStructure(InvalidStructureKind::UnknownNodeType))?;
        Ok(Self { children })
    }

    fn child(&self, nibble: Nibble) -> &Child<'a> {
        &self.children[nibble.as_byte() as usize]
    }
}

/// An extension node of a Merkle-Patricia Trie.
///
/// As a space-saving optimization, extension nodes replace one or more branch nodes that have only
/// a single child.
#[derive(Debug)]
struct ExtensionNode<'a> {
    path: NibbleSequence,
    child: Child<'a>,
}

impl<'a> ExtensionNode<'a> {
    pub const EVEN_NIBBLES_FLAG: u8 = 0x00;
    pub const ODD_NIBBLES_FLAG: u8 = 0x10;

    fn try_new(path: NibbleSequence, item: &'a [u8]) -> Result<Self, ProofError> {
        let child = Child::try_from(item)?;
        Ok(Self { path, child })
    }
}

/// A leaf node of a Merkle-Patricia Trie.
///
/// The leaf stores the remainder of the key nibbles as well as the data associated with the key.
/// The type of data depends on the kind of tree (account, storage, etc.).
#[derive(Debug)]
struct LeafNode<'a> {
    suffix: NibbleSequence,
    data: &'a [u8],
}

impl<'a> LeafNode<'a> {
    pub const EVEN_NIBBLES_FLAG: u8 = 0x20;
    pub const ODD_NIBBLES_FLAG: u8 = 0x30;

    fn new(suffix: NibbleSequence, data: &'a [u8]) -> Self {
        Self { suffix, data }
    }
}

/// A node of a Merkle-Patricia Trie that is part of a witness proof.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum ProofNode<'a> {
    Empty,
    Branch(BranchNode<'a>),
    Extension(ExtensionNode<'a>),
    Leaf(LeafNode<'a>),
}

/// Decodes an RLP-encoded byte string, unless it is an embedded node.
/// In the latter case, the input slice is simply returned without consuming the RLP header.
/// This way, the embedded node can be stored and decoded in the future.
fn decode_bytes_if_not_embedded<'a>(rlp: &mut &'a [u8]) -> Result<&'a [u8], ProofError> {
    // If the RLP value is a list, it must be an embedded node.
    if rlp[0] >= alloy_rlp::EMPTY_LIST_CODE {
        return Ok(rlp);
    }
    alloy_rlp::Header::decode_bytes(rlp, false).map_err(ProofError::InvalidRlpEncoding)
}

/// Decodes a [ProofNode] from an RLP-encoded byte string.
///
/// The type of node is determined by the number of items, and in the case of leaf/extension
/// nodes, an additional flag bit encoded in the first byte of the first item.
///
/// NOTE: This could also be implemented via the alloy_rlp::Decodable trait on ProofNode,
/// however in that case we could not produce [ProofError]s for invalid structures.
fn decode_node<'a>(raw: &mut &'a [u8]) -> Result<ProofNode<'a>, ProofError> {
    let mut items =
        match alloy_rlp::Header::decode_raw(raw).map_err(ProofError::InvalidRlpEncoding)? {
            alloy_rlp::PayloadView::String(..) => return Ok(ProofNode::Empty),
            alloy_rlp::PayloadView::List(items) => items,
        };

    match items.len() {
        2 => {
            // We first have to decode the key, which uses the "Hex-Prefix Encoding" to
            // indicate whether the number of nibbles is even or odd, as well as an additional flag.
            // See Ethereum Yellowpaper Appendix C.
            let prefix_encoded_key = alloy_rlp::Header::decode_bytes(&mut items[0], false)
                .map_err(ProofError::InvalidRlpEncoding)?;
            let flag = prefix_encoded_key[0] & 0xf0;

            let nibbles = match flag {
                ExtensionNode::EVEN_NIBBLES_FLAG | LeafNode::EVEN_NIBBLES_FLAG => {
                    // The number of nibbles is even, we can discard the flag byte.
                    Ok(NibbleSequence::from_bytes(&prefix_encoded_key[1..], false))
                }
                ExtensionNode::ODD_NIBBLES_FLAG | LeafNode::ODD_NIBBLES_FLAG => {
                    // Discard the high-nibble of the first byte, combine the remainder.
                    Ok(NibbleSequence::from_bytes(prefix_encoded_key, true))
                }
                _ => Err(ProofError::InvalidStructure(
                    InvalidStructureKind::InvalidHexPrefixEncoding,
                )),
            }?;

            let payload = decode_bytes_if_not_embedded(&mut items[1])?;

            if flag == ExtensionNode::EVEN_NIBBLES_FLAG || flag == ExtensionNode::ODD_NIBBLES_FLAG {
                Ok(ProofNode::Extension(ExtensionNode::try_new(
                    nibbles, payload,
                )?))
            } else {
                Ok(ProofNode::Leaf(LeafNode::new(nibbles, payload)))
            }
        }
        17 => {
            let bytes = items
                .iter_mut()
                .map(decode_bytes_if_not_embedded)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ProofNode::Branch(BranchNode::try_new(bytes)?))
        }
        _ => Err(ProofError::InvalidStructure(
            InvalidStructureKind::UnknownNodeType,
        )),
    }
}

/// A witness proof is used to verify the existence or non-existence of a key in a Merkle-Patricia
/// Trie for an expected root hash (for example from
/// [BlockHeader::state_root](crate::types::BlockHeader::state_root)).
#[derive(Debug)]
pub struct WitnessProof<LeafType> {
    encoded_proof: Vec<SerializableByteVec>,
    phantom: PhantomData<LeafType>,
}

impl<LeafType> WitnessProof<LeafType>
where
    LeafType: HasKey + Default + alloy_rlp::Decodable,
{
    /// Verifies that the provided proof is valid for the given root hash and key.
    ///
    /// This operation consumes the proof, returning the leaf data encoded therein.
    /// In case the proof demonstrates the non-existence of a key, the default value of the leaf
    /// type is returned.
    ///
    /// A proof is verified by checking that it either contains a complete path from the root node
    /// to the desired leaf, with each node having the expected hash (as encoded in the parent
    /// node), or that the path terminates at an empty node, inside an extension node or at a
    /// different leaf. In the second case, non-existence is demonstrated.
    ///
    /// If the proof is invalid or incomplete, an error is returned.
    ///
    /// Importantly, this function does not verify that the tree structure encoded by the proof is a
    /// valid branch of a Merkle-Patricia Trie. For example, it does not check that extension nodes
    /// are always followed by a leaf node, or that branch nodes have at least two non-empty
    /// children. The reason for this is that if a proof has the correct root hash as stored in
    /// a block within the blockchain, it is already implicitly guaranteed to also have a valid
    /// structure.
    pub fn verify(self, root_hash: &Hash, key: &LeafType::Key) -> Result<LeafType, ProofError> {
        let key_nibbles = {
            let mut hasher = sha3::Keccak256::new();
            hasher.update(key.as_ref());
            NibbleSequence::from_bytes(hasher.finalize().as_ref(), false)
        };
        let mut keys_iter = key_nibbles.into_iter().peekable();

        // We have to jump through some hoops to be able to iterate over both the encoded proof as
        // well as any embedded nodes that may be contained therein.
        if self.encoded_proof.is_empty() {
            return Err(ProofError::Incomplete);
        };

        let mut next_raw_node = self.encoded_proof[0].as_bytes();

        let mut expected_hash = root_hash.clone();
        let mut i = 0;
        loop {
            let node_hash = hash_bytes(next_raw_node);

            if node_hash != expected_hash {
                if i == 0 {
                    return Err(ProofError::RootHashMismatch(root_hash.clone(), node_hash));
                } else {
                    return Err(ProofError::NodeHashMismatch(
                        expected_hash.clone(),
                        node_hash,
                    ));
                }
            }

            let node = decode_node(&mut next_raw_node)?;

            match node {
                ProofNode::Empty => {
                    // We have demonstrated non-existence.
                    return Ok(LeafType::default());
                }
                ProofNode::Branch(branch) => {
                    // Get the next child along the path we're interested in.
                    let child = branch.child(*keys_iter.next().unwrap());
                    match child {
                        Child::Empty => {
                            // We have demonstrated non-existence.
                            return Ok(LeafType::default());
                        }
                        Child::Hash(hash) => {
                            let mut bytes = [0; 32];
                            bytes.copy_from_slice(hash);
                            expected_hash = Hash::from(bytes);
                            if i + 1 >= self.encoded_proof.len() {
                                return Err(ProofError::Incomplete);
                            }
                            next_raw_node = self.encoded_proof[i + 1].as_bytes();
                        }
                        Child::Embedded(embedded) => {
                            next_raw_node = embedded;
                            expected_hash = hash_bytes(embedded);
                        }
                    }
                }
                ProofNode::Extension(extension) => {
                    for nibble in &extension.path {
                        if let Some(k) = keys_iter.next() {
                            if k != nibble {
                                // There is a mismatch, we have demonstrated non-existence.
                                return Ok(LeafType::default());
                            }
                        } else {
                            // Error: The extension path is longer than the key nibbles.
                            return Err(ProofError::InvalidStructure(
                                InvalidStructureKind::UnexpectedNumberOfNibbles,
                            ));
                        }
                    }
                    if keys_iter.peek().is_none() {
                        // Error: The extension path fully consumed the key nibbles.
                        return Err(ProofError::InvalidStructure(
                            InvalidStructureKind::UnexpectedNumberOfNibbles,
                        ));
                    }

                    match extension.child {
                        Child::Empty => Err(ProofError::InvalidStructure(
                            InvalidStructureKind::MissingChild,
                        ))?,
                        Child::Hash(hash) => {
                            let mut bytes = [0; 32];
                            bytes.copy_from_slice(hash);
                            expected_hash = Hash::from(bytes);
                            if i + 1 >= self.encoded_proof.len() {
                                return Err(ProofError::Incomplete);
                            }
                            next_raw_node = self.encoded_proof[i + 1].as_bytes();
                        }
                        Child::Embedded(embedded) => {
                            next_raw_node = embedded;
                            expected_hash = hash_bytes(embedded);
                        }
                    }
                }
                ProofNode::Leaf(leaf) => {
                    for nibble in &leaf.suffix {
                        if let Some(k) = keys_iter.next() {
                            if k != nibble {
                                // There is a mismatch, we have demonstrated non-existence.
                                return Ok(LeafType::default());
                            }
                        } else {
                            // Error: The leaf suffix is longer than the key nibbles.
                            return Err(ProofError::InvalidStructure(
                                InvalidStructureKind::UnexpectedNumberOfNibbles,
                            ));
                        }
                    }
                    if keys_iter.next().is_some() {
                        // Error: The leaf suffix did not fully consume the key nibbles.
                        return Err(ProofError::InvalidStructure(
                            InvalidStructureKind::UnexpectedNumberOfNibbles,
                        ));
                    }
                    return alloy_rlp::decode_exact::<LeafType>(leaf.data)
                        .map_err(ProofError::InvalidRlpEncoding);
                }
            }

            i += 1;
        }
    }
}

impl<LeafType> From<Vec<SerializableByteVec>> for WitnessProof<LeafType> {
    fn from(value: Vec<SerializableByteVec>) -> Self {
        Self {
            encoded_proof: value,
            phantom: PhantomData,
        }
    }
}

fn hash_bytes(bytes: &[u8]) -> Hash {
    let mut hasher = sha3::Keccak256::new();
    hasher.update(bytes);
    let bytes: [u8; 32] = hasher.finalize().into();
    Hash::from(bytes)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use alloy_rlp::Encodable;

    use super::*;
    use crate::types::AccountProof;

    impl Encodable for BranchNode<'_> {
        fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
            let payload_length = self
                .children
                .iter()
                .map(|child| match child {
                    Child::Empty => [].length(),
                    Child::Hash(h) => h.length(),
                    Child::Embedded(e) => e.len(),
                })
                .sum();
            alloy_rlp::Header {
                list: true,
                payload_length,
            }
            .encode(out);
            for child in &self.children {
                match child {
                    Child::Empty => [].encode(out),
                    Child::Hash(h) => h.encode(out),
                    // The embedded child is already RLP encoded, so we just write it directly.
                    Child::Embedded(e) => out.put_slice(e),
                }
            }
        }
    }

    /// Encodes a branch node with the provided children.
    /// All nibbles that are not present in the `children` map are set to be empty.
    fn encode_branch_node(children: &HashMap<Nibble, &[u8]>) -> SerializableByteVec {
        let mut vec = Vec::<Child>::new();
        for i in 0..16 {
            if let Some(child) = children.get(&Nibble::from_lower_bits(i)) {
                match child.len() {
                    0 => vec.push(Child::Empty),
                    32 => vec.push(Child::Hash(child)),
                    _ => vec.push(Child::Embedded(child)),
                }
            } else {
                vec.push(Child::Empty);
            }
        }
        vec.push(Child::Empty); // Terminator (unused)

        let branch = BranchNode {
            children: vec.try_into().unwrap(),
        };
        SerializableByteVec::from(alloy_rlp::encode(&branch).as_ref())
    }

    /// Encodes a sequence of nibbles using the "Hex-Prefix Encoding", which stores whether the
    /// number of nibbles is even or odd, as well as an additional flag.
    /// See Ethereum Yellowpaper Appendix C.
    fn hex_prefix_encode(nibbles: &NibbleSequence, flag: bool) -> Vec<u8> {
        let mut bytes = nibbles.to_bytes();
        if nibbles.len() % 2 == 0 {
            bytes.insert(0, 0x00);
        } else {
            assert!(bytes[0] & 0xf0 == 0);
            bytes[0] |= 0x10;
        }
        if flag {
            bytes[0] |= 0x20;
        }
        bytes
    }

    impl Encodable for ExtensionNode<'_> {
        fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
            let hp_encoded_path = hex_prefix_encode(&self.path, false);
            let child_len = match self.child {
                Child::Empty => [].length(),
                Child::Hash(h) => h.length(),
                Child::Embedded(e) => e.len(),
            };
            alloy_rlp::Header {
                list: true,
                payload_length: hp_encoded_path.as_slice().length() + child_len,
            }
            .encode(out);
            hp_encoded_path.as_slice().encode(out);
            match self.child {
                Child::Empty => [].encode(out),
                Child::Hash(h) => h.encode(out),
                // The embedded child is already RLP encoded, so we just write it directly.
                Child::Embedded(e) => out.put_slice(e),
            }
        }
    }

    fn encode_extension_node(path: NibbleSequence, child: &[u8]) -> SerializableByteVec {
        let extension = ExtensionNode {
            path,
            child: match child.len() {
                0 => Child::Empty,
                32 => Child::Hash(child),
                _ => Child::Embedded(child),
            },
        };
        SerializableByteVec::from(alloy_rlp::encode(&extension).as_ref())
    }

    impl Encodable for LeafNode<'_> {
        fn encode(&self, out: &mut dyn alloy_rlp::BufMut) {
            let hp_encoded_suffix = hex_prefix_encode(&self.suffix, true);
            alloy_rlp::Header {
                list: true,
                payload_length: hp_encoded_suffix.as_slice().length() + self.data.length(),
            }
            .encode(out);
            hp_encoded_suffix.as_slice().encode(out);
            self.data.encode(out);
        }
    }

    fn encode_leaf_node(suffix: NibbleSequence, data: &[u8]) -> SerializableByteVec {
        let leaf = LeafNode { suffix, data };
        SerializableByteVec::from(alloy_rlp::encode(&leaf).as_ref())
    }

    #[test]
    fn child_try_from_u8_slice_distinguishes_child_types_based_on_size() {
        let empty: &[u8] = &[];
        let embedded: &[u8] = &[0; 31];
        let hash: &[u8] = &[0; 32];
        let invalid: &[u8] = &[0; 33];

        assert_eq!(Child::try_from(empty).unwrap(), Child::Empty);
        assert_eq!(
            Child::try_from(embedded).unwrap(),
            Child::Embedded(embedded)
        );
        assert_eq!(Child::try_from(hash).unwrap(), Child::Hash(hash));
        assert_eq!(
            Child::try_from(invalid).unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::UnexpectedSize)
        );
    }

    #[test]
    fn branch_node_try_new_creates_correct_child_types() {
        let empty: &[u8] = &[];
        let embedded: &[u8] = &[0; 31];
        let hash: &[u8] = &[0; 32];

        let mut items: Vec<&[u8]> = vec![empty; 17];
        items[2] = embedded;
        items[5] = hash;

        let branch = BranchNode::try_new(items).unwrap();
        assert_eq!(*branch.child(Nibble::ZERO), Child::Empty);
        assert_eq!(*branch.child(Nibble::ONE), Child::Empty);
        assert_eq!(*branch.child(Nibble::TWO), Child::Embedded(embedded));
        assert_eq!(*branch.child(Nibble::FIVE), Child::Hash(hash));
    }

    #[test]
    fn branch_node_try_new_returns_error_if_child_has_unexpected_size() {
        let items: Vec<&[u8]> = vec![&[0u8; 33]; 17];
        let err = BranchNode::try_new(items);
        assert_eq!(
            err.unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::UnexpectedSize)
        );
    }

    #[test]
    fn branch_node_try_new_returns_error_if_number_of_children_is_not_17() {
        let items: Vec<&[u8]> = vec![&[], &[], &[]];
        let err = BranchNode::try_new(items);
        assert_eq!(
            err.unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::UnknownNodeType)
        );
    }

    #[test]
    fn extension_node_try_new_creates_correct_child_type() {
        let path = NibbleSequence::try_from_hex("0x1234").unwrap();
        let empty: &[u8] = &[];
        let embedded: &[u8] = &[0; 31];
        let hash: &[u8] = &[0; 32];

        let extension = ExtensionNode::try_new(path.clone(), empty).unwrap();
        assert_eq!(extension.path, path);
        assert_eq!(extension.child, Child::Empty);

        let extension = ExtensionNode::try_new(path.clone(), embedded).unwrap();
        assert_eq!(extension.path, path);
        assert_eq!(extension.child, Child::Embedded(embedded));

        let extension = ExtensionNode::try_new(path.clone(), hash).unwrap();
        assert_eq!(extension.path, path);
        assert_eq!(extension.child, Child::Hash(hash));
    }

    #[test]
    fn extension_node_try_new_returns_error_if_child_has_unexpected_size() {
        let child = &[0u8; 33];
        let err = ExtensionNode::try_new(NibbleSequence::try_from_hex("0x1234").unwrap(), child);
        assert_eq!(
            err.unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::UnexpectedSize)
        );
    }

    #[test]
    fn decode_node_handles_empty_node() {
        let empty = alloy_rlp::encode([]);
        let node = decode_node(&mut empty.as_ref()).unwrap();
        assert!(matches!(node, ProofNode::Empty));
    }

    #[test]
    fn decode_node_handles_branch_node() {
        let hash1 = Hash::try_from_hex(
            "0x0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff",
        )
        .unwrap();
        let hash2 = Hash::try_from_hex(
            "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        )
        .unwrap();
        let embedded = encode_leaf_node(
            NibbleSequence::try_from_hex("0x1234").unwrap(),
            &[0u8, 1u8, 2u8, 3u8],
        );
        let encoded = encode_branch_node(&HashMap::from([
            (Nibble::ZERO, hash1.as_ref()),
            (Nibble::FIVE, embedded.as_bytes()),
            (Nibble::NINE, &[]),
            (Nibble::FIFTEEN, hash2.as_ref()),
        ]));

        let node = decode_node(&mut encoded.as_bytes()).unwrap();
        assert!(matches!(node, ProofNode::Branch(_)));
        if let ProofNode::Branch(branch) = node {
            assert_eq!(*branch.child(Nibble::ZERO), Child::Hash(hash1.as_bytes()));
            assert_eq!(*branch.child(Nibble::ONE), Child::Empty);
            assert_eq!(
                *branch.child(Nibble::FIVE),
                Child::Embedded(embedded.as_bytes())
            );
            assert_eq!(*branch.child(Nibble::NINE), Child::Empty);
            assert_eq!(
                *branch.child(Nibble::FIFTEEN),
                Child::Hash(hash2.as_bytes())
            );
        } else {
            panic!("Expected branch node");
        }
    }

    #[test]
    fn decode_node_handles_extension_node() {
        let run_for_path_and_child = |path: &str, child: &[u8]| {
            let encoded = encode_extension_node(NibbleSequence::try_from_hex(path).unwrap(), child);

            let node = decode_node(&mut encoded.as_bytes()).unwrap();
            assert!(matches!(node, ProofNode::Extension(_)));
            if let ProofNode::Extension(extension) = node {
                assert_eq!(extension.path, NibbleSequence::try_from_hex(path).unwrap());
                assert!(match extension.child {
                    Child::Hash(h) => h == child,
                    Child::Empty => child.is_empty(),
                    Child::Embedded(e) => e == child,
                });
            } else {
                panic!("Expected extension node");
            }
        };

        let hash = Hash::try_from_hex(
            "0x0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff",
        )
        .unwrap();
        let empty = [0u8; 0];
        let embedded = encode_leaf_node(
            NibbleSequence::try_from_hex("0x1234").unwrap(),
            &[0u8, 1u8, 2u8, 3u8],
        );
        let children = [hash.as_ref(), empty.as_slice(), embedded.as_bytes()];

        for c in children {
            run_for_path_and_child("0x", c); // Not a valid path, but we can decode it
            run_for_path_and_child("0x1", c);
            run_for_path_and_child("0x12", c);
            run_for_path_and_child("0x123", c);
            run_for_path_and_child("0x1234", c);
        }
    }

    #[test]
    fn decode_node_handles_leaf_node() {
        let run_for_suffix = |suffix: &str| {
            let data = [0u8, 1u8, 2u8, 3u8];
            let encoded = encode_leaf_node(NibbleSequence::try_from_hex(suffix).unwrap(), &data);

            let node = decode_node(&mut encoded.as_bytes()).unwrap();
            assert!(matches!(node, ProofNode::Leaf(_)));
            if let ProofNode::Leaf(leaf) = node {
                assert_eq!(leaf.suffix, NibbleSequence::try_from_hex(suffix).unwrap());
                assert_eq!(*leaf.data, data);
            } else {
                panic!("Expected leaf node");
            }
        };

        run_for_suffix("0x"); // Not a valid suffix, but we can decode it
        run_for_suffix("0x1");
        run_for_suffix("0x12");
        run_for_suffix("0x123");
        run_for_suffix("0x1234");
    }

    #[test]
    fn decode_node_returns_error_if_node_type_unknown() {
        // Node types are determined by the number of items (either 2 or 17)
        let items = vec![0u32, 0u32, 0u32];
        let raw = [SerializableByteVec::from(
            alloy_rlp::encode(&items).as_slice(),
        )];
        let err = decode_node(&mut raw[0].as_bytes());
        assert_eq!(
            err.unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::UnknownNodeType)
        );
    }

    #[test]
    fn decode_node_returns_error_on_invalid_hex_prefix_encoding() {
        // The high nibble of the first byte must be zero
        let hex_prefix = vec![0xff_u8];
        let node = vec![hex_prefix.as_slice(), &[]];
        let rlp = alloy_rlp::encode(node);
        let err = decode_node(&mut rlp.as_slice());
        assert_eq!(
            err.unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::InvalidHexPrefixEncoding)
        );
    }

    #[test]
    fn decode_node_returns_error_for_branch_node_with_invalid_child() {
        let child = alloy_rlp::encode(vec![&[0; 1]; 33]); // child exceeds 32 bytes
        let encoded = encode_branch_node(&HashMap::from([(Nibble::ZERO, child.as_slice())]));
        let err = decode_node(&mut encoded.as_bytes());
        assert!(
            matches!(err, Err(ProofError::InvalidStructure(e)) if e == InvalidStructureKind::UnexpectedSize)
        );
    }

    #[test]
    fn decode_node_returns_error_for_extension_node_with_invalid_child() {
        let child = alloy_rlp::encode(vec![&[0; 1]; 33]); // child exceeds 32 bytes
        let encoded = encode_extension_node(
            NibbleSequence::try_from_hex("0x123").unwrap(),
            child.as_slice(),
        );
        let err = decode_node(&mut encoded.as_bytes());
        assert!(
            matches!(err, Err(ProofError::InvalidStructure(e)) if e == InvalidStructureKind::UnexpectedSize)
        );
    }

    #[test]
    fn decode_node_forwards_rlp_errors() {
        // Invalid top level encoding
        {
            // 0xb8 marks a string where the size is encoded in the following byte
            let rlp = vec![0xb8_u8];
            let err = decode_node(&mut rlp.as_slice());
            assert!(
                matches!(err, Err(ProofError::InvalidRlpEncoding(e)) if e == alloy_rlp::Error::InputTooShort)
            );
        }

        // Leaf/extension node contains list instead of byte string as first item
        {
            let node = vec![vec![0x80_u8], vec![]];
            let rlp = alloy_rlp::encode(node);
            let err = decode_node(&mut rlp.as_slice());
            assert!(
                matches!(err, Err(ProofError::InvalidRlpEncoding(e)) if e == alloy_rlp::Error::UnexpectedList)
            );
        }
    }

    #[test]
    fn verify_returns_error_on_root_hash_mismatch() {
        let empty = alloy_rlp::encode([]);
        let raw: Vec<SerializableByteVec> = vec![SerializableByteVec::from(empty.as_ref())];
        let acc = WitnessProof::<AccountState>::from(raw.clone()).verify(
            &Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            &Address::default(),
        );
        assert!(acc.is_ok());

        let err =
            WitnessProof::<AccountState>::from(raw).verify(&Hash::default(), &Address::default());
        assert_eq!(
            err.as_ref().unwrap_err(),
            &ProofError::RootHashMismatch(
                Hash::default(),
                Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            )
        );
        assert_eq!(
            err.unwrap_err().to_string(),
            format!(
                "root hash mismatch: expected {}, received {}",
                Hash::default(),
                Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            )
        );
    }

    #[test]
    fn verify_returns_error_on_node_hash_mismatch() {
        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));

        let leaf = encode_leaf_node(
            NibbleSequence::try_from_hex(&address_hash_hex[1..]).unwrap(),
            &alloy_rlp::encode(AccountState::default()),
        );
        let leaf_hash = hash_bytes(leaf.as_bytes());
        let empty_hash = Hash::default();

        // For branch nodes
        // (In practice branch nodes must have >= 2 children, but it doesn't matter in this case)
        {
            let address_hash_nibble =
                NibbleSequence::try_from_hex(&address_hash_hex[0..1]).unwrap()[0];
            let good_branch =
                encode_branch_node(&HashMap::from([(address_hash_nibble, leaf_hash.as_ref())]));
            let bad_branch =
                encode_branch_node(&HashMap::from([(address_hash_nibble, empty_hash.as_ref())]));

            let result =
                WitnessProof::<AccountState>::from(vec![good_branch.clone(), leaf.clone()])
                    .verify(&hash_bytes(good_branch.as_bytes()), &address);
            assert!(result.is_ok());

            let result = WitnessProof::<AccountState>::from(vec![bad_branch.clone(), leaf.clone()])
                .verify(&hash_bytes(bad_branch.as_bytes()), &address);
            assert_eq!(
                result.unwrap_err(),
                ProofError::NodeHashMismatch(empty_hash.clone(), leaf_hash.clone())
            );
        }

        // For extension nodes
        // (Again not quite a valid Trie: extension nodes must be followed by a branch node)
        {
            let good_extension = encode_extension_node(
                NibbleSequence::try_from_hex(&address_hash_hex[0..1]).unwrap(),
                leaf_hash.as_ref(),
            );
            let bad_extension = encode_extension_node(
                NibbleSequence::try_from_hex(&address_hash_hex[0..1]).unwrap(),
                empty_hash.as_ref(),
            );

            let result =
                WitnessProof::<AccountState>::from(vec![good_extension.clone(), leaf.clone()])
                    .verify(&hash_bytes(good_extension.as_bytes()), &address);
            assert!(result.is_ok());

            let result =
                WitnessProof::<AccountState>::from(vec![bad_extension.clone(), leaf.clone()])
                    .verify(&hash_bytes(bad_extension.as_bytes()), &address);
            assert_eq!(
                result.unwrap_err(),
                ProofError::NodeHashMismatch(empty_hash.clone(), leaf_hash.clone())
            );
        }
    }

    #[test]
    fn verify_returns_error_for_extension_node_without_child() {
        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));
        let extension = encode_extension_node(
            NibbleSequence::try_from_hex(&address_hash_hex[0..5]).unwrap(),
            &[],
        );
        let result = WitnessProof::<AccountState>::from(vec![extension.clone()])
            .verify(&hash_bytes(extension.as_bytes()), &Address::default());
        assert_eq!(
            result.unwrap_err(),
            ProofError::InvalidStructure(InvalidStructureKind::MissingChild,)
        );
    }

    #[test]
    fn verify_returns_leaf_node_data_for_valid_proof() {
        let acc_state = AccountState {
            balance: Wei::from(123u64),
            nonce: Nonce::from(456u64),
            code_hash: Hash::try_from_hex(
                "0x0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff",
            )
            .unwrap(),
            storage_hash: Hash::try_from_hex(
                "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            )
            .unwrap(),
        };
        let acc_state_rlp: Vec<u8> = alloy_rlp::encode(&acc_state);

        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));

        // Single leaf node
        {
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(&address_hash_hex).unwrap(),
                &acc_state_rlp,
            );
            let result = WitnessProof::<AccountState>::from(vec![leaf.clone()])
                .verify(&hash_bytes(leaf.as_bytes()), &address);
            assert_eq!(result.unwrap(), acc_state);
        }

        // Extension nodes with different path lengths
        {
            let test_extension_length = |len: usize| {
                assert!(len > 0 && len < address_hash_hex.len());
                let leaf = encode_leaf_node(
                    NibbleSequence::try_from_hex(&address_hash_hex[len..]).unwrap(),
                    &acc_state_rlp,
                );
                let extension = encode_extension_node(
                    NibbleSequence::try_from_hex(&address_hash_hex[0..len]).unwrap(),
                    hash_bytes(leaf.as_bytes()).as_ref(),
                );
                let result =
                    WitnessProof::<AccountState>::from(vec![extension.clone(), leaf.clone()])
                        .verify(&hash_bytes(extension.as_bytes()), &address);
                assert_eq!(result.unwrap(), acc_state);
            };
            test_extension_length(1);
            test_extension_length(2);
            test_extension_length(3);
            test_extension_length(4);
        }

        // Branch node
        {
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(&address_hash_hex[1..]).unwrap(),
                &acc_state_rlp,
            );
            let branch = encode_branch_node(&HashMap::from([(
                NibbleSequence::try_from_hex(&address_hash_hex).unwrap()[0],
                hash_bytes(leaf.as_bytes()).as_ref(),
            )]));
            let result = WitnessProof::<AccountState>::from(vec![branch.clone(), leaf.clone()])
                .verify(&hash_bytes(branch.as_bytes()), &address);
            assert_eq!(result.unwrap(), acc_state);
        }

        // Extension node followed by branch node
        {
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(&address_hash_hex[5..]).unwrap(),
                &acc_state_rlp,
            );
            let branch = encode_branch_node(&HashMap::from([(
                NibbleSequence::try_from_hex(&address_hash_hex[4..5]).unwrap()[0],
                hash_bytes(leaf.as_bytes()).as_ref(),
            )]));
            let extension = encode_extension_node(
                NibbleSequence::try_from_hex(&address_hash_hex[0..4]).unwrap(),
                hash_bytes(branch.as_bytes()).as_ref(),
            );
            let result = WitnessProof::<AccountState>::from(vec![
                extension.clone(),
                branch.clone(),
                leaf.clone(),
            ])
            .verify(&hash_bytes(extension.as_bytes()), &address);
            assert_eq!(result.unwrap(), acc_state);
        }
    }

    #[test]
    fn verify_returns_embedded_leaf_node_data_for_valid_proof() {
        let key = Hash::default();
        let key_hash_hex = const_hex::encode(hash_bytes(key.as_ref()));

        let value = AccountStorageEntry(U256::from(123u64));
        let value_rlp = alloy_rlp::encode(&value);

        // Branch node with single embedded child
        {
            // We have to chain 9 branch nodes together to remove enough nibbles for the leaf node
            // to be small enough to be embedded (< 32 bytes).
            let branch_count = 9;
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(&key_hash_hex[branch_count..]).unwrap(),
                &value_rlp,
            );
            let mut branches = vec![];
            branches.push(encode_branch_node(&HashMap::from([(
                NibbleSequence::try_from_hex(&key_hash_hex[branch_count - 1..branch_count])
                    .unwrap()[0],
                leaf.as_bytes(),
            )])));
            let mut next_hash = hash_bytes(branches[0].as_bytes());
            for i in (0..branch_count - 1).rev() {
                let branch = encode_branch_node(&HashMap::from([(
                    NibbleSequence::try_from_hex(&key_hash_hex[i..i + 1]).unwrap()[0],
                    next_hash.as_ref(),
                )]));
                next_hash = hash_bytes(branch.as_bytes());
                branches.push(branch);
            }

            let proof = branches.into_iter().rev().collect::<Vec<_>>();
            let result = WitnessProof::<AccountStorageEntry>::from(proof.clone())
                .verify(&hash_bytes(proof[0].as_bytes()), &key);
            assert_eq!(result.unwrap(), value);
        }

        // Branch node with two embedded children, itself embedded into extension node
        {
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(&key_hash_hex[63..64]).unwrap(),
                &value_rlp,
            );
            // We can put whatever we want here, as we're not going to visit this node
            let other_leaf = encode_leaf_node(NibbleSequence::try_from_hex("0x1").unwrap(), &[]);
            let leaf_nibble = NibbleSequence::try_from_hex(&key_hash_hex[62..63]).unwrap()[0];
            let branch = encode_branch_node(&HashMap::from([
                (leaf_nibble, leaf.as_bytes()),
                (
                    Nibble::from_lower_bits((leaf_nibble.as_byte() + 1) % 16),
                    other_leaf.as_bytes(),
                ),
            ]));
            let extension = encode_extension_node(
                NibbleSequence::try_from_hex(&key_hash_hex[0..62]).unwrap(),
                branch.as_bytes(),
            );
            let proof = vec![extension.clone()];
            let result = WitnessProof::<AccountStorageEntry>::from(proof.clone())
                .verify(&hash_bytes(extension.as_bytes()), &key);
            assert_eq!(result.unwrap(), value);
        }
    }

    #[test]
    fn verify_returns_default_leaf_data_for_valid_non_existence_proof() {
        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));

        // Empty tree
        {
            let empty = SerializableByteVec::from(alloy_rlp::encode([]).as_ref());
            let result = WitnessProof::<AccountState>::from(vec![empty])
                .verify(&Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(), &address);
            assert_eq!(result.unwrap(), AccountState::default());
        }

        // Leaf with different key
        {
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(
                    "0x0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff",
                )
                .unwrap(),
                &[1, 2, 3],
            );
            let result = WitnessProof::<AccountState>::from(vec![leaf.clone()])
                .verify(&hash_bytes(leaf.as_bytes()), &address);
            assert_eq!(result.unwrap(), AccountState::default());
        }

        // Extension with partially matching path
        {
            let partial_path = format!("{}deadbeef", &address_hash_hex[0..5]);
            let extension =
                encode_extension_node(NibbleSequence::try_from_hex(&partial_path).unwrap(), &[]);
            let result = WitnessProof::<AccountState>::from(vec![extension.clone()])
                .verify(&hash_bytes(extension.as_bytes()), &address);
            assert_eq!(result.unwrap(), AccountState::default());
        }

        // Branch with empty child slot
        {
            let branch = encode_branch_node(&HashMap::from([]));
            let result = WitnessProof::<AccountState>::from(vec![branch.clone()])
                .verify(&hash_bytes(branch.as_bytes()), &address);
            assert_eq!(result.unwrap(), AccountState::default());
        }
    }

    #[test]
    fn verify_returns_default_leaf_data_for_embedded_non_existence_proof() {
        let key = Hash::default();
        let key_hash_hex = const_hex::encode(hash_bytes(key.as_ref()));
        let other_key = Hash::try_from_hex(
            "0x0000111122223333444455556666777788889999aaaabbbbccccddddeeeeffff",
        )
        .unwrap();

        // Not a real trie: A leaf would never be embedded in an extension node (because then
        // the whole thing could have been a single leaf node). It however still exercises the
        // correct control flow.
        let leaf = encode_leaf_node(
            NibbleSequence::try_from_hex(&key_hash_hex[63..64]).unwrap(),
            &[],
        );
        let extension = encode_extension_node(
            NibbleSequence::try_from_hex(&key_hash_hex[0..63]).unwrap(),
            leaf.as_bytes(),
        );
        let proof = vec![extension.clone()];
        let result = WitnessProof::<AccountStorageEntry>::from(proof.clone())
            .verify(&hash_bytes(extension.as_bytes()), &other_key);
        assert_eq!(result.unwrap(), AccountStorageEntry::default());
    }

    #[test]
    fn verify_gracefully_handles_proofs_with_superfluous_nodes_after_embedded_nodes() {
        let key = Hash::default();
        let key_hash_hex = const_hex::encode(hash_bytes(key.as_ref()));

        let value = AccountStorageEntry(U256::from(123u64));
        let value_rlp = alloy_rlp::encode(&value);

        let leaf = encode_leaf_node(
            NibbleSequence::try_from_hex(&key_hash_hex[63..64]).unwrap(),
            value_rlp.as_slice(),
        );
        let superfluous_leaf = encode_leaf_node(NibbleSequence::try_from_hex("0x1").unwrap(), &[]);
        let extension = encode_extension_node(
            NibbleSequence::try_from_hex(&key_hash_hex[0..63]).unwrap(),
            leaf.as_bytes(),
        );
        let proof = vec![extension.clone(), superfluous_leaf.clone()];
        let result = WitnessProof::<AccountStorageEntry>::from(proof.clone())
            .verify(&hash_bytes(extension.as_bytes()), &key);
        // "Gracefully" in this case means that the superfluous leaf is ignored
        // and the correct value is returned.
        assert_eq!(result.unwrap(), value);
    }

    #[test]
    fn verify_returns_error_for_invalid_leaf_node_data() {
        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));
        let leaf = encode_leaf_node(
            NibbleSequence::try_from_hex(&address_hash_hex).unwrap(),
            &[1, 2, 3],
        );
        let result = WitnessProof::<AccountState>::from(vec![leaf.clone()])
            .verify(&hash_bytes(leaf.as_bytes()), &address);
        assert_eq!(
            result.unwrap_err(),
            ProofError::InvalidRlpEncoding(alloy_rlp::Error::UnexpectedString)
        );
    }

    #[test]
    fn verify_returns_error_for_incomplete_proof() {
        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));
        let child_hash = Hash::default();

        // No nodes at all
        {
            let result = WitnessProof::<AccountState>::from(vec![])
                .verify(&Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(), &address);
            assert_eq!(result.unwrap_err(), ProofError::Incomplete);
        }

        // Single extension node
        {
            let extension = encode_extension_node(
                NibbleSequence::try_from_hex(&address_hash_hex[0..5]).unwrap(),
                child_hash.as_ref(),
            );
            let result = WitnessProof::<AccountState>::from(vec![extension.clone()])
                .verify(&hash_bytes(extension.as_bytes()), &address);
            assert_eq!(result.unwrap_err(), ProofError::Incomplete);
        }

        // Single branch node
        {
            let branch = encode_branch_node(&HashMap::from([(
                NibbleSequence::try_from_hex(&address_hash_hex).unwrap()[0],
                child_hash.as_ref(),
            )]));
            let result = WitnessProof::<AccountState>::from(vec![branch.clone()])
                .verify(&hash_bytes(branch.as_bytes()), &address);
            assert_eq!(result.unwrap_err(), ProofError::Incomplete);
        }
    }

    #[test]
    fn verify_ignores_superfluous_data_in_proof() {
        let empty = SerializableByteVec::from(alloy_rlp::encode([]).as_ref());
        let proof = vec![empty.clone(), empty.clone(), empty.clone()];
        let result = WitnessProof::<AccountState>::from(proof).verify(
            &Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            &Address::default(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn verify_returns_error_for_unexpected_key_lengths_in_extension_and_leaf_nodes() {
        let address = Address::default();
        let address_hash_hex = const_hex::encode(hash_bytes(address.as_bytes()));
        let too_long = format!("{address_hash_hex}deadbeef");

        // Extension node with path > 32 bytes
        {
            let extension =
                encode_extension_node(NibbleSequence::try_from_hex(&too_long).unwrap(), &[]);
            let result = WitnessProof::<AccountState>::from(vec![extension.clone()])
                .verify(&hash_bytes(extension.as_bytes()), &address);
            assert_eq!(
                result.unwrap_err(),
                ProofError::InvalidStructure(InvalidStructureKind::UnexpectedNumberOfNibbles)
            );
        }

        // Extension node with path == 32 bytes
        {
            let extension = encode_extension_node(
                NibbleSequence::try_from_hex(&address_hash_hex).unwrap(),
                &[],
            );
            let result = WitnessProof::<AccountState>::from(vec![extension.clone()])
                .verify(&hash_bytes(extension.as_bytes()), &address);
            assert_eq!(
                result.unwrap_err(),
                ProofError::InvalidStructure(InvalidStructureKind::UnexpectedNumberOfNibbles)
            );
        }

        // Leaf node with suffix > 32 bytes
        {
            let leaf = encode_leaf_node(NibbleSequence::try_from_hex(&too_long).unwrap(), &[]);
            let result = WitnessProof::<AccountState>::from(vec![leaf.clone()])
                .verify(&hash_bytes(leaf.as_bytes()), &address);
            assert_eq!(
                result.unwrap_err(),
                ProofError::InvalidStructure(InvalidStructureKind::UnexpectedNumberOfNibbles)
            );
        }

        // Leaf node with short suffix
        {
            let leaf = encode_leaf_node(
                NibbleSequence::try_from_hex(&address_hash_hex[0..5]).unwrap(),
                &[],
            );
            let result = WitnessProof::<AccountState>::from(vec![leaf.clone()])
                .verify(&hash_bytes(leaf.as_bytes()), &address);
            assert_eq!(
                result.unwrap_err(),
                ProofError::InvalidStructure(InvalidStructureKind::UnexpectedNumberOfNibbles)
            );
        }
    }

    #[test]
    fn proof_is_generic_over_leaf_type() {
        let empty = SerializableByteVec::from(alloy_rlp::encode([]).as_ref());

        let result = WitnessProof::<AccountState>::from(vec![empty.clone()]).verify(
            &Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            &Address::default(),
        );
        assert_eq!(result.unwrap(), AccountState::default());

        let result = WitnessProof::<AccountStorageEntry>::from(vec![empty]).verify(
            &Hash::try_from_hex(EMPTY_TREE_ROOT_HASH).unwrap(),
            &Hash::default(),
        );
        assert_eq!(result.unwrap(), AccountStorageEntry::default());
    }

    // =========================================================================
    // The following tests decode data that was either obtained from the
    // real Sonic network or generated with an external tool (Carmen).
    // =========================================================================

    // Basic proof containing only branch nodes.
    // This is for Sonic block 0x30daa7757e6791b35187782d6ce5a904ce9004460a2a7c4ef3a975bb69be1e95
    const BASIC_PROOF: (&str, &str) = (
        "0x4fb543de674c1ea7e1cb536995b0c7dd21bf00ed1142d14be862be1f284d75b9",
        r#"{
            "address":"0x63795e0f9223ec4bfef5fbe3dbf9331f1c57cc5c",
            "accountProof":["0xf90211a016922957725904b8fe55d12ad5c970e6c3e66005956f530459cf4ed8c0ee1e90a0b8c24df9339f30cd1f3c67e0acca6abd4e71c15ecf12e5779489200da05babd2a019ac030fd4dc1c5b6a4cebb955df30a025213b11d08611b5000113720fcf9215a0e9f0c2057e62c4a8e5c26d1b004233a15d5f5dab041b2c80d3b6f25ad71d15a2a0bb4872e89e2218a8bf31afef0406b1ab78e2c0d042eb9a601f696662add0b230a0ae6b0ae45dee8979ccb05ffffcbec9ad18f3431ad832f9f56b854e5526afa6efa0c756565238e3e38afa11bc63d823bcf76e8cd45d6357f6067c99c617bf84779ea0b0247193ce5c1bd926d43a2c23e2739122a967c0d889495494a7b2a61d7b485ca066b8eb433287ab09298ea881d6183c1fe24bbce62b78011a9d96c0143a9b3413a02634b7b0420283384cc737cc4f34c08899983d476ebf29fed113c7e6a7acda3aa0b5a408793b8986d7163c33c5a2103e812b9008b682d8c29466add8c86a21f5e4a0b7fd16c2ea91ecb24d4b00df5d6f58d5477c160179d2853b5628ae93b2089366a02e3671e82b26cfbc5405d003375664293548acb7f59e6cb2293cca599dad08e0a05df63f79280c10d6b845f5f0f44aadc2c821145fec42d2fcd932de09bc4e3c5da0fcf3864c022b93f945be0a7b7d4c3d9518328da6531649ddeb22fd208103d7a3a05faecc56f055e0f3eaa2bec500ebe66f37539a72f4b6dd14761852c75264ecc280","0xf90211a0752dfbedcb4cbebbee9a43626d6749140a9081546c171e19343d31646ffe1aeea08cdbbcd08e2da5b6543cc014ca313ef386bc0472ccb134a0c2327916d702ecf2a0367686f60d58ee5fcabb643a1b4ce0f6f9d9b2e1f3cc4c3c872847c99db125fea0a19f1b2943d4d5a7c48355d6ba9bc2bd9f2a025389221c7d6b33a3767767cddba0964e8b9bec443881c458330a66615f5603a3f23b7488da431ce8fc47485aacb7a0ed47b5cf2fbc18b1222930dff4b63f80dfa3a88d190582a1c5ade6dce4d9ee51a08a1addb3512ab4151893939966a35f5acd0740c8dbb4062905afb52ee390a617a0f0e5aeaaaf305f8dd3ef9d9e8902291c7e284f6cf0bfd19b3d4e46fc79a5b4dda045eb0c23fb07f393deecf927d143d3cc02850a37f24e0058159ad54f7ba06843a0087f99523365ceb219d256d920b1af878f24797192f981366e5c0227890a476ea0303b27b1dcda67b6cac986089ea05845ac5a4b88b17370cc8e6b480640d12255a0ca91d84b9806ab671fa15860084fbe3c79af13d411b99caaed2e9e3bba3acfd8a098ca3a513e6fd573e61152360ce169f069ad07ec5a3450ba62a2c92b721bec7aa0f806f61054e105070f5175c67ed0c3366825e53b5c86f77b609542ca12d1af6aa0a0a815fcaf19124d871263a1904a7e63786544650fba64afc3d31b67674ac115a02fe4442a539724400425ecb5ebaf31f594533089440a617d163d01b50d9b875b80","0xf90211a00b029836967b7ebcf333b071fa408161869b905315f33d6e59fbd2c5e5287765a010488a817032584c14ed42c3089c73b3c04f6e12e04ce15ce6eba7aaf86c004aa061a34691851615f957945135b95576cce6388c829c71b7f907dcab8941eaaa2ca0f83027c0ad53c45d99c4f065716bb6ecc96502b168ab7c1aece1c71349f6ac6ea0c830e8cd31245102ce2d81d8e14749a08acec7d0d1009c4e5c86a4f70af4f7f1a07a89f27149fec3099405fba800139a16140bf15b9a25561cdee9c6329d5c67baa09117a680849ce2f35d2ad14d81d5be98c232477c0299d6f60a67f2ddf53770f7a015f580c85ede03a338c8b729fad6862dda45995545636ca93bdc63a05df05cc1a06997911cd6473a0a05f8d299122272491ecb5ea598b86343598ea12684f736dca0056a036498cff2be7b3ab258018381b2db85f535ca21459235ec6fa7d5b9d998a084680f8253c46a9ca18615192db436c6a4f659ef5eb2ba28524f1e0ab3a7f3d1a0e19a50141f677504c497ea1011cdda02d38e993bc562705debe8e93f76de047da08907f5ce26f64cf1b67d01c15f6543ee5e2953d8f18a454877213edbade0a90da0d935ad7042ccc9e36b4ffc9cb0515837428909a6bd7f99212b6446b08e57189aa0a7949a7d2cfc5af38a250de974969894893006f656e45259d1c0608ef803327da0b38b04d42b95577c119b788c42df9580eb5c5a172b3f765326ee204efbfd6b2180","0xf90211a0bb8720b83062f894fa15fc34a6c4015923cba8840a10270ff7e09a6bdf7f7218a0023eba8240f2d82924ae6257d8545c4b0483a9109280a0c990d7a9b89f38f914a04e16cc5d58d9861eeb80d2bd05183129ffcd7bf906cc9e876059668b8dfcefb3a0ad859331670bdb84bc25ef50f71dfe45d8de72670e61053aa3bc9b530289e657a047f967ea8a68f67175170103806c6a4988ac16c8a8d5bb577a2fcb547b9244d4a00006018186847cd320bb63190e4b0d67f29dc5189782399881ea5ec254002791a0d7aea03dd239834c9090e18f3abf5b7f54a75d5f8ade7c883b4bc58c614949ffa0bba41742aebd19abb96d43b2cab79fdd57a16e8f54f43a472fcf73af8e298e04a09ba87e926fcb2d237dd6afd300642a71eb7e97ca30cd9b8b185561d2ed941bd8a061b7307631f928e72260c018e1c01d613b76459fd278dff59ab688cf81f76c95a09fc0ef46230fa1005a8af7ef34eb58868e934afd943b7cdacb3011800ae5dca7a06a04b6af6cdc1f3c4aaaf4f4c6bc1bdd8a98fc85f57976a6e9cb7f72df46617fa00b2de4042fa49b387e2b15c67b5bcc763169f8ba3a20c6544b43e1e19f8e9ca8a0b8058dde16000c038a8cacc57e5ea905a3c6efa7c1af4b567719439b6932140aa05eb8846549f0eee66e8829948e679630cf6d5138a961cda05c7c01720e7c96d3a0677d4160d4ffddbc47414d6ceb882eb0c170b02d1ca57ae5a7e2b3c025651e9380","0xf90191a0fbb65a3499f06bc088cc264b3cc3ee35c4a9eed446e42cc1e2758597315dc9f1a0e7fb4f6d4585ea0f699ffc15b191c2b8f4af110bf1e52ab9f99a957337288b1da00518be3777b9f679ebd80bc97b8ee5b6c2d6719caf9d470c2cefd8d1b558ba68a0b1f4af98466d247d380cffcb0cd297c95492445c040fc6e3f818115bb364c9baa0f93ad8b6e2b5909ba1b63e9d86b1546f1e0f68ffa30193c801c93d329d44aa7f80a0ad7e744d50c6dbf6ac0b3caa88a909faf7b58bded4d13cf46cc6d21bb31d054ba04f62415daf679925808b072d4a53f6b10ee19eb4278c6efe1fa516518314a0f280a08f19e3111dfad753498422df1a37aa4ed04223ef86ba9bd1b04fef55119002b280a0e581e1e3c7597798b575db89b2be657fd5919a6d423e09c1e785c4d107b0d43aa076b8e80179597c7e0f824aa2ce9f9038b2fa4429f16f09b24d00da137a28c29f80a0f70483485d0bcba3a43493228b33429b94f3ee280fbdfadf434a40372ce3c39ba022a86596ae8a7a4d4a7079d3b6098333a217147d74d4f8c1fb60a25e5152123e80","0xf851808080808080808080808080a09b2f0815e3b2c2284abe0235939239ade2ea864f0c218b105bdd3b41fb4e81fda06108b58fb0f2819d319c58fa46b3a7b6ca166693a51bb9fecef6c6606ffe9c00808080","0xf8739e201c9c99d2b78c1cd9882c73672e829b398b8afad8cfe55ad7e0a5d208aeb852f8508303da8a89a6dc8e1c3472cb3626a056e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421a0c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"],
            "balance":"0xa6dc8e1c3472cb3626",
            "codeHash":"0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
            "nonce":"0x3da8a",
            "storageHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
            "storageProof":[]
        }"#,
    );

    // Proof containing extension node.
    // This is for Sonic block 0x7a3e581b0a3c3813a912cb8714cdee98ef5d57c74a22c55e6b05854695f346d6
    const EXTENSION_NODE_PROOF: (&str, &str) = (
        "0x02b545b5ec868981951a096c1ec700b556cf854cb3836b1ad650b5640fbf32e9",
        r#"{
            "address":"0x70bfbe78548f1159cb9b453e4d6ad0e3648a5a8d",
            "accountProof":["0xf90211a06a39b2443411d980e77a9cbea69756c8b759d254f1672439c7b35d8cc205cd78a0ef26ace5319938e210f19217524a0738805988ecb9a6763d1a01909431ac9f62a001c3b0d7ab745d1628d57bf4b16fbc50aef972b24d964139e908f5d6f909f4bba02d9c6eff5508e0b7a3c3baba4dfd4b3691f9c9826d23abc3144c560078c77b15a00df1b3a983e54b4d8af810d9b003bb8343db35af83b12eee0c3bf273ed22977ca0a11c25bb8878c11b22758f1c10a7d1a221a3a0f957809af5dcae8951a8854b45a0c9dbb115c6cb625c5aa093fff10ec83331896a84ee82547ac964b6e4c1655389a018490c128e44d53e6c7185db38d762b86c03ea68638de6fa9b055e9d2a163c35a0055513aaa8a6bc256d155cc0d0e2ffd65d245585ef0dea30d4f62d615cf84e98a0ca3fe512a4c7d1cdb5b820066853ace35df42b32543efe119bf91de28281c926a0a9d40aab8712ab65157f0e56a96e0fea2a37cc92d0891867435a699196e083e0a059a09e8a318033e377387c774930ec2ebcfb71a47c85bc85177c711a9d9fc7ffa0ff55153d30fda652114da0cb97e28bcd4d562428e6dd4405577f192f0f72f85fa07770a7c43d1676e7c8a650d904c8b8aef3a63c2c80b309adce9c12a999590142a053b4f5a21ab34ba8ee9bad2992d01373399eacbb17f0e1d36632060cf020ec47a0e960e7ad5e72dd7cbdd6a5a33bd29506133a2ddee3aac95203f2fc634d09395c80","0xf90211a062e84d9783df105043f2224bd6a4285dbab422006c0abe5d255b9352f246cefaa0201dc9bbc51bb3f15964b4d8e35b79ae3e27b4948c1f9798cc113063dd7caac0a09f0c56c896fc3b0d8655a238fa89cd37fbec6573517c086a1359d920eef119b1a0b40f473be065b0ad51dd832c254cc9c1e6c2588e8321bf9cf63e1d35762da0d2a0d6a9651e183a91a4a4a7a27bb6061aabce0702896945ac1764cbb43b84d235a2a040e198950be1ade568702b5a5252403bf21ab3726314488af48433d44c1944aba07a0eb9fcf89bfa65e044531cd44b39f4ef767cd1a94f300b3e11e4c07838b1e1a0195d0b4df628633f4dd5490197edcaed221fc4b89a251b26bbfec55c2d60a93ca00f11d79dd7d6c3329febce19c705951ee574f24695cfc9a00cfe5a05ed2b9f3aa0f158d598298ff1fcb154f421e7b79a17763ebc84cefa2a80b6cfd635e96d3720a093ddae475267686a6eaeb09ee16d07c6696e448989333038a08dfffa24182033a02d1df181856bd37e732fa57d36b1022a379083f93b523a0772080ec84985e078a02db2c2be438556086dca34562100fc02a1425ca12bac3a4b89a5f46acc8380e9a0a64deebf87403a3464686982792c9fa64df23fad5f41603b4c2517721727086ba056a1e7ebc8b5923d9c9b5a380b0cac4be92e62db484ba94a0181ab54a7b266bca094568119ec2d4c8ecfeb2798cb8524422f23c2f7220196de6fbe1fa79a8b426c80","0xf90211a0eff2fdcb69b9ae7093df8fc96b11548436d2a52c41f3c62ac1b413d3f30ccc91a0a217a9ee02469729444ca370a262841a5e442bb3025adbb6384da9a6902a7ea7a088f075dd4fb073c436ffc79f26f2ac201d8aae950cad40a98867e2eaca3e0e43a0007f4115ecff911a015e3dc49de98b6cc8124a0a717b4314aefb6fe2a7a984b4a0c50e4ad0a66b34a555270df3521003cd163df032b7904406670615f7a6e96f47a0050d9e4ddfe39293e9788d1df75de6bcda14826315ce6edc139dd112323d6dffa0381efbe3d6332e9c1ed02ae2524209e4eacfa14456ab25d3102ed38318dc1ccea01cfaa6e42e43cf9a97e7069094750729421b29129bb0bc2379ada25b7ba78d09a0625fe3e089fda8f0828bd48adaaf05b9a488a9543fe2245692e51d95813fec3ea0c33db9b6f97d0fad2f6d354e39efd20310b27a3c0df7e02c484919a31a8e159fa09d77a6bfdb7cd3e46fa695e65f0d87676da4b41424fac67dbee836a82f5a05ffa0819ccae0f0d464ed5af27939ea2b6b40c964a1f92900868f660bb42c61312302a020361b09cde0cb4752c14a8ff43798e90bcc665c3a6559ec01243c844691b602a055f8eae60f0ff37d3849711e0f8a6c36df7b4ff276df572a36255d6857d37c0aa02fe6c3d3365829be5ef7d73c8ce4d68be4b816822207fe438cb5a41629a456bca02d4391ef752d7742f360ea5d641df2f3a140ffa9db8aa4c4ee3a218ed394c6af80","0xf90211a0624fb573bd742ec482631e952cb97d0d9d9724a3877c3ce38273b92775aefa69a06e270f975445b49e5443f2f3389983373932b9e7c6952448f49623a8395814f1a0d88868096b2545e9d5d9cdd6854fd80f53075260af0d7edaa1a8b455f4bfaa93a01d83a759da8d01ced9f9da3564584d2b253b4ad87b6bce042504f4587cd3ad1ba0e16bd798ea4110f8ee822f71e91f1cc7c9964435704558f83b4dc8924cc511dca0f60241ffa785afd3ac398c144b89949959e657198602b310d7342bba74853edea038843f09b7927b9d08f8ff38d53bb208c9e5662a92b1c5be23c244597b2e6c26a0f331d6dd4a63bd4a921c7235bb1b9f8a5f57282afa538644741a42ffc792458da0297e7f348d1b011e314b95ec708c002d1bdb68b2a6bfe844543d281bf3b10828a0910ea049ec96d1bf2d4bd73cc0408bf3f3076355e9f4a14008a29b8f6c9f3956a0aa5d061f26eadb96f457f290cd4ff430ef812efddc4457f103485a8e99d2e685a069d1495685bd5bfe11e36251cb4f1843b54bb2ba2638ec062131e54bf9f8fd49a05d57a03f97f2b3e8ee628242046f81a05dbb327057981a1021f4242c481ea566a0422c3ba3e02fa55cb5fbbe31d616a6b54afb3d6c282e92d8e2d277805785dc4da0ad180b184bc3108b3ac2862ec808e423e1e6db3a096bc8f406872420f405d8daa01adde947cdead3d5e42bb18d76f68d197a02634d8b33aa07d635dc44010d38d280","0xf901d1a0f7041379b6f2fe10ef0d4a6993377b0ac852d34913a87b199b03da9c113da7dc80a013981a4d9afe1d87a90bef2963c7694747f9f3b73464dae346a2c1d6434552dfa0be10fe94c6f1457a9a426543eaaee98bde74af5d68c8bb0f8f2f357631fda0f480a0d2294dfb3da2f41d07ba7c94c40b2437e2ad212a515c846e38f810a6271c366ba05b4e4ede834fce22837603cb58214af71185823c4e258dc03d666589560306a7a0fc5c262e4a52c28680b3caace2ebb92a33bb8a6e3663d64177d47907271370b9a0ad06f9124d291b9707911ed19cc9f6185882306d8c7c8c70573f245128033087a03c39dc1176970a4cf7ae47a453bc77fdd752981a3db5431fd562657464d6a61ea074a25a0807dc2891f5fb1ee57b8c1cb98a1a132ae3ff24fe99bd960931ce9b85a0f071fabd6140f7b5610a0b271578c5a6007ce5f9ec376459639782e19fae5094a0a1a78537fdf626136e9e01d78a5955853e5d2cb26038e59c4b178a72293d2fdda054f62f790a3784a6f30a314926fc952975fc663d7807eca482f391040331e876a08ea1b767d06cd9d131d082c81e6a7363fc6f92202dc681b26864c2ff3d1e999ba01bb2dd69294d5a0a7bbf327e73a800ca7f17a9c7688d92f6acdc8d71038225bf80","0xe21ea0a732de75ad04d82bf0f35b1179ca9a8d7c3f3a9c0417ff217b5c98da0f09c02e","0xf851a0b1440864317362d00c935efd665ba856de0aa4ac3ed6e4dc3363df0ae135e6caa00b789da4311faefe8e3f4ae67407e8fdecaca8e2cf58ced266b3f051c481f7c9808080808080808080808080808080","0xf8669d3d49a153be352736c65becde824052b669774ef9c43348bd0e2071360fb846f8440180a09a03b116991098da6cf50bf51759f21a1afd28503bedddc5639a7cf4b729571fa04e03628e5c454741655543b84be8db6a4876f66a318c80e20684535faf8a36e5"],
            "balance":"0x0",
            "codeHash":"0x4e03628e5c454741655543b84be8db6a4876f66a318c80e20684535faf8a36e5",
            "nonce":"0x1",
            "storageHash":"0x9a03b116991098da6cf50bf51759f21a1afd28503bedddc5639a7cf4b729571f",
            "storageProof":[]
        }"#,
    );

    // Synthetic proof created using Carmen, with long shared prefix (0x0000000)
    const SYNTHETIC_EXTENSION_NODE_PROOF: (&str, &str) = (
        "0x301daa3d0a4d0d4882362c81ad3f4405f66035bb0b20b9636b8e00a4391bb386",
        r#"{
            "address":"0xf8072d0000000000000000000000000000000000",
            "accountProof":["0xe68400000000a0390f441dc6cb92efab6f4a52c52385a2e6abff71c9e2562666397f3b213be2ad","0xf8518080808080a0209176cd61b113c321b3fc9e26a1e03171f91cbfaafc16c8d418c123cb9b48f18080a079ca21e62fa9956942efd981fcc9ca9c014c3dde1e56913d4c6cbbfc05514a648080808080808080","0xf8669d384bdb5a495b968790fa101785283da4e4c75c3fa508e0479b0d6d74f3b846f8440180a056e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421a0c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"],
            "balance":"0x0",
            "codeHash":"0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
            "nonce":"0x1",
            "storageHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
            "storageProof":[]
        }"#,
    );

    // This is for Sonic block 0xce61fd03b55c72fd56b6689b99e313f17a56357bd920d1a8ae807429908f8100
    const NON_EXISTENCE_PROOF: (&str, &str) = (
        "0x40a6309cc36231501331ee28d87d831d785b9b3b4623aea3212f65f9a226b131",
        r#"{
            "address":"0xca017cacd3bacf37c908b004b14d25cf3e1fa775",
            "accountProof":["0xf90211a0cfb2e804152357b5ec2fd17174fc790ef3e10bfb5dee07b3cb3edad573ffef26a06755b3a9e39b84a620b5a88f17e98adb19ba93b59a1a52c95a8d9c17ffcb8cefa03df8c08e2d720d00c1de92e5be2a35fa1f5e62974a708d034c94cb64c5bd3f0ca0938853f4889ac3c740e1e4b3372fac3bf62b8a9ceed67589d2bcd47836e60e9aa0986e9ac83c0e85575861e379296adfc449bfdd709b1b02f054ff73631d21e559a07715ad68046137f6704164ba0de28a14ee7897f31668fb8efa01ee8481abaaaea01f67e9f700aea3bd418e21281bb4a7659537d1053b767407faadb025692b941ca00007c923d8057eb31ce46b09cff75c5907fad4d94a1589daa191da7ebd7dab84a0475aa32a74c8d2fd357c36a9cbadf3a9cfc675e8046a7af18ed92d3640ae8ceda06377240e15b400322847f94c640d1209ad236cc19ff29db636aacfcef896d018a0d4fb4a6362d8514f604824aa5192f6e0ec4e4ae3e213560c8ff4fc7b5eb1f3d1a0436f1e0a53f55188f8ee6deaf2b7bcf4b4348bb5491a8de6993be186fd1ae351a069ba587fa2ed5d5017ebf3149fbdb1a25a821e89ab12ef2371adaf9dec034fb8a0f69ba3204f597bfdce4ee2d784e76f701ff15b01cb0fa5ef6789d6313cf3e7a2a030891a16b243d524ec3a24d35a54e50b6a3d010001b6483ba5dd12299aad1f99a071210942763212cfb65e863a721e960a66218e9cc33e320976e526ca1602445580","0xf90211a0fecd38edece465dd91d3c8eb24fe5194b84031cd1fc79dfb3d498cf8f1cc3661a0cf4bfdd3abd45d62c5ec79a7fcd6caccf6497d6f990c4e8b8678f77913482786a0e7abfb98c228cbbca8e95bca18316f244ca61b9132366e101b9ac6049e063402a07b6041d54648308207bb3b9781e98f217beaba0f23505677c3db02d8ba82f9c8a09dadb71717653909de740ad2128b9e08e6d227a8a2cef49868c432a764dcf723a08f39bf926c7a886eab3f7ad2be11b0cb653f9c35394e2d9e83339829d2f4dc12a0726b06fad8442f4da583af5619a9e27f1cb0a6d29082fb5f4c9fef9662be6fd3a0b06f69f8d89ed91d7fe8e3af21ef8ff9f05e6c995ef594da8b87c5633cce6462a0b2676605531163fc21df34119735c96567f1b235959e2ee3d1698c7b576a0466a0d8585a1c527d550eb43288abac9e20730673890b3c6121af634f211dd6f3a8cba05042b508f8ff51c6af844aad3fa8f18816f61d926a644b3703297d0cb7009ac0a08e844831e81f983731f830116569ccc904bea40928c910faaae1390b16f59caea08be0639ad5c0f3495b98b2f6c0301e1c47349c7e442b902303eb21957cd5923ca06c58efe6241335966dda5fce61370da1bf3f922da94e509b00db957a20750e3fa0db95ebcfbfbeec011db92f22375737adfa832277625a151b9f8af9749ad384c2a0c68c4037004cc4021df1cf453c4c79b86d65fc39eb0eb12b989498b81467d0a180","0xf90211a0bdb8e8e026ba504d9c2314403c1b7f8729111a255132f1527b07faf7fb36102ba0815c6985cfe63d2e7235d1b63d32f362205319e3fe94a40ba83263ca04bf4610a0f802da7207c1c049cede2f800b3b4461f1537e7a57177ddd5fe0def806d04f8da07eaf44d3f97d6bddf940a95c58ce72368ad8052318ad21db74b2610d088f0be1a06ee6a432456fb60ca9316ffc0b9e775750caa0d70b4d5b3cb3e357959df6a6e8a0a7e1312393a9a31739b48d46c4d27ac13b59d4b8f8f9a4fdedc1e816199261bca0220401d231a59cf0d05565ee73bbde98be1d8f874502db18a86c68626b0533d9a0a6c45632a1407aee6178c43b6080d9a531d88349a81e219fcb6b700cbe4fb324a08a77ed6859cb8c2b6f440ee85a528cf17ae17ea41488e295292e9d841b4e0899a0b51f94487889085851d3e2af41cacedc68bb89b5841cf24226066715925b2511a01b3dcebc3ab4b6ee3fd39f87d5c628dbd4a6fe20a2bdee149f8563228afda0c7a0d3c72a76a4ccc2cf1e62327aba80f5db7b0aa82a67a5d00d81f9a4dba4b867cba04d0d022af6666fe20109649eb3225f18651cdfe0355b519cbd2dba19625bc5b4a0e1aa20401bad683448e3cecb7c42a9c6f3dcb805652c054de951018bb9f7391fa0f516999cdad88144c6d7cb48491713763628578405e8b2da42cadeaa3ab96b98a084784a6bb9dab62e216610efa8773a547f18ba6db13d427995ee5fefc689a25580","0xf90211a03e071e530112b8f4b1306cd7499f77cf7b25c92b944e9f462e3526838a92473ea0aa78eb8328c5db0340974373d78b2cbd08a67dc7d9e19a43578892b4ec9a95ffa09cf7953c0269096de100961224792184e3f4739b26db54108879e0354765f769a07585e0d84224353231b7d5eefa4f10ddb138dff6f079899b76328361ef02e5dea08ca218444d72c0a232f602ab166c8fcabfffe5f5872a513b8ac38eb207cf2297a00d0215c8ac7abe2056bd046b97a8cee72448d17c37bb368bbe23c27820f57520a055cb136a7a9253abf67533e543b9e6e81d3208cf3f68a0652cd4c8747cadc472a0bf3b22d069e1b730b92a296fe7fc92e6b3aedc5403766c2fb3a5bb2912a575efa09dbf307351c9e5377f89509375fa61040dfc02b1b63c9268550985b52737bd86a0a01208262d1fbfea40273b232e8ae7b34da03eedd08b0be0ce662ec26f76977ba0431eb7deb12a604640a1ed00b95bca88342c36acaae22d6284364bff36a5c9cca02b3becbe0c4b56d4e129630a939a2e0e4dc8c015cbd27ac90f1f1ff36f2a7b30a02eb4d152ec8968a30091ce72432190763e535c2823d6da453b3a7e17d7b0d3b7a06acba1ab6355288ee4a3fe7c1ad5eb2bb1f113da08a95e9266d14ee1951163fba01ec969d7ce2ccaa0cd978a9ca2597678b70f24456ce093e4c78b838a54c56756a036231c8f41edf621a23df668d80721a44594170658ccf5cd4f500d8261e37c4980","0xf901f1a07906e7b21ccc6e23829f6a3d080da7173131faa1310afbe9c71e7c4d3be57983a0d5449e08b36c842c1a8d8dbf8ec0584de9229adb5299eccf094512e5e6037d35a03bd61d6f4668eb656dc60efb8465bb77c5e8f8209c1c45279c2ff62458dd097380a0a7a5f7530382f83dc47a48f29071136b944ac9750d9193dd661d6955a0bfc674a060eedcebe0e7573376269b6ba3b99a1baeab0351a8f291edb70a76f73510040ea07fbca644e9ab2f447a832347397bddc429dfd757174ab8dee7b70b7021e3a3b5a0da3996d2c53c8b7429ca466cc4a402f37475043c2d9a2186cf01a85f13410ab1a07cdf960cb92c009878b4c3ec5a6ae4d95e3d86f4fab0f174e29ce7fcd590e2f0a039046653c72ccce65c1b6930712220dd5ecb2133cd4df706c22adb70f7286835a0742facd6d404116aedfe8d5af62fcaa4019e464cb3fdfdecc21f629775f5f641a0271f68571d8bc46cf357bc07b1b92ac23309b2f71e942e13aedeb5fb53ecfe79a04cd76830468231a95f263cc37e375296d80395340c8b03c3eb965bb4f7361795a0ab08bdbd41e333d5a631969034b3c9555da4915536f635b2b571c282aa5be3faa0e2eae3dcf68ef5cf68233d784caff2077465d6cbca2557fc43aa5e1b952a4fe5a09f204e4b3dc1722c00feed4e9816a5f2cbc61c1471cc0a549902259144be89dd80","0xf871a09925afc59f56a1576eac410948bed2c9b4bf5574b367d642b9b119efd7599b97808080a045fb3fa9177e6a3ec26f18cba6bdb67203d21a03671aaae4b493e85a0fc9e1428080808080a05f818db8104f001ad4ced847e9663c37bd58a033da37bd1b96af5da9158fb8f7808080808080"],
            "balance":"0x0",
            "codeHash":"0x0000000000000000000000000000000000000000000000000000000000000000",
            "nonce":"0x0",
            "storageHash":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
            "storageProof":[]
        }"#,
    );

    fn verify_account_proof((root_hash, proof): (&str, &str)) {
        let acc_proof: AccountProof = serde_json::from_str(proof).unwrap();
        let acc = WitnessProof::<AccountState>::from(acc_proof.account_proof)
            .verify(&Hash::try_from_hex(root_hash).unwrap(), &acc_proof.address)
            .unwrap();
        assert_eq!(acc.balance, acc_proof.balance);
        assert_eq!(acc.nonce, acc_proof.nonce);
        assert_eq!(acc.storage_hash, acc_proof.storage_hash);
        assert_eq!(acc.code_hash, acc_proof.code_hash);
    }

    #[test]
    fn verify_works_for_real_account_proofs() {
        verify_account_proof(BASIC_PROOF);
        verify_account_proof(EXTENSION_NODE_PROOF);
        verify_account_proof(SYNTHETIC_EXTENSION_NODE_PROOF);
        verify_account_proof(NON_EXISTENCE_PROOF);
    }

    // This for Sonic block 0xce61fd03b55c72fd56b6689b99e313f17a56357bd920d1a8ae807429908f8100
    const STORAGE_PROOF: &str = r#"{
        "address":"0x29219dd400f2bf60e5a23d13be72b486d4038894",
        "accountProof":[],
        "balance":"0x0",
        "codeHash":"0xdc7d03167b2b33566fa1cf50046e5bf4828f6d401678ed832d2cd7973ea17380",
        "nonce":"0x1",
        "storageHash":"0x2e4e9ede39accda32c1b5dbdb0456631eb0b3f9edb056e9b559847ee231a762f",
        "storageProof":[{
            "key":"0x031f7105616ee437d22bbf3791fbae0ad36de66b37f968c178e501f10f5f6bcc",
            "value":"0xa1a1f4871",
            "proof":["0xf90211a028919621a983379f0c91ab970bba3540055c38e2a1c2b090dedb6c9b596c5319a0f22e8650dc6384c8bc06a6e2a93c334e18a93218bdf8f1bfc6befeb92e624d59a02cab6474d69c114fa7e436b49ea348e1837e17e754152648a355fd485ada3d2fa029c6e9fb42054e902640acaabeee29b5fba98da1d9da06191833468357f4193ea0dc1700db8e37de661041497f81262fa30e538af1bb2b8d34208d27679e872c07a0ffdb573fdc44d7acf8c7dca81be064ead42c319c10691d4feb0376ec3d5f7410a0ac591879c9087544e4a63f5d8ea8586b0fc1f21b9202a1664bd89a4a66faac61a00e09d7794d8237d5b8efcbb97692af546235cf59154fc3551c37a98617dc8734a01a34144aa9c238ca5b366e10268428b817567a41350b35cf8fc638dc8992760fa0de0c98e62a73e0f049f1e2d3f63d723d1871f6222f465378566a8f7d0cca183ea0abcf94ef1565782f5dae11ba731f3b6a7bbb7bf7e013c500a56654ec37da29c4a06d72c283415102f6c7face200c836142370657d2322a6ff0506a8bb7a8939a86a08cfd0f638484a56876922affef0e9391c1e86891e4666a18ba8eeba204c17b5aa0401eb33541aee65ac41706486e330a9a22ac15385cc90fbb5e7cb2104e3ec466a062fd4b0d22e58fedf1402e451c136d9a317be185607712cc39182a9aad31802da02ea29f42a6a1b6cf46b90be533e99783b89d5312422208c25a83922acc35ea7380","0xf90211a05f2a6ddf5a49478b525950530337bdf0ece537f1d263550cc9b0ce0841e67b48a0fabc18ec5fc2f03a8ca6224033852c47981c98cb1b2b4f4c30b9a28ef9a73260a00c95b6c09d755a4d1721086f2148292c276c3b84de2aec4c8c55f61d967762cfa06bf003ae2a0492c90c13f6478be7c0ce3d3d4db3b710f3c7b1f4381675a03bd5a0d9c06b8e2dce4fbd292d562464dcf02a9c74abc92deebddea90d5569a4ae8825a01252eb12cdf32c0bdc633e7994f3dd3cb65939d741e8474c8c224ddd3c49b879a0d1dd11373bdc4861062b930aee8abe3aed4eddc2c92cc303d754898bbb026b13a0ebd73db4134bd7e40efe9f3ecc2d4c21e94b80ab5ba13f945593061e7870e042a02d9de5923203846ce438b604b77c81b322488fd788a2cd470a807357ce183b9ba025bbae3ef35f2d85a5c3079747df4f6103c92adbd4399b3dd82d4771ba181b69a00bf82ca2e014f5cc75e43875e8d90fb9d67e0d6a034f02debfa80c2f66f3d718a0cc2a5864ae6854dde79fcec8f8d8d463741d543e5239f561c30e4fa8134a3ed2a01dd21827e4c371203a474b7bce443a364b1dd6d6ab1f748add18bb0fcb25a8eaa0aef0616e4858fc5684fd1f62eb44d8e953ada860fc4ff06fb678fc0f18e74c47a0a358efec264564e6bba17341d5ff3ed03bc006bc5495aa4c79158e72b83d0d4aa09585e1773ee859d3be9b3e3400f2efec7d6db05a70f74ee974f92442b47da5a680","0xf90211a026738317bac9839dbfa04603e03ae8cab8b20c222c84ec3fa8e8fe6f04f72415a06e7ed9d7efbca11d9d45f9007c01b1f6d689b9c266ac9ca6d21fb9c688d81cc4a03dd6ff8da22bfbf339dfbe5010749e7a2c91f7471424689f74e9261bbc5f2feea0106f55a16987a99eee770c2ee43b28746919d93cf8ebadb2e87e5e33cbf5925ba02fa02a62ba0702c1a772774031db327198759c2ccb89ad16797f30a95bb20b4aa00dcfb6c253da3e0e5d4c0f06f09c96c541761c7341feca268cf22e2da06007b4a0028139d4ff124301f4976b314951e524a5d7aad0edc5087473e5d02aec4b643ca0d428a55c2694c3b6c5eb9b9496f1545e97401162b23f0e9be6c7d9f410318cb8a04b99593de56b751ec6f2bf17beee920a7417c917f739e3713849613e96514384a074d39001a64f94270ca4e11aa936101cc6eff5185c5a632947d5a907f7bd3342a0a5e5d3f5bf2b47bdd230c1393ef67b9ea258085c3c054770dbbefb7fadf70bd8a09c85f4ac6141c3c14536e8cbb19d31aa9fb81bb03224e1a76a0cc0a9aa7ecc79a00572878b6d94085a353a36bb85afe5ea1b8d28b79909f510d0d747f77adccf4aa0a0b06afde6949a626ac37a8eadd5597c78e4977cee99e5741c3235eb2099d688a08fe7314955d245932e02f571ed64cab8f2f12abddca5b7f13948834daaec0636a0b3c1f759887c25ac9f166ab00d775f88782e03a9569a4be524633085575880a580","0xf90211a09ff18fdd5321bc32739c68fc82c18ae3379799264684fba75b8eada628c49c88a08387a28a5f7a5ca4d8a1e826a8080f30cc3f4d729f1fe483874aaa0106006a10a041199ada0745c727bd35a0951873ea87e5d1a945bbaaf9fd6c0b540781012034a02edbfc08e5dea05a138f86c375fbf05c9085123c734326bbfa0b3203fdbba888a07b34ddb4a71649f4c9232b6190a72b9d949db07a2c48d0554b6450180e5e55b4a03b28af0b1a8294b6fa3bdb64e82e358cf7979c2aa8abaaf0811f336c3d922356a043d02d5a427bea6e6137efc024f57c35b6a2f1322eea826c78086cb68c336a53a0701a17d4d5cd63404bdfb5a0a2eb78127c39a41b54b107b3e3c6b267c90bb6b7a0ccdffc3bb5516248313a3e3a06797f45b1be2751321856eca2b2571886577168a05daf5d354c37f1ac861f078bd9fc01c4e5b28a0d984b4b57a35f6068fc23be28a097663a7c0216f4b3769d1dd9ba8e30332755229745378d9d1d739572fdfd5054a078d4d1668b582b8a6eb8224bd77ce48797492ea2eec245e28261a9376ddc068ba0ad8af91f8bd6cc81dad2627928c6a00790d34ffea85082e820c6d4860b3916aaa0bf049687a48fce914d83fa4cf958428ebd91265fcc406945502897e90dbb86e6a0f70a1b89e9097ebdb4bbfdcd4dfc60f73e6f0463c2d09058470b6b3fa0d23d57a0e635de3954b58e6edd9b280d534cf15d5a957fdd90fd1e461f14366349ca93b780","0xf87180808080a0be33e237e716c246e449dd30276ffa69ae2dae130d9baaffe813079b3d2ea5d580808080a0176108580f4ddbf2453a9e8d1bf3fc4ea9598a73d46b36b110fc197ca13d863480808080a08960751e1fed1975efcbf7f66b6defa1699410b7ad600334110d6ad53b49f1748080","0xe69e34ee9e7ef0f164c9ee65ce9492b033ae0a2c3cf2f1f46078c5f8e5b8fa3286850a1a1f4871"]
        }]
    }"#;

    // This for Sonic block 0xce61fd03b55c72fd56b6689b99e313f17a56357bd920d1a8ae807429908f8100
    const NON_EXISTENCE_STORAGE_PROOF: &str = r#"{
        "address":"0x29219dd400f2bf60e5a23d13be72b486d4038894",
        "accountProof":[],
        "balance":"0x0",
        "codeHash":"0xdc7d03167b2b33566fa1cf50046e5bf4828f6d401678ed832d2cd7973ea17380",
        "nonce":"0x1",
        "storageHash":"0x2e4e9ede39accda32c1b5dbdb0456631eb0b3f9edb056e9b559847ee231a762f",
        "storageProof":[{
            "key":"0x000000000000000000000000000000000000000000000000000000000000beef",
            "value":"0x0",
            "proof":["0xf90211a028919621a983379f0c91ab970bba3540055c38e2a1c2b090dedb6c9b596c5319a0f22e8650dc6384c8bc06a6e2a93c334e18a93218bdf8f1bfc6befeb92e624d59a02cab6474d69c114fa7e436b49ea348e1837e17e754152648a355fd485ada3d2fa029c6e9fb42054e902640acaabeee29b5fba98da1d9da06191833468357f4193ea0dc1700db8e37de661041497f81262fa30e538af1bb2b8d34208d27679e872c07a0ffdb573fdc44d7acf8c7dca81be064ead42c319c10691d4feb0376ec3d5f7410a0ac591879c9087544e4a63f5d8ea8586b0fc1f21b9202a1664bd89a4a66faac61a00e09d7794d8237d5b8efcbb97692af546235cf59154fc3551c37a98617dc8734a01a34144aa9c238ca5b366e10268428b817567a41350b35cf8fc638dc8992760fa0de0c98e62a73e0f049f1e2d3f63d723d1871f6222f465378566a8f7d0cca183ea0abcf94ef1565782f5dae11ba731f3b6a7bbb7bf7e013c500a56654ec37da29c4a06d72c283415102f6c7face200c836142370657d2322a6ff0506a8bb7a8939a86a08cfd0f638484a56876922affef0e9391c1e86891e4666a18ba8eeba204c17b5aa0401eb33541aee65ac41706486e330a9a22ac15385cc90fbb5e7cb2104e3ec466a062fd4b0d22e58fedf1402e451c136d9a317be185607712cc39182a9aad31802da02ea29f42a6a1b6cf46b90be533e99783b89d5312422208c25a83922acc35ea7380","0xf90211a01cc2b71e2d61fb35bb5ce684d7697a7f59f4b1d733ae778b94278b41ecbcaf9da077989d41a9205949c410a764aee1138117ea33be8439d72e8349fb67ce49c967a05dcf3b76241d3bb6b2dd62033125d48739e67b246d43615527c8217152d06dffa04841005051bb3bb3735b4c68f2a0f95c5cca997e7df31dde35338a773a123721a03a757090520c0f8dc411be5c4872bd3cbdb890f6ba1f4338cda7aaf659299030a0d448bda39c6d208477c51090ed416e233836a01c99e2a46564bca43544bc49a3a09c760b74e04c411e47bf5c4dda8c6c7637be75988e4f69bb599a77477f57eaffa0fe82e71dd8fd4ba7f83138a3b83932d047a9a525374d97c7b4ee38a86e64c8a2a071f3028a6a31bfa60b2182a05eef9e9df72c86cd60d11142d7519bb653fd59dea01812688f56db28c59fdd2edb5de39662ef0af40e5fde66809797afee08f87b64a0ae9e8d239ab9dc73916d2917b03b6b20311a25cb4c91d5b43adea67528f82082a04da9c78eb69d42fd6af8acbe5b42a44917a98009230a23872ba7921c4c0553b0a055a14a7e89522dd090bbf6813cfe81036cf8974239c3ef30fa57a04762dc4d14a07e31bbed68e6fde50865eb2bae36688812f90f3eb541281a68ea8aab6f176456a0c67b3217be92d1e2fae4560f2eb1b6db2fdaa7eb9e96dc04763bf6165c16c435a0512576a5dc60e1f6e0ed586b718975f856030a64977c9f648968ba3958cce38680","0xf90211a0f1e2782a79ff91e2798c22ed5f3ed482c9b3b706e214368d34c336dd01396ceba06e9cffc17aafc9a0b5a336a2a85f572fb1b8445fd8a6a17d2147738cb5702451a081518f5d0e8f6eddeabd80fe0e0ebddb916855aa027110e224364cac8c151f87a0620589b6be1cd0e91afd3ddf12cf73d83c6cb0dd13cd1cc3d7796692cefd4ef3a0e6cec34c873643b95a804decba77e6531d6a72551f6f2b821f2a7d61ba09a57da0f567f4863fb6567f67506209c11ee2a583780a9164abac63b6440ed45809b3bba0e69510e93df004d787d6b08d64c91fdd6917a060565c4d72e3757908184f2971a041747c758cad939bbc4d5f3b0140ebe75fcea630f88dace81a411061d46a0fd2a008eff9715d380a8412dab082b1d7ec5a76bda2c287264419f1a95a29d33c8aefa02d26ea10ecdf8078e2465ab1831be5e61700c23038c274da6d5a330de06718c3a079e30c3b15584da5e9a9682b21eb39e5587a74cd1065b5ffc8bd61485d0dca22a0011c56a6ba8ebfc0f60b316d102c27da55a0ec475e9a28c41b9bcca418f64323a05490f552b1eedb8045e02dbfc12c64741babce68fb399a7392e59412cd40467ca0055b418a9a41bcba62051cc83497b4ad370ee4e665b211dc1e8fd08c5ee0427ea0f3419409ec7a06fd7dba9091807d9c5ca685f8d49ee9bd36cd7eebe6b9531b88a0b0eb743cb9fee02d0e6dabe840b6b9bdc7da794a2a0cac63e69185f129b8d7c380","0xf90211a05f4955b58cd12d9d3bb10e42f98e20abc4aeb8dab1c03b836d93cfefebffb1a7a0a483b64f147edb6ed56af292caa13239f9f62d272a51c5208db3958669198506a0081ce5300e50051f8e1f0b1467c03c94e245267608715fbca5fccc25391426fda082af277fc3dae4451eae8a067001da3f36311664d1859e3d81ee5da18b4c476aa0380a168ac499d9a6581af21b8d0b5f81c06109ef732e2576c85f2135a52b7873a0828c758b1b93af00c133c1a4643857b5333bdc5d64865746a3feb565cfd08f39a0714397b190dbaa6312622e71abe4dec35bd281d209416c985560f2d6a35aba8fa0147e7b52bd357dafb2dcd37b5071ebbbe602f6a614ad09dd02aa429363fb40e8a016525961ac963ebf2933c0ed48c2c0009bc16cd7179f9891367b9e96359411eba01b15cc81611d178f295893ae53a87b3abd4d7ab8785f6b47cbdcf5f63dba52c3a0694a59a6efa9d87c0645360798dfb1adbe8265e5eccb47ae7db5ef3ed4417bc8a0b2e9dfd343c6c74e593b47f3fb01ade675ecf724ef464d495cec8318eb08d5d1a07dfdad5b8eac4ac3486873b4829e926290f04252745475aa5e2ef6242045c3c3a04a6cf37c002a22fc57c8cf0a6abd2b9b48bc880e31ef4ac882caa09aac0f9a92a0aea8cc2d04c3a896b63e15bd2f81eeb8dacd1de9e79e38707a2aeee5b6139b54a024a9a9f01bd3257f5df5e7814acac0810f024d2ef98fc0d6ee442c65bfc7f2f280","0xf89180a0063a08ec608ff0d6199ca8962a68b17a845125b7ab0732757ac6513b409d48188080a04f694e1ce44e2c7b75594bec50114318b7391d5b2bee36e02140171873bb79f5808080a0ea40f0e10a4f5f3d1c44048e71438cfbbd3d6c54f39eff15ad21f530cacd7743808080808080a0f7d9dc13b820e6ea6e451bdea68b9fc25ec4682422776998524dd377c70b1df580"]
        }]
    }"#;

    // This is an example of an extension node marking non-existence,
    // by being the last node in the proof.
    // This is for Sonic block 0xb724cbcecdc0861332b86bb5c3eb6610cdd0102de389429c657a7bdfd78b0a27
    const EXTENSION_NODE_NON_EXISTENCE_STORAGE_PROOF: &str = r#"{
        "address":"0xd4ad5ed9e1436904624b6db8b1be31f36317c636",
        "accountProof":[],
        "balance":"0x23e649fd3fa970b8837",
        "codeHash":"0x26000284133cf300d330d3f665fce790e9dfe63e3460c1f706690f769c3a286d",
        "nonce":"0x1",
        "storageHash":"0xf0a5bba646cf77b966fa58d850ced1f5886fd58bf7b6923b7a2ddfcef93f96f3",
        "storageProof":[{
            "key":"0xaaaaaaaaaaaabc627c839821520502653d823567393dc199701c33514afd5f13",
            "value":"0x0",
            "proof":["0xf90211a0347159d59ea6338087ccb89c6aa9c06c9e7d06a9f9d5d910516d7e4b205f367fa048235def22df028c71cae2f065f33080af43670b884ee8ab830cab159e068d11a0e3b7fe1c46c2709176219397765e3bb25b0853f438b8e5848f5c0351e361ceb6a0279f5fed7b0283b6cb18fac1e742ff81e85821eae7ea9078819ef3b0945da75ca0244059ed41d4e441b4c987fceb5d8da6b5c4198171f0af89f4b4ed5c32dce48aa085734c40679b0003c9c58d59356e7047b8c571d6a176d9129d06203f7509418fa0bf8c901b43c7913b2010363e639dd70fd044af8ed665523a830684d195a46570a0beca4ec84806d292db63e211d3d301fd96431cb9b4f06888ad2bccd9360008d7a0724996ef9adb9d10ea7a06304f56335d719a2633eae039fa7755e6f6ea3fab7fa06ebe63126c383593e9ad591fccfc1a58d039ea4f482f6090656d6a8095cd5229a0fc8390d5b0797536e830f29f5b5e8da6f427fa88738bc3a4cbfd9cb86e5c761da0a5094534e92c3cd341afec952a3ef4171919a68420d23e5a94f4ac1e6f45c9aca0aefc57a4a770472ca2aac328f4529449bf1b236f26e74e50c7004b1c5d4e3877a08df11e75743972171d2ff0bc769c3a7ca53c907df6a9a0a828330df6b013b8b0a02c3bc2280f683f85c9713512907e062ac53c246dafea2dc8ff492c6e9657f148a0f76b88d81ddcf19724d3ba95599d1b24b945006a7da55e4de15ae8b416249a0b80","0xf90211a05c7175f00f176dbff4e3ced35e311df80092b9387cccea26ad0df6ce1d304736a0cbf18cd7522cc078ccd52d89b21cda1c60b6d486f95b5961ba06a6c2f8d10a1ba0d909bc5da052134a181b91d2e8f77bbb8afb192b606398974bdd32dcc07bfcf2a0320da1cf6806aa1363a87f02ba777b8c412aa81aa370617d66f7a9fbfa9a3106a0fac90544482e60863846de7e5eb88e8f9653531303a1c6435a47a45f8b9c8c2ca06dfbc73ad8e8676e73a12f525fa699a282475b556af6fb8e0b62ad0375b8b688a0b33e5a00ff4a187e2dcdf0264794bef4121aa1f9af5261206da8c67e1391c500a0667644b2ad838b95d27262b00c9b346c65414943ee559c96232a0fa56688db5aa0658bb134016fe4e008df56bc2a48738747567a3d8f5b564f27598d6307365e9ea08da085ead65608a215861789ee6fa4d849db82c638140ed6526302e7557cd7b1a0f9e925b213b46e7f3d22fc86fd146193744f06663a87fa2538a14c194d8fd5b2a065e22dad86ab7f620e7db5d66c5784b8c8f95d72ed35988db888817149ac31a9a06f1ba0d011b9a0c08ed318c237e46a1be03445a5cf88e13c30f0c1e23693ff2ea0d5b8d1122578005134a19eb5838fe130f86a1ee181d3f48bed97746bbbdfbadfa0cfe8c500c4635f2f95ae4b1d2f2250a5f407f71f06e26c8c48a0d7f392125e53a079989437306aac872dd23becea7baf99f0f1f31ff049cbceec74e18abd63c27580","0xf90211a0866ed7988f4f880c0c47de7ef3ad46e3c4f39ab7348b9b0d2627885e68e6ffd7a0f595708284eb24f22a2f0878429807d3384382311cddc6423615d8a9d7b7faf8a01dbff478e7408497522f20413dd10f66ab6c5c567d19d660d18cf22ec66b3d4ea0eb530fc2cc37b500e812a2b140f6e2e0f838fa2fe38c584d92271e076731b024a0306077028532dee6ce0bc25dd0f25c8643f8b2cc6d04a5ae9e4a8e27c267c467a0ca68f75743045d1972386e5c7dbd3340293e002d004ce41162807cf77fa1e54aa013347542dcc86e66e8e41e3cc76ae36ce7e9a999f0b51549b65b0671bda4d82fa07ad0d03ac61929eb0cd152c01cd79555ed73364d29fcc1aebed724f128217e18a0f93789635afeff0f5d1a77d85fcf32606dfdb387cba7e5f34be1d48207071f8ba0e8ce74932733f6c1c3a1d9cc2a14e69512cf4326af3ace1e340bdef3c392c927a0fb6e5e650341ce0b5a611a52b9486afd1dcb35e6d472286ce44c0961158ef50ba0d5e99f79d6cd329ee6e812e12c5ab82ad23e49030388d0d175ae5c046d5bb3a3a0c8c9ccb0fcbde936ea46a0538282f18d53a142eb8e7e33e21f3e3f7c1cc1a330a07a02dc213fe7c984a9e69935f95d27133d76fdd3ae9280b88f3b42a6f2f80529a00a7419f7dd0a39f639954308d289a8614b50232f2a50cecb876c6c0249a74426a0c3051d05923f7c23484ea9535ff8c87d3b6dbceffb8250874664a8ff17b8747680","0xf90211a06114ab5d04e93bca74aef8b8217a8021bae5d00dd2e55059843344ac8e69e7d8a020d7b952a4089e3d63043b4f775a6ef86b50e41b07935c578d74dbbfea487d3fa075a681ed16f63cd6d63426eff4430c10bee450a2175258ac2723fb1035fcc945a0270655a6e190318777b471a40b95b68ae0227fd25bbe59fbe41350827d6ddafca00b595d04a8c242a209fc1d08c4dee4b1e9333d5c9d05b85f473700227dc97eeaa0ca0bb804b1f94172afa7e6cfb2e6a3eff3756cb2f2f41dbc1ddaf8edeabce93ea0f802210abcdf6da9c62f094f82d573ba2c7cab7e3467673b62b4258c8b33f85da0c47ba9c9bb4f025f69bbc652575c4917d9a1d4a9f7249b4411a064ee7d4cef6ca0e97af7d0872793220c73eb630fd6f9b5ee51e97e3d272f1babff683455c2ff1aa06d0aa85590e3f937084cd42d4bf0b1023f4565e9aafe892a913214bfa9fd0154a0ff62245b63a3bad6ee000312f8fe700fdc7031b274695d5fce8940f25ec0c60fa02325f1a08e8a4cdfa945b7e41c6ab0c85d3d3a004f6b2b5783dd28ce187c7aeba0ae2e181914142d577dfcf44fbdc4889d92ec4973dabf56b31bbc33f1a0708736a00d9be361efa9e1cc29d4d81e9e7fbca032847205d4d1eaec767521accf7978aba09921068e76c12206ed510dd0983cb7719502422c58387361e9fd625b8794fccea0207d070bd85b8507433246107a7f7fbf96154d215e04405a7f4281ed5e89c84680","0xe21da0f38b925bdedd97bb569e7a17b08d76dfbb2c7a58a12d65eaa0b84a3039140aa7"]
        }]
    }"#;

    // The keys
    //   0xc76547ce3912f8c25a9943819c2992169865dfd500bed5213c8a92ceff5db5e3
    //   0x2968f9295ca3ab4960ae553a18f47567e56f2777ad762ee1d639421728926a37
    // have a 4 byte shared prefix when hashed (0x52f22562), which then
    // results in leaf nodes small enough so they can be embedded.
    //
    // This proof was created using Carmen.
    const SYNTHETIC_EMBEDDED_STORAGE_PROOF: &str = r#"{
        "address":"0x0000000000000000000000000000000000000000",
        "accountProof": [],
        "balance":"0x0",
        "codeHash":"0xc5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
        "nonce":"0x0",
        "storageHash":"0x9c4bb2487c9bd27d4949534cc1e9da638daba35b7eb5cd71ddd23c69b2ceb814",
        "storageProof":[{
            "key":"c76547ce3912f8c25a9943819c2992169865dfd500bed5213c8a92ceff5db5e3",
            "proof":["0xe7850052f22562a024314028774ef0c938f75813c7abd280e7fd3920159afe5ca0c0066fdc9ba73c", "0xf84d8080de9c386611e29bf504838cffcd7b643be1ae75a3fd7b14060b980c71704b0a80de9c326a16b8274c5efd1d4634b9a95e6cdd72b32ecb7ddb110b47e1b7300b808080808080808080808080"],
            "value":"0xa"
        }]
    }"#;

    fn verify_storage_proof(account_proof: &str) {
        let acc_proof: AccountProof = serde_json::from_str(account_proof).unwrap();
        let value =
            WitnessProof::<AccountStorageEntry>::from(acc_proof.storage_proof[0].proof.clone())
                .verify(&acc_proof.storage_hash, &acc_proof.storage_proof[0].key)
                .unwrap();
        assert_eq!(value.0, acc_proof.storage_proof[0].value);
    }

    #[test]
    fn verify_works_for_real_storage_proofs() {
        verify_storage_proof(STORAGE_PROOF);
        verify_storage_proof(NON_EXISTENCE_STORAGE_PROOF);
        verify_storage_proof(EXTENSION_NODE_NON_EXISTENCE_STORAGE_PROOF);
        verify_storage_proof(SYNTHETIC_EMBEDDED_STORAGE_PROOF);
    }
}
