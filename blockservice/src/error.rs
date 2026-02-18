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

use prost::DecodeError;

use crate::app_dir::AppDirError;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("application directory error: {0}")]
    AppDir(#[from] AppDirError),
    #[error("error in underlying storage layer: {0}")]
    StorageLayer(String),
    #[error("error during protobuf decoding: {0}")]
    Protobuf(#[from] DecodeError),
    #[error("conversion from generic representation to Rust type failed")]
    TypeConversion,
    // std::io::Error is not PartialEq + Eq, so we cannot wrap it directly
    #[error("I/O error: {0}")]
    Io(String),
    #[error("configuration error: {0}")]
    Config(String),
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(err: toml::ser::Error) -> Self {
        Error::Config(format!("TOML serialization failed: {err}"))
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Error::Config(format!("TOML deserialization failed: {err}"))
    }
}

impl From<toml_edit::TomlError> for Error {
    fn from(err: toml_edit::TomlError) -> Self {
        Error::Config(format!("TOML edit failed: {err}"))
    }
}
