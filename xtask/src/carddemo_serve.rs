use clap::Args;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub(crate) struct ServeArgs {
    /// Local address for the browser terminal and authenticated gateway.
    #[arg(long, default_value = "127.0.0.1:8080")]
    listen: SocketAddr,
    /// Durable SQLite state and compiled artifacts; use a dedicated directory.
    #[arg(long)]
    state_dir: PathBuf,
}

pub(crate) fn run(root: &Path, args: ServeArgs) -> Result<(), String> {
    mainframe_env_conformance::serve_carddemo_from_env(
        &root.join("conformance/profiles/carddemo/inventory/carddemo-corpus.json"),
        &args.state_dir,
        args.listen,
    )
    .map_err(|error| error.to_string())
}
