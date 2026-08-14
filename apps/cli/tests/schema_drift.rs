//! H4: schema drift gate (§09:38). Regenerates the JSON Schema from the Rust
//! model in-process and compares it against the golden file at
//! `docs/reference/output-schema.json`. Fails if they differ, telling the
//! developer to regenerate with `cargo run -- schema > docs/reference/output-schema.json`.
use schemars::schema_for;
use std::path::PathBuf;

#[test]
fn schema_matches_golden() {
    let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("reference")
        .join("output-schema.json");
    let golden = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
        panic!(
            "cannot read golden schema {}: {e}; regenerate with `cargo run -- schema > {}`",
            golden_path.display(),
            golden_path.display()
        )
    });
    let generated = schema_for!(math_talk_radar_cli::output::ScanOutput);
    let generated_json = serde_json::to_string_pretty(&generated).expect("schema serializes");
    assert_eq!(
        golden.trim(),
        generated_json.trim(),
        "schema drift detected: Rust model ↔ golden output-schema.json differ. \
         Regenerate with `cargo run -- schema > {}`",
        golden_path.display()
    );
}
