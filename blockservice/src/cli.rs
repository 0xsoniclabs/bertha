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

use std::{path::PathBuf, str::FromStr};

use bertha_types::{Hash, HexConvert};
use clap::{Parser, Subcommand};

const DEFAULT_APPLICATION_DIRECTORY: &str = ".";

/// Block Service
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct Args {
    /// The path to the blockservice directory.
    #[arg(long, global = true, default_value = DEFAULT_APPLICATION_DIRECTORY )]
    pub dir: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Initialize a new block database.
    Init,
    /// Import all blocks from the specified snapshot (`.g`) file into the block database, and
    /// optionally also verify the parent hashes.
    ImportGfile {
        gfile: PathBuf,
        #[arg(long, default_value_t = false)]
        verify: bool,
    },
    /// Import all blocks from the specified directory (which is expected to contain `.era1` files)
    /// into the block database, and optionally also verify the parent hashes. The blocks are stored
    /// under the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ImportEra1 {
        era1_dir: PathBuf,
        chain: Chain,
        #[arg(long, default_value_t = false)]
        verify: bool,
    },
    /// Import all blocks from the specified directory (which is expected to contain `.era` files)
    /// into the block database. The blocks are stored under the specified chain, which can be a
    /// name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ImportEra { era_dir: PathBuf, chain: Chain },
    /// Import all blocks from the specified directory (which is expected to contain `.erae` files)
    /// into the block database, and optionally also verify the parent hashes. The blocks are stored
    /// under the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ImportErae {
        erae_dir: PathBuf,
        chain: Chain,
        #[arg(long, default_value_t = false)]
        verify: bool,
    },
    /// Import rules update heights from a JSON file into the block database for the specified
    /// chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ImportRulesUpdateHeights { chain: Chain, file: PathBuf },
    /// Import corrections from a JSON file into the block database for the specified chain, which
    /// can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ImportCorrections { chain: Chain, file: PathBuf },
    /// Fetch blocks from a remote block service and store them in the local database.
    Fetch {
        url: String,
        /// The chain to fetch blocks for. Can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
        chain: Chain,
        #[arg(short, long)]
        /// The first block number in the range to fetch. If not specified, fetching starts from
        /// block 0.
        from: Option<u64>,
        /// The last block number in the range to fetch. If not specified, fetching ends at the
        /// latest available block.
        #[arg(short, long)]
        to: Option<u64>,
    },
    /// Fetch metadata (rules update heights and corrections) from a remote block service and store
    /// them in the local database.
    FetchMetadata { url: String, chain: Chain },
    /// List all block ranges for all chains or only for the specific chain if specified. If url is
    /// not set this lists the locally stored block ranges, otherwise the block ranges of the remote
    /// block service.
    List {
        chain: Option<Chain>,
        #[arg(short, long)]
        url: Option<String>,
    },
    /// Check that all parent hashes match the hash of the parent block starting from the specified
    /// block number with the specified block hash. The chain can be a name (e.g. `sonic`,
    /// `sepolia`) or a numeric ID.
    Verify {
        chain: Chain,
        block_number: Option<u64>,
        #[arg(value_parser(Hash::try_from_hex))]
        block_hash: Option<Hash>,
    },
    /// Delete all blocks of the specified chain, optionally restricted to the range from `from` to
    /// `to`. The chain can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    Purge {
        chain: Chain,
        from: Option<u64>,
        to: Option<u64>,
    },
    /// Print the block as JSON. The chain can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    View { chain: Chain, block_number: u64 },
    /// Print the rules update heights stored in the block database for the specified chain, which
    /// can be a name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ViewRulesUpdateHeights { chain: Chain },
    /// Print the corrections stored in the block database for the specified chain, which can be a
    /// name (e.g. `sonic`, `sepolia`) or a numeric ID.
    ViewCorrections { chain: Chain },
    /// Start the block server.
    Start,
}

/// A chain identifier that can be specified either by a well-known name or by a numeric chain ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chain {
    Id(u64),
    Sonic,
    SonicTestnet,
    Sepolia,
    Hoodi,
}

impl Chain {
    /// Returns the numeric chain ID.
    pub fn to_chain_id(&self) -> u64 {
        match self {
            Chain::Id(id) => *id,
            Chain::Sonic => 146,
            Chain::SonicTestnet => 14601,
            Chain::Sepolia => 11155111,
            Chain::Hoodi => 560048,
        }
    }
}

