use clap::Parser;
use math_talk_radar_cli::{cli::Cli, runtime};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    runtime::init_logging(cli.verbose, cli.log_format);
    runtime::run(cli).await
}
