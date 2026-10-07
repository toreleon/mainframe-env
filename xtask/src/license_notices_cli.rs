//! Export target-filtered legal texts for executable sandbox distributions.
use clap::Args;
use std::path::{Path, PathBuf};

#[derive(Debug, Args)]
pub(crate) struct NoticeArgs {
    #[arg(long)]
    pub(crate) check: bool,
    /// Write complete third-party legal texts to this external artifact path.
    #[arg(long)]
    output: Option<PathBuf>,
}

pub(crate) fn run(root: &Path, args: &NoticeArgs) -> Result<(), String> {
    let target = super::host_target(root)?;
    let report = super::dependency_licenses::generate(root, &target)?;
    if let Some(output) = &args.output {
        std::fs::write(output, &report.bytes).map_err(|error| error.to_string())?;
    }
    println!(
        "license-notices target={target} production-packages={} third-party-packages={} unique-legal-texts={} bytes={}",
        report.production_packages,
        report.third_party_packages,
        report.unique_legal_texts,
        report.bytes.len()
    );
    Ok(())
}
