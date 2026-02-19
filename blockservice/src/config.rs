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

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(test)]
use toml_edit::Formatted;
use toml_edit::{Document, DocumentMut, Item};
use tonic::metadata::{Ascii, MetadataValue};

use crate::{Error, grpc::auth};

/// The default config file, used for the implementation of [Config::default].
const DEFAULT_CONFIG_TOML: &str = include_str!("../res/blockservice.toml");

/// A collection of various configuration options for the blockservice.
///
/// The configuration is always associated with a TOML file, which acts as the source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    port: u16,
    #[serde(default)]
    chains: Vec<ChainConfig>,
    #[serde(
        default,
        serialize_with = "serialize_auth_token",
        deserialize_with = "deserialize_auth_token"
    )]
    auth_token: Option<MetadataValue<Ascii>>,
    #[serde(skip)]
    toml: String,
    #[serde(skip)]
    path: PathBuf,
}

fn deserialize_auth_token<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<MetadataValue<Ascii>>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(auth::token_to_metadata_value)
        .transpose()
        .map_err(serde::de::Error::custom)
}

fn serialize_auth_token<S: Serializer>(
    token: &Option<MetadataValue<Ascii>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match token {
        Some(t) => serializer
            .serialize_str(&auth::extract_user_token(t).map_err(serde::ser::Error::custom)?),
        None => serializer.serialize_none(),
    }
}

impl Config {
    /// Creates a new default configuration file at the given path and returns
    /// the corresponding [Config] object.
    ///
    /// Returns an error if the file already exists.
    pub fn create_default(path: impl AsRef<Path>) -> Result<Self, Error> {
        let cfg = Config {
            path: path.as_ref().to_path_buf(),
            ..Config::default()
        };
        let mut file = std::fs::File::create_new(&path)?;
        file.write_all(cfg.toml.as_bytes())?;
        Ok(cfg)
    }

    /// Loads the configuration from the given path.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)?;
        let mut config: Self = toml::from_str(&content)?;
        config.path = path.to_path_buf();
        config.toml = content;
        Ok(config)
    }

    /// Returns the port on which the blockservice should listen.
    pub fn get_port(&self) -> u16 {
        self.port
    }

    /// Returns the authentication token.
    pub fn get_auth_token(&self) -> Option<&MetadataValue<Ascii>> {
        self.auth_token.as_ref()
    }

    /// Sets the authentication token and writes the changes to the config file.
    #[cfg(test)]
    pub fn set_auth_token(
        &mut self,
        auth_token: Option<MetadataValue<Ascii>>,
    ) -> Result<(), Error> {
        self.auth_token = auth_token;

        let mut doc = self.toml.parse::<DocumentMut>()?;
        match &self.auth_token {
            Some(auth_token) => {
                let value = Item::Value(toml_edit::Value::String(Formatted::new(
                    auth::extract_user_token(auth_token).map_err(Error::Config)?,
                )));
                if !doc.contains_key("auth_token") {
                    doc.insert("auth_token", value);
                } else {
                    doc["auth_token"] = value;
                }
            }
            None => {
                doc.remove("auth_token");
            }
        }

        self.toml = doc.to_string();

        let mut file = std::fs::File::create(&self.path)?;
        file.write_all(self.toml.as_bytes())?;

        Ok(())
    }

    /// Returns all configured chain IDs in ascending order.
    pub fn get_chain_ids(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self.chains.iter().map(|c| c.id).collect();
        ids.sort();
        ids
    }

    /// Adds a new chain configuration to the config file.
    pub fn add_chain(&mut self, chain: ChainConfig) -> Result<(), Error> {
        if self.get_chain_config(chain.id).is_some() {
            return Err(Error::Config(format!(
                "chain with id {} already exists",
                chain.id
            )));
        }

        let mut doc = self.toml.parse::<DocumentMut>()?;

        let chain_table = toml::to_string(&chain)?
            .parse::<Document<_>>()?
            .into_table();

        if !doc.contains_key("chains") {
            doc.insert(
                "chains",
                Item::ArrayOfTables(toml_edit::ArrayOfTables::new()),
            );
        }

        doc["chains"]
            .as_array_of_tables_mut()
            .ok_or(Error::Config(
                "expected 'chains' to be an array of tables".to_owned(),
            ))?
            .push(chain_table);

        self.chains.push(chain);
        self.toml = doc.to_string();

        let mut file = std::fs::File::create(&self.path)?;
        file.write_all(self.toml.as_bytes())?;

        Ok(())
    }

    /// Returns the configuration for a chain with the given ID,
    /// or an empty configuration if no such chain exists.
    pub fn get_chain_config(&self, id: u64) -> Option<ChainConfig> {
        self.chains.iter().find(|c| c.id == id).cloned()
    }

    pub fn get_chain_configs(&self) -> &[ChainConfig] {
        &self.chains
    }
}

