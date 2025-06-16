use std::path::PathBuf;

use bertha_types::{Hash, HexConvert};
use clap::{Parser, Subcommand};

/// Block Server
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Initialize a new block database in the current directory or at the specified path.
    Init {
        /// The path to the block database. Defaults to the current working directory.
        path: Option<PathBuf>,
    },
    /// Import all blocks from the specified snapshot (`.g`) file into the block database.
    Import { snapshot_file: PathBuf },
    /// Lists all locally stored block ranges for all chains or only for the specific chain if
    /// specified.
    List { chain_id: Option<u64> },
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
    /// Delete all blocks for chains not referenced in the config file.
    Clean,
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
    fn call_without_arguments_prints_usage() {
        let args = ["blockserver"];
        let expected = "\
Block Server

Usage: blockserver <COMMAND>

Commands:
  init    Initialize a new block database in the current directory or at the specified path
  import  Import all blocks from the specified snapshot (`.g`) file into the block database
  list    Lists all locally stored block ranges for all chains or only for the specific chain if specified
  verify  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  clean   Delete all blocks for chains not referenced in the config file
  start   Start the block server
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_invalid_subcommand_prints_parse_error() {
        let args = ["blockserver", "invalid"];
        let expected = "\
error: unrecognized subcommand 'invalid'

Usage: blockserver <COMMAND>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_help_argument_prints_help() {
        let args = ["blockserver", "--help"];
        let expected = "\
Block Server

Usage: blockserver <COMMAND>

Commands:
  init    Initialize a new block database in the current directory or at the specified path
  import  Import all blocks from the specified snapshot (`.g`) file into the block database
  list    Lists all locally stored block ranges for all chains or only for the specific chain if specified
  verify  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  clean   Delete all blocks for chains not referenced in the config file
  start   Start the block server
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_init_subcommand_without_argument_parses_successfully() {
        let args = ["blockserver", "init"];
        let expected = Args {
            command: Command::Init { path: None },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_path_parses_successfully() {
        let path = "/path/to/database";
        let args = ["blockserver", "init", path];
        let expected = Args {
            command: Command::Init {
                path: Some(PathBuf::from(path)),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "init", "--help"];
        let expected = "\
Initialize a new block database in the current directory or at the specified path

Usage: blockserver init [PATH]

Arguments:
  [PATH]  The path to the block database. Defaults to the current working directory

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockserver", "init", "/path/to/database", "invalid"];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver init [PATH]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_import_subcommand_without_argument_prints_parse_error() {
        let args = ["blockserver", "import"];
        let expected = "\
error: the following required arguments were not provided:
  <SNAPSHOT_FILE>

Usage: blockserver import <SNAPSHOT_FILE>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_import_subcommand_with_path_parses_successfully() {
        let path = "/path/to/snapshot.g";
        let args = ["blockserver", "import", path];
        let expected = Args {
            command: Command::Import {
                snapshot_file: PathBuf::from(path),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_import_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "import", "--help"];
        let expected = "\
Import all blocks from the specified snapshot (`.g`) file into the block database

Usage: blockserver import <SNAPSHOT_FILE>

Arguments:
  <SNAPSHOT_FILE>

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_import_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockserver", "import", "/path/to/snapshot.g", "invalid"];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver import <SNAPSHOT_FILE>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_list_subcommand_without_argument_parses_successfully() {
        let args = ["blockserver", "list"];
        let expected = Args {
            command: Command::List { chain_id: None },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_chain_id_parses_successfully() {
        let chain_id = 146;
        let args = ["blockserver", "list", &chain_id.to_string()];
        let expected = Args {
            command: Command::List {
                chain_id: Some(chain_id),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "list", "--help"];
        let expected = "\
Lists all locally stored block ranges for all chains or only for the specific chain if specified

Usage: blockserver list [CHAIN_ID]

Arguments:
  [CHAIN_ID]

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_invalid_argument_prints_parse_error() {
        let args = ["blockserver", "list", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '[CHAIN_ID]': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockserver", "list", "146", "invalid"];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver list [CHAIN_ID]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_verify_subcommand_without_argument_prints_parse_error() {
        let args = ["blockserver", "verify"];
        let expected = "\
error: the following required arguments were not provided:
  <CHAIN_ID>

Usage: blockserver verify <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_verify_subcommand_with_id_and_number_and_hash_parses_successfully() {
        let chain_id = 146;
        let block_number = 123456;
        let block_hash = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let args = [
            "blockserver",
            "verify",
            &chain_id.to_string(),
            &block_number.to_string(),
            block_hash,
        ];
        let expected = Args {
            command: Command::Verify {
                chain_id,
                block_number: Some(block_number),
                block_hash: Some(Hash::try_from_hex(block_hash).unwrap()),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_verify_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "verify", "--help"];
        let expected = "\
Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash

Usage: blockserver verify <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

Arguments:
  <CHAIN_ID>
  [BLOCK_NUMBER]
  [BLOCK_HASH]

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_verify_subcommand_with_invalid_argument_prints_parse_error() {
        let args = ["blockserver", "verify", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '<CHAIN_ID>': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_verify_subcommand_with_additional_argument_prints_parse_error() {
        let args = [
            "blockserver",
            "verify",
            "146",
            "123456",
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "invalid",
        ];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver verify <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_without_argument_prints_parse_error() {
        let args = ["blockserver", "purge"];
        let expected = "\
error: the following required arguments were not provided:
  <CHAIN_ID>

Usage: blockserver purge <CHAIN_ID> [FROM] [TO]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_chain_id_parses_successfully() {
        let chain_id = 146;
        let args = ["blockserver", "purge", &chain_id.to_string()];
        let expected = Args {
            command: Command::Purge {
                chain_id,
                from: None,
                to: None,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_chain_id_and_from_and_to_parses_successfully() {
        let chain_id = 146;
        let from = 1000;
        let to = 2000;
        let args = [
            "blockserver",
            "purge",
            &chain_id.to_string(),
            &from.to_string(),
            &to.to_string(),
        ];
        let expected = Args {
            command: Command::Purge {
                chain_id,
                from: Some(from),
                to: Some(to),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "purge", "--help"];
        let expected = "\
Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`

Usage: blockserver purge <CHAIN_ID> [FROM] [TO]

Arguments:
  <CHAIN_ID>
  [FROM]
  [TO]

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_invalid_argument_prints_parse_error() {
        let args = ["blockserver", "purge", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '<CHAIN_ID>': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockserver", "purge", "146", "1000", "2000", "invalid"];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver purge <CHAIN_ID> [FROM] [TO]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_clean_subcommand_without_argument_parses_successfully() {
        let args = ["blockserver", "clean"];
        let expected = Args {
            command: Command::Clean,
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_clean_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "clean", "--help"];
        let expected = "\
Delete all blocks for chains not referenced in the config file

Usage: blockserver clean

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_clean_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockserver", "clean", "invalid"];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver clean

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_start_subcommand_without_argument_parses_successfully() {
        let args = ["blockserver", "start"];
        let expected = Args {
            command: Command::Start,
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_start_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockserver", "start", "--help"];
        let expected = "\
Start the block server

Usage: blockserver start

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_start_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockserver", "start", "invalid"];
        let expected = "\
error: unexpected argument 'invalid' found

Usage: blockserver start

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    fn trim_whitespace_at_end_of_lines(s: &str) -> String {
        s.split("\n")
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
    }

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
                assert_eq!(args, expected);
            }
            (Err(parse_msg), Err(expected_msg)) => {
                let help: String = trim_whitespace_at_end_of_lines(&parse_msg.to_string());
                assert_eq!(help, expected_msg);
            }
        }
    }
}