impl FromStr for Chain {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "sonic" => Ok(Chain::Sonic),
            "sonic-testnet" => Ok(Chain::SonicTestnet),
            "sepolia" => Ok(Chain::Sepolia),
            "hoodi" => Ok(Chain::Hoodi),
            s => s
                .parse::<u64>()
                .map(Chain::Id)
                .map_err(|_| "chain must be a chain ID or a chain name (sonic, sonic-testnet, sepolia, hoodi)".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bertha_types::{Hash, HexConvert};
    use clap::Parser;

    use super::*;

    #[test]
    fn call_with_help_argument_prints_help() {
        let args = ["blockservice", "--help"];
        let expected = "\
Block Service

Usage: blockservice [OPTIONS] <COMMAND>

Commands:
  init                         Initialize a new block database
  import-gfile                 Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  import-era1                  Import all blocks from the specified directory (which is expected to contain `.era1` files) into the block database, and optionally also verify the parent hashes. The blocks are stored under the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  import-era                   Import all blocks from the specified directory (which is expected to contain `.era` files) into the block database. The blocks are stored under the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  import-erae                  Import all blocks from the specified directory (which is expected to contain `.erae` files) into the block database, and optionally also verify the parent hashes. The blocks are stored under the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  import-rules-update-heights  Import rules update heights from a JSON file into the block database for the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  import-corrections           Import corrections from a JSON file into the block database for the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  fetch                        Fetch blocks from a remote block service and store them in the local database
  fetch-metadata               Fetch metadata (rules update heights and corrections) from a remote block service and store them in the local database
  list                         List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service
  verify                       Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash. The chain can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  purge                        Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`. The chain can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  view                         Print the block as JSON. The chain can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  view-rules-update-heights    Print the rules update heights stored in the block database for the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  view-corrections             Print the corrections stored in the block database for the specified chain, which can be a name (e.g. `sonic`, `sepolia`) or a numeric ID
  start                        Start the block server
  help                         Print this message or the help of the given subcommand(s)

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn db_path_can_be_passed_to_root_or_subcommand() {
        // `--dir` is an argument on the root command, but because it is marked as `global`, it
        // can also be used with any subcommand.
        let path = "some/path";
        let expected = Args {
            dir: PathBuf::from(path.to_owned()),
            command: Command::Init,
        };
        let args_cases = [
            ["blockservice", "--dir", path, "init"], // pass path to root command
            ["blockservice", "init", "--dir", path], // pass path to subcommand
        ];
        for args in args_cases {
            parse_and_compare(&args, Ok(expected.clone()));
        }
    }

    #[test]
    fn path_can_be_parsed_from_string() {
        let path = "/path/to/snapshot.g";
        let args = ["blockservice", "import-gfile", path];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_APPLICATION_DIRECTORY),
            command: Command::ImportGfile {
                gfile: PathBuf::from(path),
                verify: false,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[rstest::rstest]
    #[case::name("sonic", Chain::Sonic)]
    #[case::id("146", Chain::Id(146))]
    fn chain_can_be_specified_by_name_or_chain_id(#[case] chain_str: &str, #[case] chain: Chain) {
        let args = ["blockservice", "verify", chain_str];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_APPLICATION_DIRECTORY),
            command: Command::Verify {
                chain,
                block_number: None,
                block_hash: None,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn block_number_is_parsed_as_decimal_and_block_hash_is_parsed_as_hex() {
        let chain_id = 146;
        let block_number = 123456;
        let block_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let args = [
            "blockservice",
            "verify",
            &chain_id.to_string(),
            &block_number.to_string(),
            block_hash,
        ];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_APPLICATION_DIRECTORY),
            command: Command::Verify {
                chain: Chain::Id(chain_id),
                block_number: Some(block_number),
                block_hash: Some(Hash::try_from_hex(block_hash).unwrap()),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    fn trim_whitespace_at_end_of_lines(s: &str) -> String {
        s.split("\n")
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[track_caller]
    fn parse_and_compare(args: &[&str], expected: Result<Args, &str>) {
        let args = Args::try_parse_from(args);
        match (args, expected) {
            (Ok(_), Err(msg)) => {
                panic!(
                    "arguments parsed successfully, but were expected to fail to parse with error:\n{msg}"
                );
            }
            (Err(msg), Ok(_)) => {
                panic!(
                    "arguments were expected to parse successfully but failed to parse with error:\n{msg}"
                );
            }
            (Ok(args), Ok(expected)) => {
                assert_eq!(
                    args, expected,
                    "arguments parsed successfully, but do not match the expected ones"
                );
            }
            (Err(parse_msg), Err(expected_msg)) => {
                let msg = trim_whitespace_at_end_of_lines(&parse_msg.to_string());
                assert_eq!(
                    msg, expected_msg,
                    "arguments failed to parse with error, as expected, but the error message does not match the expected one"
                );
            }
        }
    }

    #[rstest::rstest]
    #[case("sonic", Chain::Sonic)]
    #[case("sonic-testnet", Chain::SonicTestnet)]
    #[case("sepolia", Chain::Sepolia)]
    #[case("hoodi", Chain::Hoodi)]
    #[case("146", Chain::Id(146))]
    fn from_str_parses_known_chains(#[case] input: &str, #[case] expected: Chain) {
        assert_eq!(expected, input.parse::<Chain>().unwrap());
    }

    #[rstest::rstest]
    #[case(Chain::Sonic, 146)]
    #[case(Chain::SonicTestnet, 14601)]
    #[case(Chain::Sepolia, 11155111)]
    #[case(Chain::Hoodi, 560048)]
    #[case(Chain::Id(99), 99)]
    fn to_chain_id_returns_correct_id(#[case] chain: Chain, #[case] expected: u64) {
        assert_eq!(chain.to_chain_id(), expected);
    }

    #[test]
    fn from_str_returns_error_for_invalid_input() {
        assert!("invalid".parse::<Chain>().is_err());
    }
}
