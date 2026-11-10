//! Integration tests for the `traverse-contracts` publish-gate parse of
//! on-disk capability records (registry#656, traverse decision 130).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use traverse_registry::validate_capability_contract_parse_tree;

const RECORD: &str = "capabilities/doc-approval/doc-approval.analyze/1.0.0/contract.json";

fn valid_contract() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/contracts/valid.json");
    serde_json::from_str(&fs::read_to_string(path).expect("read fixture")).expect("parse fixture")
}

fn write_record(root: &Path, contract: &Value, deprecated: bool) {
    let path = root.join(RECORD);
    let dir = path.parent().expect("parent");
    fs::create_dir_all(dir).expect("mkdir");
    fs::write(&path, serde_json::to_string_pretty(contract).expect("json")).expect("write");
    if deprecated {
        fs::write(dir.join("deprecated.json"), r#"{"deprecated":true}"#).expect("write");
    }
}

fn with_provenance(field: &str, value: Option<&str>) -> Value {
    let mut contract = valid_contract();
    let provenance = contract["provenance"].as_object_mut().expect("provenance");
    match value {
        Some(value) => provenance.insert(field.to_string(), Value::from(value)),
        None => provenance.remove(field),
    };
    contract
}

fn none() -> BTreeSet<PathBuf> {
    BTreeSet::new()
}

#[test]
fn accepts_spec_002_provenance() {
    let tmp = tempfile_dir("accept");
    write_record(&tmp, &with_provenance("source", Some("ai-assisted")), false);

    let report = validate_capability_contract_parse_tree(&tmp, &none());
    assert!(report.ok(), "{:?}", report.errors);
    assert_eq!(report.parsed, 1);
}

#[test]
fn rejects_unknown_provenance_source() {
    for source in ["derived", "registry-wave2"] {
        let tmp = tempfile_dir(&format!("source-{source}"));
        write_record(&tmp, &with_provenance("source", Some(source)), false);

        let report = validate_capability_contract_parse_tree(&tmp, &none());
        assert_eq!(report.errors.len(), 1, "{source}");
        let error = &report.errors[0];
        assert_eq!(error.code, "contract.traverse_contracts_parse_failed");
        assert_eq!(error.path, RECORD);
        assert!(error.message.contains(source), "{}", error.message);
    }
}

#[test]
fn rejects_missing_required_provenance_fields() {
    for field in ["source", "author", "created_at"] {
        let tmp = tempfile_dir(&format!("missing-{field}"));
        write_record(&tmp, &with_provenance(field, None), false);

        let report = validate_capability_contract_parse_tree(&tmp, &none());
        assert_eq!(report.errors.len(), 1, "{field}");
        assert!(
            report.errors[0].message.contains(field),
            "{}",
            report.errors[0].message
        );
    }
}

#[test]
fn skips_deprecated_records_unless_listed() {
    let tmp = tempfile_dir("deprecated");
    write_record(&tmp, &with_provenance("source", Some("derived")), true);

    let report = validate_capability_contract_parse_tree(&tmp, &none());
    assert!(report.ok(), "{:?}", report.errors);
    assert_eq!((report.parsed, report.skipped_deprecated), (0, 1));

    let listed = BTreeSet::from([PathBuf::from(RECORD)]);
    let report = validate_capability_contract_parse_tree(&tmp, &listed);
    assert_eq!((report.parsed, report.skipped_deprecated), (1, 0));
    assert_eq!(report.errors.len(), 1);
}

#[test]
fn reports_unreadable_records() {
    let tmp = tempfile_dir("unreadable");
    fs::create_dir_all(tmp.join(RECORD)).expect("mkdir");
    fs::create_dir_all(tmp.join("capabilities/x/x.y/1.0.0")).expect("mkdir");
    fs::write(
        tmp.join("capabilities/x/x.y/1.0.0/contract.json"),
        [0xff, 0xfe],
    )
    .expect("write");

    let report = validate_capability_contract_parse_tree(&tmp, &none());
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert_eq!(report.errors[0].code, "contract.unreadable");
}

#[test]
fn empty_root_passes() {
    let tmp = tempfile_dir("empty");
    let report = validate_capability_contract_parse_tree(&tmp, &none());
    assert!(report.ok());
    assert_eq!(report.parsed, 0);
}

#[test]
fn binary_exit_codes() {
    let tmp = tempfile_dir("bin");
    write_record(&tmp, &with_provenance("source", Some("derived")), true);
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_validate_capability_contracts"))
            .arg("--root")
            .arg(&tmp)
            .args(extra)
            .output()
            .expect("run binary")
    };

    let output = run(&[]);
    assert!(output.status.success(), "{output:?}");

    let output = run(&[RECORD]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("contract.traverse_contracts_parse_failed"),
        "{stderr}"
    );

    assert_eq!(run(&["--bogus"]).status.code(), Some(2));
    assert!(run(&["--help"]).status.success());
}

fn tempfile_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "traverse-registry-contract-parse-tree-{}-{}",
        label,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).expect("tempdir");
    base
}
