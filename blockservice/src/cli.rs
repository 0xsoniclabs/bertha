use std::path::PathBuf;

use bertha_types::{Hash, HexConvert};
use clap::{Parser, Subcommand};

const DEFAULT_BLOCKSERIVCE_PATH: &str = ".";

/// Block Service
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
pub struct Args {
    /// The path to the blockservice directory.
    #[arg(long, global = true, default_value = DEFAULT_BLOCKSERIVCE_PATH )]
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

Usage: blockservice [OPTIONS] <COMMAND>

Commands:
  init    Initialize a new block database
  import  Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  fetch   Fetch blocks from a remote block service and store them in the local database
  list    List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service
  verify  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  view    Print the block as JSON
  start   Start the block server
  help    Print this message or the help of the given subcommand(s)

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_invalid_subcommand_prints_parse_error() {
        let args = ["blockservice", "invalid"];
        let expected = "\
error: unrecognized subcommand 'invalid'

Usage: blockservice [OPTIONS] <COMMAND>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_help_argument_prints_help() {
        let args = ["blockservice", "--help"];
        let expected = "\
Block Service

Usage: blockservice [OPTIONS] <COMMAND>

Commands:
  init    Initialize a new block database
  import  Import all blocks from the specified snapshot (`.g`) file into the block database, and optionally also verify the parent hashes
  fetch   Fetch blocks from a remote block service and store them in the local database
  list    List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service
  verify  Check that all parent hashes match the hash of the parent block starting from the specified block number with the specified block hash
  purge   Delete all blocks of the specified chain, optionally restricted to the range from `from` to `to`
  view    Print the block as JSON
  start   Start the block server
  help    Print this message or the help of the given subcommand(s)

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
    fn call_with_init_subcommand_without_argument_parses_successfully() {
        let args = ["blockservice", "init"];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
            command: Command::Init,
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "init", "--help"];
        let expected = "\
Initialize a new block database

Usage: blockservice init [OPTIONS]

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_init_subcommand_with_additional_argument_prints_parse_error() {
        let args = ["blockservice", "init", "additional"];
        let expected = "\
error: unexpected argument 'additional' found

Usage: blockservice init [OPTIONS]

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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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
      --dir <DIR>  The path to the blockservice directory [default: .]
      --verify
  -h, --help       Print help
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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
            command: Command::List {
                chain_id: None,
                url: None,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_chain_id_parses_successfully() {
        let chain_id = 146;
        let args = ["blockservice", "list", &chain_id.to_string()];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
            command: Command::List {
                chain_id: Some(chain_id),
                url: None,
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_chain_id_and_url_parses_successfully() {
        let chain_id = 146;
        let url = "http://example.com";
        let args = ["blockservice", "list", &chain_id.to_string(), "--url", url];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
            command: Command::List {
                chain_id: Some(chain_id),
                url: Some(url.to_string()),
            },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_list_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "list", "--help"];
        let expected = "\
List all block ranges for all chains or only for the specific chain if specified. If url is not set this lists the locally stored block ranges, otherwise the block ranges of the remote block service

Usage: blockservice list [OPTIONS] [CHAIN_ID]

Arguments:
  [CHAIN_ID]

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -u, --url <URL>
  -h, --help       Print help
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

Usage: blockservice list [OPTIONS] [CHAIN_ID]

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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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

Usage: blockservice verify [OPTIONS] <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

Arguments:
  <CHAIN_ID>
  [BLOCK_NUMBER]
  [BLOCK_HASH]

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
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

Usage: blockservice verify [OPTIONS] <CHAIN_ID> [BLOCK_NUMBER] [BLOCK_HASH]

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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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

Usage: blockservice purge [OPTIONS] <CHAIN_ID> [FROM] [TO]

Arguments:
  <CHAIN_ID>
  [FROM]
  [TO]

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
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

Usage: blockservice purge [OPTIONS] <CHAIN_ID> [FROM] [TO]

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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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

Usage: blockservice view [OPTIONS] <CHAIN_ID> <BLOCK_NUMBER>

Arguments:
  <CHAIN_ID>
  <BLOCK_NUMBER>

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
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

Usage: blockservice view [OPTIONS] <CHAIN_ID> <BLOCK_NUMBER>

For more information, try '--help'.
";
        parse_and_compare(&args, Err(expected));
    }

    #[test]
    fn call_with_start_subcommand_without_argument_parses_successfully() {
        let args = ["blockservice", "start"];
        let expected = Args {
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
            command: Command::Start { port: 8080 },
        };
        parse_and_compare(&args, Ok(expected));
    }

    #[test]
    fn call_with_start_subcommand_with_help_argument_prints_subcommand_help() {
        let args = ["blockservice", "start", "--help"];
        let expected = "\
Start the block server

Usage: blockservice start [OPTIONS] [PORT]

Arguments:
  [PORT]  [default: 8080]

Options:
      --dir <DIR>  The path to the blockservice directory [default: .]
  -h, --help       Print help
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

Usage: blockservice start [OPTIONS] [PORT]

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
      --dir <DIR>    The path to the blockservice directory [default: .]
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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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
            dir: PathBuf::from(DEFAULT_BLOCKSERIVCE_PATH),
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
