use std::path::PathBuf;

use bertha_types::{Hash, HexConvert};
use clap::{Parser, Subcommand};

/// Block Service
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Initialize a new block database in the current directory or at the specified path.
    Init {
        /// The path to the block database. Defaults to the current working directory.
        path: Option<PathBuf>,
    },
    /// Import all blocks from the specified snapshot (`.g`) file into the block database, and
    /// optionally also verify the parent hashes.
    Import {
        snapshot_file: PathBuf,
        #[arg(long, default_value_t = false)]
        verify: bool,
    },
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
    /// List all locally stored block ranges for all chains or only for the specific chain if
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
    /// Print the block as JSON.
    View { chain_id: u64, block_number: u64 },
    /// Start the block server.
    Start {
        #[arg(default_value_t = 8080)]
        port: u16,
    },
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use bertha_types::{Hash, HexConvert};
    use clap::Parser;

    use super::*;

    #[test]
    fn call_without_arguments_prints_usage() {
        let args = ["blockservice"];
        let expected = "\
Block Service

Usage: blockservice <COMMAND>

Commands:
  init    Initialize a new block database in the current directory or at the specified path
  import  Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  fetch   Fetch blocks from a remote block service and store them in the local database
  list    List all locally stored block ranges for all chains or only for the specific chain if specified
  verify  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  clean   Delete all blocks for chains not referenced in the config file
  view    Print the block as JSON
  start   Start the block server
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_invalid_subcommand_prints_parse_error() {
        let args = ["blockservice", "invalid"];
        let expected = "\
error: unrecognized subcommand 'invalid'

Usage: blockservice <COMMAND>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_help_argument_prints_help() {
        let args = ["blockservice", "--help"];
        let expected = "\
Block Service

Usage: blockservice <COMMAND>

Commands:
  init    Initialize a new block database in the current directory or at the specified path
  import  Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  fetch   Fetch blocks from a remote block service and store them in the local database
  list    List all locally stored block ranges for all chains or only for the specific chain if specified
  verify  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  clean   Delete all blocks for chains not referenced in the config file
  view    Print the block as JSON
  start   Start the block server
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_init_subcommand_without_argument_parses_successfully() {
        let args = ["blockservice", "init"];
        let expected = Args {
            command: Command::Init { path: None },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_path_parses_successfully() {
        let path = "/path/to/database";
        let args = ["blockservice", "init", path];
        let expected = Args {
            command: Command::Init {
                path: Some(PathBuf::from(path)),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "init", "--help"];
        let expected = "\
Initialize a new block database in the current directory or at the specified path

Usage: blockservice init [PATH]

Arguments:
  [PATH]  The path to the block database. Defaults to the current working directory

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "init", "/path/to/database", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice init [PATH]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_import_subcommand_without_argument_prints_parse_error() {
        let args = ["blockservice", "import"];
        let expected = "\
error: the following required arguments were not provided:
  <SNAPSHOT_FILE>

Usage: blockservice import <SNAPSHOT_FILE>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_import_subcommand_with_path_parses_successfully() {
        let path = "/path/to/snapshot.g";
        let args = ["blockservice", "import", path];
        let expected = Args {
            command: Command::Import {
                snapshot_file: PathBuf::from(path),
                verify: false,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_import_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "import", "--help"];
        let expected = "\
Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes

Usage: blockservice import [OPTIONS] <SNAPSHOT_FILE>

Arguments:
  <SNAPSHOT_FILE>

Options:
      --verify
  -h, --help    Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_import_subcommand_with_additional_argument_prints_parse_error() {
        let args = [
            "blockservice",
            "import",
            "/path/to/snapshot.g",
            "additional",
        ];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice import [OPTIONS] <SNAPSHOT_FILE>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_list_subcommand_without_argument_parses_successfully() {
        let args = ["blockservice", "list"];
        let expected = Args {
            command: Command::List { chain_id: None },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_chain_id_parses_successfully() {
        let chain_id = 146;
        let args = ["blockservice", "list", &chain_id.to_string()];
        let expected = Args {
            command: Command::List {
                chain_id: Some(chain_id),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "list", "--help"];
        let expected = "\
List all locally stored block ranges for all chains or only for the specific chain if specified

Usage: blockservice list [CHAIN_ID]

Arguments:
  [CHAIN_ID]

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_invalid_argument_prints_parse_error() {
        let args = ["blockservice", "list", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '[CHAIN_ID]': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "list", "146", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice list [CHAIN_ID]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_verify_subcommand_without_argument_prints_parse_error() {
        let args = ["blockservice", "verify"];
        let expected = "\
error: the following required arguments were not provided:
  <CHAIN_ID>

Usage: blockservice verify <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

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
            "blockservice",
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
        let args = ["blockservice", "verify", "--help"];
        let expected = "\
Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash

Usage: blockservice verify <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

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
        let args = ["blockservice", "verify", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '<CHAIN_ID>': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_verify_subcommand_with_additional_argument_prints_parse_error() {
        let args = [
            "blockservice",
            "verify",
            "146",
            "123456",
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "additional",
        ];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice verify <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_without_argument_prints_parse_error() {
        let args = ["blockservice", "purge"];
        let expected = "\
error: the following required arguments were not provided:
  <CHAIN_ID>

Usage: blockservice purge <CHAIN_ID> [FROM] [TO]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_chain_id_parses_successfully() {
        let chain_id = 146;
        let args = ["blockservice", "purge", &chain_id.to_string()];
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
            "blockservice",
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
        let args = ["blockservice", "purge", "--help"];
        let expected = "\
Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`

Usage: blockservice purge <CHAIN_ID> [FROM] [TO]

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
        let args = ["blockservice", "purge", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '<CHAIN_ID>': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_purge_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "purge", "146", "1000", "2000", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice purge <CHAIN_ID> [FROM] [TO]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_clean_subcommand_without_argument_parses_successfully() {
        let args = ["blockservice", "clean"];
        let expected = Args {
            command: Command::Clean,
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_clean_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "clean", "--help"];
        let expected = "\
Delete all blocks for chains not referenced in the config file

Usage: blockservice clean

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_clean_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "clean", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice clean

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_view_subcommand_without_argument_prints_parse_error() {
        let args = ["blockservice", "view"];
        let expected = "\
error: the following required arguments were not provided:
  <CHAIN_ID>
  <BLOCK_NUMBER>

Usage: blockservice view <CHAIN_ID> <BLOCK_NUMBER>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_view_subcommand_with_chain_id_and_block_number_parses_successfully() {
        let chain_id = 146;
        let block_number = 123456;
        let args = [
            "blockservice",
            "view",
            &chain_id.to_string(),
            &block_number.to_string(),
        ];
        let expected = Args {
            command: Command::View {
                chain_id,
                block_number,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_view_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "view", "--help"];
        let expected = "\
Print the block as JSON

Usage: blockservice view <CHAIN_ID> <BLOCK_NUMBER>

Arguments:
  <CHAIN_ID>
  <BLOCK_NUMBER>

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_view_subcommand_with_invalid_argument_prints_parse_error() {
        let args = ["blockservice", "view", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '<CHAIN_ID>': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_view_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "view", "1", "0", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice view <CHAIN_ID> <BLOCK_NUMBER>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_start_subcommand_without_argument_parses_successfully() {
        let args = ["blockservice", "start"];
        let expected = Args {
            command: Command::Start { port: 8080 },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_start_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "start", "--help"];
        let expected = "\
Start the block server

Usage: blockservice start [PORT]

Arguments:
  [PORT]  [default: 8080]

Options:
  -h, --help  Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_start_subcommand_with_invalid_port_prints_parse_error() {
        let args = ["blockservice", "start", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '[PORT]': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_start_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "start", "8080", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice start [PORT]

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_fetch_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "fetch", "--help"];
        let expected = "\
        Fetch blocks from a remote block service and store them in the local database

Usage: blockservice fetch [OPTIONS] <URL> <CHAIN_ID>

Arguments:
  <URL>
  <CHAIN_ID>

Options:
  -f, --from <FROM>  The first block number in the range to fetch. If not specified, fetching starts from block 0
  -t, --to <TO>      The last block number in the range to fetch. If not specified, fetching ends at the latest available block
  -h, --help         Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_fetch_subcommand_without_arguments_prints_parse_error() {
        let args = ["blockservice", "fetch"];
        let expected = "\
error: the following required arguments were not provided:
  <URL>
  <CHAIN_ID>

Usage: blockservice fetch <URL> <CHAIN_ID>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_fetch_subcommand_without_chain_id_prints_parse_error() {
        let args = ["blockservice", "fetch", "http://example.com"];
        let expected = "\
error: the following required arguments were not provided:
  <CHAIN_ID>

Usage: blockservice fetch <URL> <CHAIN_ID>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_fetch_subcommand_with_invalid_chain_id_prints_parse_error() {
        let args = ["blockservice", "fetch", "http://example.com", "invalid"];
        let expected = "\
error: invalid value 'invalid' for '<CHAIN_ID>': invalid digit found in string

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_fetch_subcommand_with_all_arguments_parses_successfully() {
        let url = "http://example.com";
        let chain_id = 146;
        let from = 1000;
        let to = 2000;
        // No `from` or `to` arguments
        let args = ["blockservice", "fetch", url, &chain_id.to_string()];
        let expected = Args {
            command: Command::Fetch {
                url: url.to_string(),
                chain_id,
                from: None,
                to: None,
            },
        };
        parse_and_compare(&args, Ok(expected));

        // With `from` argument
        let args = [
            "blockservice",
            "fetch",
            url,
            &chain_id.to_string(),
            "--from",
            &from.to_string(),
        ];
        let expected = Args {
            command: Command::Fetch {
                url: url.to_string(),
                chain_id,
                from: Some(from),
                to: None,
            },
        };
        parse_and_compare(&args, Ok(expected));

        // With `to` argument
        let args = [
            "blockservice",
            "fetch",
            url,
            &chain_id.to_string(),
            "--to",
            &to.to_string(),
        ];
        let expected = Args {
            command: Command::Fetch {
                url: url.to_string(),
                chain_id,
                from: None,
                to: Some(to),
            },
        };
        parse_and_compare(&args, Ok(expected));

        // With both `from` and `to` arguments
        let args = [
            "blockservice",
            "fetch",
            url,
            &chain_id.to_string(),
            "--from",
            &from.to_string(),
            "--to",
            &to.to_string(),
        ];
        let expected = Args {
            command: Command::Fetch {
                url: url.to_string(),
                chain_id,
                from: Some(from),
                to: Some(to),
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