impl Default for Config {
    fn default() -> Self {
        // Cosmetic: Comments are prefixed with '##' to not be parsed by the unit test below,
        // remove the first '#' before writing to disk (e.g. in [Config::create_default]).
        let config_toml = DEFAULT_CONFIG_TOML.replace("##", "#");

        Config {
            toml: config_toml.clone(),
            path: PathBuf::new(),
            // Safe to unwrap since we control the default config.
            ..toml::from_str(&config_toml).unwrap()
        }
    }
}

/// A set of configuration options for a specific blockchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainConfig {
    pub id: u64,
    pub name: String,
    pub description: String,

    /// Optional JSON RPC endpoint for this chain.
    pub json_rpc: Option<String>,

    /// An optional list of paths to state update files for this chain.
    /// Can be transferred to other blockservice instances using the `fetch-state-updates` command.
    pub state_updates: Option<Vec<PathBuf>>,
}

impl ChainConfig {
    /// Creates a new chain configuration for the given chain ID.
    /// All other fields are initialized to their default values.
    pub fn new(id: u64) -> Self {
        ChainConfig {
            id,
            name: String::default(),
            description: String::default(),
            json_rpc: None,
            state_updates: None,
        }
    }

    pub fn pretty_name(&self) -> String {
        let name = if self.name.is_empty() {
            "(no name)"
        } else {
            &self.name
        };
        let description = if self.description.is_empty() {
            "(no description)"
        } else {
            &self.description
        };
        format!("[{}] {}: {}", self.id, name, description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::test_dir::{Permissions, TestDir};

    #[test]
    fn embedded_default_config_is_valid() {
        // The default config template contains commented out examples.
        // We remove the first '#' to enable them.
        // Actual comments need to be prefixed with '##' to avoid being parsed.
        let config_toml = DEFAULT_CONFIG_TOML.replace("\n#", "\n");
        let config: Config = toml::from_str(&config_toml).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(
            config.chains,
            [ChainConfig {
                id: 133337,
                name: "Example chain".to_string(),
                description: "An example blockchain".to_string(),
                json_rpc: Some("https://example.com/jsonrpc".to_string()),
                state_updates: Some(vec![PathBuf::from("./state_updates_133337.json")])
            }]
        );
    }

    #[test]
    fn auth_token_serialization_and_deserialization_succeed_for_valid_and_non_existing_auth_tokens()
    {
        // Test cases in the form (line to add to config, expected auth token)
        let cases = [
            // valid token
            (
                "auth_token = \"my-token\"\n",
                Some(auth::token_to_metadata_value("my-token").unwrap()),
            ),
            // no token
            ("", None),
        ];
        for (toml_token_line, expected_token) in cases {
            let config_toml = format! {"port = 8080\nchains = []\n{toml_token_line}"};
            let config = toml::from_str::<Config>(&config_toml).unwrap();
            assert_eq!(config.auth_token, expected_token);
            let toml = toml::to_string(&config).unwrap();
            assert_eq!(config_toml, toml);
        }
    }

    #[test]
    fn default_creates_config_from_embedded_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.chains.len(), 0);
    }

    #[test]
    fn create_default_writes_default_config_to_file() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        Config::create_default(&config_path).unwrap();

