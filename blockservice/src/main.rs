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

use blockservice::{
    cli::Args,
    cmd::{self, ConfigFileAddressBinder},
};
use clap::Parser;
use tokio::signal;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let cancellation_token = CancellationToken::new();

    tokio::spawn({
        let cancellation_token = cancellation_token.clone();
        async move {
            tokio::select! {
                _ = cancellation_token.cancelled() => {},
            result = cmd::execute(
                args.clone(),
                cancellation_token.clone(),
                ConfigFileAddressBinder::new(args.dir),
                std::io::stdout(),
                std::io::stdin()
            ) => {
                    if let Err(e) = result {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                    cancellation_token.cancel();
                }
            }
        }
    });

    tokio::select! {
        _ = cancellation_token.cancelled() => {},
        result = signal::ctrl_c() => {
            if result.is_err() {
                eprintln!("failed to install Ctrl+C handler");
                std::process::exit(1);
            }
            println!("\nReceived Ctrl+C, shutting down...");
            cancellation_token.cancel();
        }
    }
}
