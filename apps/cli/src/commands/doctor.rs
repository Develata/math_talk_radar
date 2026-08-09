use crate::cli::DoctorArgs;
use crate::output::OUTPUT_SCHEMA_VERSION;
use crate::runtime::CliError;

pub async fn run(args: DoctorArgs) -> Result<(), CliError> {
    let binary = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".into());
    let config_dir = dirs_config_dir();
    let state_dir = dirs_state_dir();
    let schema_version = OUTPUT_SCHEMA_VERSION;

    if args.json {
        let json = serde_json::json!({
            "binary": binary,
            "config_dir": config_dir,
            "state_dir": state_dir,
            "schema_version": schema_version,
            "network_check": args.network,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json)
                .map_err(|e| CliError::serialization(format!("encode doctor json: {e}")))?
        );
    } else {
        println!("math_talk_radar doctor");
        println!("  binary:         {binary}");
        println!("  config_dir:     {config_dir}");
        println!("  state_dir:      {state_dir}");
        println!("  schema_version: {schema_version}");
        println!("  network_check:  {}", args.network);
    }
    Ok(())
}

fn dirs_config_dir() -> String {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let mut p = std::path::PathBuf::from(xdg);
        p.push("math_talk_radar");
        return p.display().to_string();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = std::path::PathBuf::from(home);
        p.push(".config");
        p.push("math_talk_radar");
        return p.display().to_string();
    }
    "<unknown>".into()
}

fn dirs_state_dir() -> String {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        let mut p = std::path::PathBuf::from(xdg);
        p.push("math_talk_radar");
        return p.display().to_string();
    }
    if let Some(home) = std::env::var_os("HOME") {
        let mut p = std::path::PathBuf::from(home);
        p.push(".local");
        p.push("share");
        p.push("math_talk_radar");
        return p.display().to_string();
    }
    "<unknown>".into()
}
