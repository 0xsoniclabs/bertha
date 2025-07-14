use std::{
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use toml_edit::{Document, DocumentMut, Item};

use crate::Error;

const DEFAULT_CONFIG_TOML: &str = include_str!("../res/blockservice.toml");

/// A collection of various configuration options for the blockservice.
///
/// The configuration is always associated with a TOML file, which acts as the source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    port: u16,
    #[serde(default)]
    chains: Vec<ChainConfig>,

    #[serde(skip)]
    toml: String,
    #[serde(skip)]
    path: PathBuf,
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

    /// Returns all configured chain IDs in ascending order.
    pub fn get_chain_ids(&self) -> Vec<u64> {
        let mut ids: Vec<_> = self.chains.iter().map(|c| c.id).collect();
        ids.sort();
        ids
    }

    /// Returns `true` if the config contains a chain with the given ID.
    pub fn has_chain(&self, id: u64) -> bool {
        self.chains.iter().any(|c| c.id == id)
    }

    /// Adds a new chain configuration to the config file.
    pub fn add_chain(&mut self, chain: ChainConfig) -> Result<(), Error> {
        if self.has_chain(chain.id) {
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
    pub fn get_chain_config(&self, id: u64) -> ChainConfig {
        self.chains
            .iter()
            .find(|c| c.id == id)
            .unwrap_or(&ChainConfig {
                id,
                ..Default::default()
            })
            .clone()
    }
}

impl Default for Config {
    fn default() -> Self {
        // Comments are prefixed with '##' to not be parsed by the unit test below.
        let config_toml = DEFAULT_CONFIG_TOML
            .lines()
            .map(|line| line.replacen("##", "#", 1))
            .collect::<Vec<_>>()
            .join("\n");

        Config {
            toml: config_toml.clone(),
            path: PathBuf::new(),
            // Safe to unwrap since we control the default config.
            ..toml::from_str(&config_toml).unwrap()
        }
    }
}

/// A set of configuration options for a specific blockchain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainConfig {
    pub id: u64,
    pub name: String,
    pub description: String,
}

impl ChainConfig {
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

    #[test]
    fn embedded_default_config_is_valid() {
        // The default config template contains commented out examples.
        // We remove the first '#' to enable them.
        // Actual comments need to be prefixed with '##' to avoid being parsed.
        let config_toml = DEFAULT_CONFIG_TOML
            .lines()
            .map(|line| line.replacen("#", "", 1))
            .collect::<Vec<_>>()
            .join("\n");
        let config: Config = toml::from_str(&config_toml).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(config.chains.len(), 1);
        assert_eq!(
            config.chains[0],
            ChainConfig {
                id: 133337,
                name: "Example chain".to_string(),
                description: "An example blockchain".to_string(),
            }
        );
    }

    #[test]
    fn default_creates_config_from_embedded_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.chains.len(), 0);
    }

    #[test]
    fn create_default_writes_default_config_to_file() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        Config::create_default(&config_path).unwrap();

        let cfg = std::fs::read_to_string(&config_path).unwrap();
        for line in DEFAULT_CONFIG_TOML.lines() {
            let line = line.replacen("##", "#", 1);
            assert!(cfg.contains(&line), "config does not contain line: {line}");
        }
    }

    #[test]
    fn create_default_fails_if_file_exists() {
        let tmpdir = tempfile::tempdir().unwrap();
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
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let chain = ChainConfig {
            id: 133337,
            name: "Example chain".to_string(),
            description: "An example blockchain".to_string(),
        };
        let my_cfg = Config {
            port: 42,
            chains: vec![chain.clone()],
            path: config_path.clone(),
            toml: String::new(),
        };
        let config_toml = toml::to_string(&my_cfg).unwrap();
        std::fs::write(&config_path, config_toml).unwrap();

        let config = Config::load(&config_path).unwrap();
        assert_eq!(config.port, 42);
        assert_eq!(config.chains.len(), 1);
        assert_eq!(config.chains[0], chain);
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
    fn get_chain_ids_returns_sorted_chain_ids() {
        let chain1 = ChainConfig {
            id: 7,
            ..Default::default()
        };
        let chain2 = ChainConfig {
            id: 1,
            ..Default::default()
        };
        let chain3 = ChainConfig {
            id: 3,
            ..Default::default()
        };
        let config = Config {
            chains: vec![chain1, chain2, chain3],
            ..Config::default()
        };
        let ids = config.get_chain_ids();
        assert_eq!(ids, vec![1, 3, 7]);
    }

    #[test]
    fn has_chain_returns_true_if_chain_exists() {
        let chain = ChainConfig {
            id: 42,
            ..Default::default()
        };
        let config = Config {
            chains: vec![chain.clone()],
            ..Config::default()
        };
        assert!(config.has_chain(42));
        assert!(!config.has_chain(1337));
    }

    #[test]
    fn add_chain_adds_chain_to_config() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        assert_eq!(config.chains.len(), 0);
        let chain = ChainConfig {
            id: 133337,
            name: "Example chain".to_string(),
            description: "An example blockchain".to_string(),
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
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        let chain = ChainConfig::default();
        config.add_chain(chain.clone()).unwrap();

        let result = config.add_chain(chain.clone());
        assert_eq!(
            result.unwrap_err(),
            Error::Config(format!("chain with id {} already exists", chain.id))
        );
    }

    #[test]
    fn add_chain_fails_if_toml_field_has_unexpected_type() {
        let tmpdir = tempfile::tempdir().unwrap();
        let config_path = tmpdir.path().join("blockservice.toml");
        let mut config = Config::create_default(&config_path).unwrap();
        config.toml += "\nchains = [42, 123]";
        std::fs::write(&config_path, &config.toml).unwrap();

        let result = config.add_chain(ChainConfig::default());
        assert_eq!(
            result.unwrap_err(),
            Error::Config("expected 'chains' to be an array of tables".to_owned())
        );
    }

    #[test]
    fn get_chain_config_returns_chain_config_for_given_id() {
        let chain = ChainConfig {
            id: 42,
            name: "Test Chain".to_string(),
            description: "A test blockchain".to_string(),
        };
        let config = Config {
            chains: vec![chain.clone()],
            ..Default::default()
        };
        let retrieved_chain = config.get_chain_config(42);
        assert_eq!(retrieved_chain, chain);
    }

    #[test]
    fn get_chain_config_returns_default_if_chain_does_not_exist() {
        let config = Config::default();
        let chain = config.get_chain_config(999);
        let expected = ChainConfig {
            id: 999,
            ..Default::default()
        };
        assert_eq!(chain, expected);
    }

    #[test]
    fn chain_config_pretty_name_formats_correctly() {
        let chain = ChainConfig {
            id: 1337,
            name: "Test Chain".to_string(),
            description: "A test blockchain".to_string(),
        };
        assert_eq!(chain.pretty_name(), "[1337] Test Chain: A test blockchain");

        let chain_no_name = ChainConfig {
            id: 42,
            name: "".to_string(),
            description: "Unnamed chain".to_string(),
        };
        assert_eq!(chain_no_name.pretty_name(), "[42] (no name): Unnamed chain");

        let chain_no_description = ChainConfig {
            id: 1,
            name: "Test Chain".to_string(),
            description: "".to_string(),
        };
        assert_eq!(
            chain_no_description.pretty_name(),
            "[1] Test Chain: (no description)"
        );
    }
}
