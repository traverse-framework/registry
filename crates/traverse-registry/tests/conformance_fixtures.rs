//! Drives the portable JSON conformance fixtures in
//! `crates/traverse-registry/fixtures/event-products/` against
//! `validate_event_product_descriptor`, proving `MANIFEST.json`'s claims
//! about each fixture are actually true. Cross-repo consumers (spec 016,
//! `traverse#896`/`#897`/`#898`) can conformance-test their own port
//! against the same fixture set without depending on this crate's
//! internals -- this test is the proof this repo's own validator agrees
//! with its own claims.

#![allow(clippy::expect_used)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use traverse_registry::{EventProductDescriptor, EventProductErrorCode, validate_event_product_descriptor};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/event-products")
}

fn load_manifest() -> Value {
    let raw = fs::read_to_string(fixtures_dir().join("MANIFEST.json"))
        .expect("MANIFEST.json should read");
    serde_json::from_str(&raw).expect("MANIFEST.json should parse")
}

fn load_descriptor(file: &str) -> EventProductDescriptor {
    let path = fixtures_dir().join(file);
    let raw = fs::read_to_string(&path).expect("fixture file should read");
    serde_json::from_str(&raw).expect("fixture file should parse as EventProductDescriptor")
}

fn error_code_from_str(name: &str) -> Option<EventProductErrorCode> {
    Some(match name {
        "MissingSupportRoute" => EventProductErrorCode::MissingSupportRoute,
        "InvalidSupportRoute" => EventProductErrorCode::InvalidSupportRoute,
        "MissingFieldClassification" => EventProductErrorCode::MissingFieldClassification,
        "UnexpectedFieldClassification" => EventProductErrorCode::UnexpectedFieldClassification,
        "DuplicateFieldClassification" => EventProductErrorCode::DuplicateFieldClassification,
        "MissingReplacement" => EventProductErrorCode::MissingReplacement,
        "UnexpectedReplacement" => EventProductErrorCode::UnexpectedReplacement,
        "InvalidReplacement" => EventProductErrorCode::InvalidReplacement,
        "NonPastTenseName" => EventProductErrorCode::NonPastTenseName,
        "ImmutableDescriptorConflict" => EventProductErrorCode::ImmutableDescriptorConflict,
        "MissingCloudEventsSource" => EventProductErrorCode::MissingCloudEventsSource,
        "InvalidCloudEventsSubjectField" => EventProductErrorCode::InvalidCloudEventsSubjectField,
        "MissingDeduplicationIdField" => EventProductErrorCode::MissingDeduplicationIdField,
        "MissingCorrelationIdField" => EventProductErrorCode::MissingCorrelationIdField,
        "MissingRetentionPolicy" => EventProductErrorCode::MissingRetentionPolicy,
        _ => return None,
    })
}

#[test]
fn manifest_lists_every_fixture_file_and_nothing_else() {
    let manifest = load_manifest();

    let listed: std::collections::BTreeSet<String> = manifest["fixtures"]
        .as_array()
        .expect("fixtures should be an array")
        .iter()
        .map(|entry| entry["file"].as_str().expect("file should be a string").to_string())
        .collect();

    let on_disk: std::collections::BTreeSet<String> = fs::read_dir(fixtures_dir())
        .expect("fixtures dir should read")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                && name != "MANIFEST.json"
        })
        .collect();

    assert_eq!(listed, on_disk);
}

#[test]
fn every_manifest_fixture_matches_its_claimed_outcome() {
    let manifest = load_manifest();

    for entry in manifest["fixtures"].as_array().expect("fixtures should be an array") {
        let file = entry["file"].as_str().expect("file should be a string");
        let expect = entry["expect"].as_str().expect("expect should be a string");
        let descriptor = load_descriptor(file);

        let existing_file = entry.get("existing").and_then(Value::as_str);
        let existing = existing_file.map(load_descriptor);

        let result = validate_event_product_descriptor(&descriptor, existing.as_ref());

        assert!(
            expect == "accept" || expect == "reject",
            "fixture {file} has unknown expect value: {expect}"
        );

        if expect == "accept" {
            assert!(
                result.is_ok(),
                "fixture {file} should be accepted but was rejected: {result:?}"
            );
            continue;
        }

        let failure =
            result.expect_err("fixture claiming reject should not have been accepted");
        let error_code_str = entry["error_code"]
            .as_str()
            .expect("a reject fixture needs an error_code");
        let expected_code =
            error_code_from_str(error_code_str).expect("MANIFEST.json used an unknown error_code");

        assert!(
            failure.errors.iter().any(|error| error.code == expected_code),
            "fixture {file} should fail with {expected_code:?}, got {failure:?}"
        );
    }
}

#[test]
fn valid_fixture_round_trips_through_serialization() {
    let descriptor = load_descriptor("valid.json");
    let reserialized = serde_json::to_string(&descriptor).expect("descriptor should serialize");
    let reparsed: EventProductDescriptor =
        serde_json::from_str(&reserialized).expect("reserialized descriptor should parse");
    assert_eq!(descriptor, reparsed);
}

#[test]
fn fixtures_directory_exists_at_the_documented_path() {
    assert!(fixtures_dir().is_dir());
}
