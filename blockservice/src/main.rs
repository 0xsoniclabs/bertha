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
            result = cmd::execute(args.clone(), cancellation_token.clone(), ConfigFileAddressBinder::new(args.dir), std::io::stdout(),  std::io::stdin() ) =>{
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