        let cfg = std::fs::read_to_string(&config_path).unwrap();
        for line in DEFAULT_CONFIG_TOML.lines() {
            let line = line.replacen("##", "#", 1);
            assert!(cfg.contains(&line), "config does not contain line: {line}");
        }
    }

    #[test]
    fn create_default_propagates_filesystem_errors() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        std::fs::File::create(&config_path).unwrap();

        let result = Config::create_default(&config_path);
        assert_eq!(
            result.unwrap_err(),
            Error::Io("File exists (os error 17)".to_owned())
        );
    }

    #[test]
    fn load_loads_config_from_file() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let chain = ChainConfig {
            id: 133337,
            name: "Example chain".to_string(),
            description: "An example blockchain".to_string(),
            json_rpc: Some("https://example.com/jsonrpc".to_string()),
            state_updates: None,
        };
        let my_cfg = Config {
            port: 42,
            chains: vec![chain.clone()],
            auth_token: None,
            path: config_path.clone(),
            toml: String::new(),
        };
        let config_toml = toml::to_string(&my_cfg).unwrap();
        std::fs::write(&config_path, config_toml).unwrap();

        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.port, 42);
        assert_eq!(config.chains, [chain]);
    }

    #[test]
    fn load_propagates_filesystem_errors() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");

        let result = Config::load(&config_path);
        assert_eq!(
            result.unwrap_err(),
            Error::Io("No such file or directory (os error 2)".to_owned())
        );
    }

    #[test]
    fn load_returns_error_for_incomplete_toml() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        std::fs::write(&config_path, "foo = 456").unwrap();

        let result = Config::load(&config_path);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("missing field `port`")
        );
    }

    #[test]
    fn load_returns_error_for_invalid_toml() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        std::fs::write(&config_path, "= 456").unwrap();

        let result = Config::load(&config_path);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unquoted keys cannot be empty")
        );
    }

    #[test]
    fn get_port_returns_port() {
        let config = Config {
            port: 1234,
            ..Config::default()
        };
        assert_eq!(config.get_port(), 1234);
    }

    #[test]
    fn get_auth_token_returns_auth_token() {
        let cases = [
            Some(auth::token_to_metadata_value("my-token").unwrap()),
            None,
        ];
        for auth_token in cases {
            let config = Config {
                auth_token: auth_token.clone(),
                ..Config::default()
            };
            assert_eq!(config.get_auth_token(), auth_token.as_ref());
        }
    }

    #[test]
    fn set_auth_token_sets_token_in_config_and_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        assert_eq!(config.auth_token, None);

        let cases = [
            Some(auth::token_to_metadata_value("my-token").unwrap()), // set token
            Some(auth::token_to_metadata_value("my-token2").unwrap()), // update token
            None,                                                     // remove token
        ];
        for auth_token in cases {
            config.set_auth_token(auth_token.clone()).unwrap();
            assert_eq!(config.auth_token, auth_token);

            // Internal TOML representation is updated
            let stored_config: Config = toml::from_str(&config.toml).unwrap();
            assert_eq!(stored_config.auth_token, auth_token);

            // Change is persisted to file
            let loaded_config = Config::load(&config_path).unwrap();
            assert_eq!(loaded_config.auth_token, auth_token);
        }
    }

    #[test]
    fn get_chain_ids_returns_sorted_chain_ids() {
        let config = Config {
            chains: vec![
                ChainConfig::new(7),
                ChainConfig::new(1),
                ChainConfig::new(3),
            ],
            ..Config::default()
        };
        let ids = config.get_chain_ids();
        assert_eq!(ids, vec![1, 3, 7]);
    }

    #[test]
    fn add_chain_adds_chain_to_config() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        assert_eq!(config.chains.len(), 0);
        let chain = ChainConfig {
            id: 133337,
            name: "Example chain".to_string(),
            description: "An example blockchain".to_string(),
            json_rpc: Some("https://example.com/jsonrpc".to_string()),
            state_updates: None,
        };
        config.add_chain(chain.clone()).unwrap();
        assert_eq!(config.chains.len(), 1);
        assert_eq!(config.chains[0], chain);

        // Internal TOML representation is updated
        let stored_config: Config = toml::from_str(&config.toml).unwrap();
        assert_eq!(stored_config.chains.len(), 1);
        assert_eq!(stored_config.chains[0], chain);

        // Change is persisted to file
        let loaded_config = Config::load(&config_path).unwrap();
        assert_eq!(loaded_config.chains.len(), 1);
        assert_eq!(loaded_config.chains[0], chain);
    }

    #[test]
    fn add_chain_fails_if_chain_with_id_already_exists() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        let chain = ChainConfig::new(42);
        config.add_chain(chain.clone()).unwrap();

        let result = config.add_chain(chain.clone());
        assert_eq!(
            result.unwrap_err(),
            Error::Config(format!("chain with id {} already exists", chain.id))
        );
    }

    #[test]
    fn add_chain_fails_if_toml_field_has_unexpected_type() {
        let tmpdir = TestDir::try_new(Permissions::ReadWrite).unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        config.toml += "\nchains = [42, 123]";
        std::fs::write(&config_path, &config.toml).unwrap();

        let result = config.add_chain(ChainConfig::new(0));
        assert_eq!(
            result.unwrap_err(),
            Error::Config("expected 'chains' to be an array of tables".to_owned())
        );
    }

    #[test]
    fn get_chain_config_returns_chain_config_for_given_id() {
        let chain = ChainConfig::new(42);
        let config = Config {
            chains: vec![chain.clone()],
            ..Default::default()
        };
        let retrieved_chain = config.get_chain_config(42).unwrap();
        assert_eq!(retrieved_chain, chain);

        let non_existent_chain = config.get_chain_config(1337);
        assert!(non_existent_chain.is_none());
    }

    #[test]
    fn get_chain_configs_returns_all_chain_configs() {
        let cases = vec![
            vec![],
            vec![ChainConfig::new(1)],
            vec![ChainConfig::new(1), ChainConfig::new(2)],
        ];
        for chain_configs in cases {
            let config = Config {
                chains: chain_configs.clone(),
                ..Config::default()
            };
            assert_eq!(config.get_chain_configs(), &chain_configs);
        }
    }

    #[test]
    fn chain_config_pretty_name_formats_correctly() {
        let chain = ChainConfig {
            name: "Test Chain".to_string(),
            description: "A test blockchain".to_string(),
            ..ChainConfig::new(1337)
        };
        assert_eq!(chain.pretty_name(), "[1337] Test Chain: A test blockchain");

        let chain_no_name = ChainConfig {
            description: "Unnamed chain".to_string(),
            ..ChainConfig::new(42)
        };
        assert_eq!(chain_no_name.pretty_name(), "[42] (no name): Unnamed chain");

        let chain_no_description = ChainConfig {
            name: "Test Chain".to_string(),
            ..ChainConfig::new(1)
        };
        assert_eq!(
            chain_no_description.pretty_name(),
            "[1] Test Chain: (no description)"
        );
    }
}
