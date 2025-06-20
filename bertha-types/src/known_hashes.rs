/// The hash of the RLP encoding of the empty list of ommers.
/// The Ommers Hash field is now deprecated and is always the constant KEC(RLP(()))
pub const EMPTY_SHA3_OMMERS_HASH: &str =
    "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347";

/// The hash of the root node of an empty Merkle-Patricia Trie.
pub const EMPTY_TREE_ROOT_HASH: &str =
    "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421";
