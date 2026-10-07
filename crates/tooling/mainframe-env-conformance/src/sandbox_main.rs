//! Portable profile composition, independent of repository discovery and certification commands.
use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mainframe-sandbox-runtime",
    about = "Mainframe Sandbox application runtime"
)]
struct Args {
    #[arg(long)]
    reference: PathBuf,
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    state_dir: PathBuf,
    #[arg(long)]
    inventory: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
}

fn main() {
    let args = Args::parse();
    if let Err(error) = mainframe_env_conformance::serve_carddemo_application(
        &args.inventory,
        &args.reference,
        &args.source,
        &args.state_dir,
        args.listen,
    ) {
        eprintln!("mainframe-sandbox-runtime: {error}");
        std::process::exit(1);
    }
}
