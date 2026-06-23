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

use std::path::PathBuf;

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
    /// under the specified chain ID.
    ImportEra1 {
        era1_dir: PathBuf,
        chain_id: u64,
        #[arg(long, default_value_t = false)]
        verify: bool,
    },
    /// Import all blocks from the specified directory (which is expected to contain `.era` files)
    /// into the block database. The blocks are stored under the specified chain ID.
    ImportEra { era_dir: PathBuf, chain_id: u64 },
    /// Import rules update heights from a JSON file into the block database for the specified chain
    /// ID.
    ImportRulesUpdateHeights { chain_id: u64, file: PathBuf },
    /// Import corrections from a JSON file into the block database for the specified chain ID.
    ImportCorrections { chain_id: u64, file: PathBuf },
    /// Fetch blocks from a remote block service and store them in the local database.
    Fetch {
        url: String,
        chain_id: u64,
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
    FetchMetadata { url: String, chain_id: u64 },
    /// List all block ranges for all chains or only for the specific chain if specified. If url is
    /// not set this lists the locally stored block ranges, otherwise the block ranges of the remote
    /// block service.
    List {
        chain_id: Option<u64>,
        #[arg(short, long)]
        url: Option<String>,
    },
    /// Check that all parent hashes match the hash of the parent block starting from the specified
    /// block number with the specified block hash.
    Verify {
        chain_id: u64,
        block_number: Option<u64>,
        #[arg(value_parser(Hash::try_from_hex))]
        block_hash: Option<Hash>,
    },
    /// Delete all blocks of the specified chain, optionally restricted to the range from `from` to
    /// `to`.
    Purge {
        chain_id: u64,
        from: Option<u64>,
        to: Option<u64>,
    },
    /// Print the block as JSON.
    View { chain_id: u64, block_number: u64 },
    /// Print the rules update heights stored in the block database for the specified chain ID.
    ViewRulesUpdateHeights { chain_id: u64 },
    /// Print the corrections stored in the block database for the specified chain ID.
    ViewCorrections { chain_id: u64 },
    /// Start the block server.
    Start,
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
  import-era1                  Import all blocks from the specified directory (which is expected to contain `.era1` files) into the block database, and optionally also verify the parent hashes. The blocks are stored under the specified chain ID
  import-era                   Import all blocks from the specified directory (which is expected to contain `.era` files) into the block database. The blocks are stored under the specified chain ID
  import-rules-update-heights  Import rules update heights from a JSON file into the block database for the specified chain ID
  import-corrections           Import corrections from a JSON file into the block database for the specified chain ID
  fetch                        Fetch blocks from a remote block service and store them in the local database
  fetch-metadata               Fetch metadata (rules update heights and corrections) from a remote block service and store them in the local database
  list                         List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service
  verify                       Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge                        Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  view                         Print the block as JSON
  view-rules-update-heights    Print the rules update heights stored in the block database for the specified chain ID
  view-corrections             Print the corrections stored in the block database for the specified chain ID
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
                chain_id,
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
}
