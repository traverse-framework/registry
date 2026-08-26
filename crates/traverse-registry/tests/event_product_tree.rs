//! Integration tests for on-disk event-product tree validation (registry#168).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_fixture_valid() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/event-products/valid.json")
}

fn write_tree(root: &std::path::Path, relative_product: &str, body: &str) {
    let path = root.join(relative_product);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(&path, body).expect("write product");
}

fn valid_product_for_tree() -> String {
    fs::read_to_string(repo_fixture_valid()).expect("read fixture")
}

fn materialize_publisher(tmp: &std::path::Path) {
    write_tree(
        tmp,
        "capabilities/content.comments/content.comments.create-comment-draft/1.0.0/contract.json",
        r#"{"id":"content.comments.create-comment-draft","version":"1.0.0"}"#,
    );
}

#[test]
fn accepts_valid_event_product_tree_with_resolvable_publisher() {
    let tmp = tempfile_dir("accept");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.comment-draft-created/1.0.0/product.json",
        &valid_product_for_tree(),
    );
    materialize_publisher(&tmp);

    let report = traverse_registry::validate_event_product_tree(&tmp);
    assert!(report.ok(), "expected pass, got {:?}", report.errors);
    assert_eq!(report.validated, 1);
}

#[test]
fn rejects_unresolvable_publisher() {
    let tmp = tempfile_dir("unresolvable");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.comment-draft-created/1.0.0/product.json",
        &valid_product_for_tree(),
    );

    let report = traverse_registry::validate_event_product_tree(&tmp);
    assert!(!report.ok());
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "event_product.publisher_unresolvable"),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_path_identity_mismatch() {
    let tmp = tempfile_dir("mismatch");
    write_tree(
        &tmp,
        "events/wrong-ns/content.comments.comment-draft-created/1.0.0/product.json",
        &valid_product_for_tree(),
    );
    materialize_publisher(&tmp);

    let report = traverse_registry::validate_event_product_tree(&tmp);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "event_product.namespace_mismatch"),
        "{:?}",
        report.errors
    );
}

/// registry#324: an on-disk product.json whose `id` doesn't equal
/// `namespace.name` must be rejected by the tree walk, not just the
/// in-memory descriptor validator -- this is exactly the shape of the bad
/// publish that slipped through (`core.action-item.status-transitioned`,
/// `namespace="core"`, `name="status-transitioned"`).
#[test]
fn rejects_event_identity_mismatch() {
    let tmp = tempfile_dir("identity-mismatch");
    let mut product: serde_json::Value =
        serde_json::from_str(&valid_product_for_tree()).expect("parse fixture");
    product["contract"]["id"] = serde_json::json!("content.comments.extra.comment-draft-created");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.extra.comment-draft-created/1.0.0/product.json",
        &serde_json::to_string_pretty(&product).expect("serialize"),
    );
    materialize_publisher(&tmp);

    let report = traverse_registry::validate_event_product_tree(&tmp);
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.code == "event_product.inconsistent_identity"),
        "{:?}",
        report.errors
    );
}

/// registry#324: a *deprecated* product.json with a bad identity must not
/// break the tree walk -- it's immutable, published before this check
/// existed, and can never be edited to satisfy it. Structural path checks
/// still run; only the full descriptor-content validation is skipped.
#[test]
fn deprecated_product_with_identity_mismatch_does_not_fail_tree() {
    let tmp = tempfile_dir("deprecated-identity-mismatch");
    let mut product: serde_json::Value =
        serde_json::from_str(&valid_product_for_tree()).expect("parse fixture");
    product["contract"]["id"] = serde_json::json!("content.comments.extra.comment-draft-created");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.extra.comment-draft-created/1.0.0/product.json",
        &serde_json::to_string_pretty(&product).expect("serialize"),
    );
    write_tree(
        &tmp,
        "events/content.comments/content.comments.extra.comment-draft-created/1.0.0/deprecated.json",
        r#"{"deprecated":true,"reason":"test","deprecated_at":"2026-08-25T00:00:00Z"}"#,
    );
    materialize_publisher(&tmp);

    let report = traverse_registry::validate_event_product_tree(&tmp);
    assert!(report.ok(), "expected pass, got {:?}", report.errors);
}

#[test]
fn rejects_immutable_republication_conflict_for_same_id_version() {
    let original = valid_product_for_tree();
    let first: traverse_registry::EventProductDescriptor =
        serde_json::from_str(&original).expect("desc");
    let mut second = first.clone();
    second.support_route = "https://support.traverse.dev/other".to_string();

    let err = traverse_registry::validate_event_product_descriptor(&second, Some(&first))
        .expect_err("mutated republication must fail");
    assert!(
        err.errors.iter().any(
            |e| e.code == traverse_registry::EventProductErrorCode::ImmutableDescriptorConflict
        )
    );

    // Tree walk: second file reuses contract.id/version with different content.
    let tmp = tempfile_dir("immutable-tree");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.comment-draft-created/1.0.0/product.json",
        &original,
    );
    let mut mutated: serde_json::Value = serde_json::from_str(&original).expect("parse");
    mutated["support_route"] = serde_json::json!("https://support.traverse.dev/other");
    // Deliberately wrong path id segment so two files can coexist; contract
    // identity still matches, so `existing` triggers immutability.
    write_tree(
        &tmp,
        "events/content.comments/content.comments.comment-draft-created-copy/1.0.0/product.json",
        &serde_json::to_string_pretty(&mutated).expect("serialize"),
    );
    materialize_publisher(&tmp);

    let report = traverse_registry::validate_event_product_tree(&tmp);
    assert!(
        report.errors.iter().any(|error| {
            error.code == "event_product.immutable_descriptor_conflict"
                || error.code == "event_product.id_mismatch"
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn binary_exits_nonzero_on_invalid_tree() {
    let tmp = tempfile_dir("bin-fail");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.comment-draft-created/1.0.0/product.json",
        &valid_product_for_tree(),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_validate_event_products"))
        .arg("--root")
        .arg(&tmp)
        .output()
        .expect("run binary");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("event_product.publisher_unresolvable"),
        "{stderr}"
    );
}

#[test]
fn binary_exits_zero_on_valid_tree() {
    let tmp = tempfile_dir("bin-ok");
    write_tree(
        &tmp,
        "events/content.comments/content.comments.comment-draft-created/1.0.0/product.json",
        &valid_product_for_tree(),
    );
    materialize_publisher(&tmp);

    let output = Command::new(env!("CARGO_BIN_EXE_validate_event_products"))
        .arg("--root")
        .arg(&tmp)
        .output()
        .expect("run binary");
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn tempfile_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "traverse-registry-event-product-tree-{}-{}",
        label,
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).expect("tempdir");
    base
}
