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
