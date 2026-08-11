#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use traverse_registry::export_async_api_tree;

fn tempfile_dir(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "traverse-registry-asyncapi-export-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).expect("create temp dir");
    base
}

fn write_tree(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parents");
    }
    fs::write(path, contents).expect("write file");
}

fn minimal_product() -> String {
    r#"{
      "contract": {
        "kind": "event_contract",
        "schema_version": "1.0.0",
        "id": "core.action-item.status-transitioned",
        "namespace": "core",
        "name": "status-transitioned",
        "version": "1.0.0",
        "lifecycle": "active",
        "owner": { "team": "loop", "contact": "founders@loop.dev" },
        "summary": "Published when an action item's status has transitioned.",
        "description": "Governed domain event for status transitions.",
        "payload": {
          "schema": {
            "type": "object",
            "required": ["action_item_id"],
            "properties": { "action_item_id": { "type": "string" } }
          },
          "compatibility": "backward-compatible"
        },
        "classification": {
          "domain": "core.action-item",
          "bounded_context": "action-items",
          "event_type": "domain",
          "tags": ["action-items"]
        },
        "publishers": [
          { "capability_id": "core.transition-action-status", "version": "1.1.0" }
        ],
        "subscribers": [],
        "policies": [{ "id": "default-action-item-event-safety" }],
        "tags": ["action-items"],
        "provenance": {
          "source": "greenfield",
          "author": "loop",
          "created_at": "2026-08-10T00:00:00Z"
        },
        "evidence": []
      },
      "support_route": "https://support.traverse.dev/events/core.action-item.status-transitioned",
      "exposure": "internal",
      "field_classifications": [
        { "field_path": "action_item_id", "classification": "none" }
      ],
      "replacement": null,
      "cloud_events_source": "traverse://capability/core.transition-action-status",
      "cloud_events_subject_field": "action_item_id",
      "deduplication_id_field": "envelope.id",
      "ordering_scope_field": "action_item_id",
      "correlation_id_field": "envelope.correlation_id",
      "causation_id_field": null,
      "retention_policy": "retain 90 days"
    }"#
    .to_string()
}

#[test]
fn writes_asyncapi_document_named_by_id_and_version() {
    let root = tempfile_dir("ok");
    write_tree(
        &root,
        "events/core/core.action-item.status-transitioned/1.0.0/product.json",
        &minimal_product(),
    );
    let out = root.join("catalog/asyncapi");

    let report = export_async_api_tree(&root, &out);
    assert!(report.ok(), "{:?}", report.errors);
    assert_eq!(report.written, 1);

    let path = out.join("core.action-item.status-transitioned@1.0.0.json");
    let document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(path).expect("read export"))
            .expect("parse export");
    assert_eq!(document["asyncapi"], "2.6.0");
    assert_eq!(document["info"]["title"], "core.action-item.status-transitioned");
    assert_eq!(document["info"]["version"], "1.0.0");
    assert!(
        document["channels"]["core.action-item.status-transitioned"]["publish"].is_object()
    );
}

#[test]
fn binary_exits_zero_and_writes_under_out() {
    let root = tempfile_dir("bin-ok");
    write_tree(
        &root,
        "events/core/core.action-item.status-transitioned/1.0.0/product.json",
        &minimal_product(),
    );
    let out = root.join("out");

    let output = Command::new(env!("CARGO_BIN_EXE_export_async_api"))
        .arg("--root")
        .arg(&root)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run binary");
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.join("core.action-item.status-transitioned@1.0.0.json")
            .is_file()
    );
}
