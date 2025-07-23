use std::fs;

use blockservice::cli::Args;
use clap::Parser;

#[tokio::test]
async fn usage_in_readme_is_up_to_date() {
    let args = Args::try_parse_from(["blockservice", "--help"]);
    assert!(args.is_err());
    let usage = args.unwrap_err().to_string();
    let readme = fs::read_to_string("./README.md").unwrap();
    assert!(readme.contains(&usage));
}
