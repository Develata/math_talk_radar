//! Scan pipeline orchestration (§12). The full pipeline lands in M4; this
//! establishes the entry shape the `scan` command calls.
use crate::cli::ScanArgs;
use crate::output::ScanOutput;

pub async fn run_scan(args: ScanArgs) -> anyhow::Result<ScanOutput> {
    let _ = args;
    Err(anyhow::anyhow!("scan pipeline not implemented in M0"))
}
