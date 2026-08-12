use clap::Parser;
use math_talk_radar_cli::{cli::Cli, runtime};

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    runtime::init_logging(cli.verbose, cli.log_format);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: failed to create async runtime: {e}");
            return std::process::ExitCode::from(3);
        }
    };
    rt.block_on(runtime::run(cli))
}
