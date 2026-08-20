//! H4: schema drift gate (§09:38). Regenerates the JSON Schema from the Rust
//! model in-process and compares it against the golden file at
//! `docs/reference/output-schema.json`. Fails if they differ, telling the
//! developer to regenerate with `cargo run -- schema > docs/reference/output-schema.json`.
//!
//! §64 backward-compatibility gate: while `OUTPUT_SCHEMA_VERSION == "1.0"`,
//! the generated schema must be backward-compatible with the frozen v1.0
//! baseline at `docs/reference/output-schema-v1.0.json`. The baseline is
//! immutable — it can only be replaced when `OUTPUT_SCHEMA_VERSION` is bumped.
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

/// §64 backward-compatibility gate. While `OUTPUT_SCHEMA_VERSION == "1.0"`,
/// the generated schema must be backward-compatible with the frozen v1.0
/// baseline. Enforces: no removed required property, no new required
/// property (v0.x may only add optional fields), no removed enum value,
/// no changed const, no removed property, no removed definition.
#[test]
fn schema_v1_backward_compatible() {
    let baseline_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("docs")
        .join("reference")
        .join("output-schema-v1.0.json");
    let baseline_str = std::fs::read_to_string(&baseline_path).unwrap_or_else(|e| {
        panic!(
            "cannot read frozen v1.0 schema {}: {e}",
            baseline_path.display()
        )
    });
    let baseline: serde_json::Value =
        serde_json::from_str(&baseline_str).expect("parse baseline schema");
    let current = schema_for!(math_talk_radar_cli::output::ScanOutput);
    let current: serde_json::Value =
        serde_json::to_value(&current).expect("serialize current schema");

    fn check_object_compat(baseline: &serde_json::Value, current: &serde_json::Value, ctx: &str) {
        let baseline_req: Vec<&str> = baseline["required"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let current_req: Vec<&str> = current["required"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        for &r in &baseline_req {
            assert!(
                current_req.contains(&r),
                "§64: required '{r}' in {ctx} v1.0 no longer required"
            );
        }
        for &r in &current_req {
            assert!(
                baseline_req.contains(&r),
                "§64: new required '{r}' in {ctx} — v0.x may only add optional fields"
            );
        }
        if let Some(baseline_enum) = baseline["enum"].as_array() {
            let current_enum: Vec<&serde_json::Value> = current["enum"]
                .as_array()
                .map(|a| a.iter().collect())
                .unwrap_or_default();
            for val in baseline_enum {
                assert!(
                    current_enum.contains(&val),
                    "§64: enum value {val} removed from {ctx}"
                );
            }
        }
        if let Some(baseline_const) = baseline.get("const") {
            assert_eq!(
                current.get("const"),
                Some(baseline_const),
                "§64: const changed in {ctx}"
            );
        }
        if let Some(baseline_props) = baseline["properties"].as_object() {
            let current_props = current["properties"].as_object();
            for name in baseline_props.keys() {
                assert!(
                    current_props.is_some_and(|p| p.contains_key(name)),
                    "§64: property '{name}' in {ctx} was removed"
                );
            }
        }
    }

    check_object_compat(&baseline, &current, "ScanOutput");
    assert_eq!(
        current["properties"]["schema_version"]["const"],
        serde_json::json!("1.0"),
        "§64: schema_version const must stay \"1.0\""
    );
    if let Some(baseline_defs) = baseline["definitions"].as_object() {
        let current_defs = current["definitions"].as_object();
        for (name, baseline_def) in baseline_defs {
            let current_def = current_defs.and_then(|d| d.get(name));
            assert!(
                current_def.is_some(),
                "§64: definition '{name}' was removed"
            );
            if let Some(current_def) = current_def {
                check_object_compat(baseline_def, current_def, &format!("definition '{name}'"));
            }
        }
    }
}
