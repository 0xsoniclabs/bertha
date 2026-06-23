// Copyright 2026 Sonic Operations Ltd
// This file is part of the Bertha testing infrastructure for Sonic.
//
// Bertha is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// Bertha is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with Bertha. If not, see <http://www.gnu.org/licenses/>.

use reth_era::e2s::error::E2sError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("rlp decoding failed: {0}")]
    Rlp(#[from] alloy_rlp::Error),
    #[error("an io error occurred: {0}")]
    Io(#[from] std::io::Error),
    #[error("gzip decompression failed: {0}")]
    Decompression(#[from] flate2::DecompressError),
    #[error("`.g` file validation failed: {0}")]
    GFile(#[from] GFileError),
    #[error("era file parsing failed: {0}")]
    Era(String),
    #[error("E2S error: {0}")]
    E2S(#[from] E2sError),
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum GFileError {
    #[error("header missing")]
    HeaderMissing,
    #[error("invalid header: got {got:?}, expected {expected:?}")]
    InvalidHeader { got: [u8; 4], expected: [u8; 4] },
    #[error("header mismatch")]
    HeaderMismatch,
    #[error("invalid file version: got {got:?}, expected {expected:?}")]
    InvalidFileVersion { got: [u8; 4], expected: [u8; 4] },
    #[error("blocks unit missing")]
    BlocksUnitMissing,
    #[error("piece size too large: got {got}, max {max}")]
    PieceSizeTooLarge { got: usize, max: usize },
}
