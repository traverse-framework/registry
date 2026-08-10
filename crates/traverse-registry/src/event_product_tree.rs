//! On-disk event-product tree validation for the registry publish path
//! (`events/<namespace>/<id>/<version>/product.json`, spec 001 FR-016).

use crate::{
    EventProductDescriptor, EventProductErrorCode, EventProductValidationFailure,
    validate_event_product_descriptor,
};
use serde_json::Error as JsonError;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// One actionable failure produced while walking the event-product tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventProductTreeError {
    pub code: String,
    pub path: String,
    pub message: String,
}

/// Result of validating every `events/**/product.json` under `root`.
#[derive(Debug, Default)]
pub struct EventProductTreeReport {
    pub validated: usize,
    pub errors: Vec<EventProductTreeError>,
}

impl EventProductTreeReport {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Walks `root/events/**/product.json`, deserializes each as
/// [`EventProductDescriptor`], runs [`validate_event_product_descriptor`]
/// (passing any previously seen descriptor for the same `(id, version)` so
/// immutable republication conflicts surface), checks path identity fields,
/// and best-effort resolves `publishers`/`subscribers` against
/// `root/capabilities/<namespace>/<id>/<version>/contract.json`.
#[must_use]
pub fn validate_event_product_tree(root: &Path) -> EventProductTreeReport {
    let mut report = EventProductTreeReport::default();
    let events_dir = root.join("events");
    if !events_dir.is_dir() {
        return report;
    }

    let product_paths = match collect_product_paths(&events_dir) {
        Ok(paths) => paths,
        Err(error) => {
            report.errors.push(error);
            return report;
        }
    };

    let mut seen: BTreeMap<(String, String), EventProductDescriptor> = BTreeMap::new();

    for path in product_paths {
        report.validated += 1;
        let relative = display_path(root, &path);

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                report.errors.push(EventProductTreeError {
                    code: "event_product.unreadable".to_string(),
                    path: relative,
                    message: format!("unable to read product.json: {err}"),
                });
                continue;
            }
        };

        let descriptor: EventProductDescriptor = match serde_json::from_str(&raw) {
            Ok(descriptor) => descriptor,
            Err(err) => {
                report.errors.push(deserialize_error(&relative, &err));
                continue;
            }
        };

        push_path_mismatches(root, &path, &descriptor, &mut report.errors);

        let key = (
            descriptor.contract.id.clone(),
            descriptor.contract.version.clone(),
        );
        let existing = seen.get(&key);
        if let Err(failure) = validate_event_product_descriptor(&descriptor, existing) {
            push_descriptor_failures(&relative, &failure, &mut report.errors);
        }

        resolve_capability_refs(root, &relative, &descriptor, &mut report.errors);
        seen.insert(key, descriptor);
    }

    report
}

fn collect_product_paths(events_dir: &Path) -> Result<Vec<PathBuf>, EventProductTreeError> {
    let mut paths = Vec::new();
    collect_product_paths_rec(events_dir, events_dir, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_product_paths_rec(
    events_dir: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), EventProductTreeError> {
    let entries = fs::read_dir(dir).map_err(|err| EventProductTreeError {
        code: "event_product.walk_failed".to_string(),
        path: events_dir.display().to_string(),
        message: format!("unable to walk events directory: {err}"),
    })?;

    for entry in entries {
        let entry = entry.map_err(|err| EventProductTreeError {
            code: "event_product.walk_failed".to_string(),
            path: dir.display().to_string(),
            message: format!("unable to read directory entry: {err}"),
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_product_paths_rec(events_dir, &path, out)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("product.json") {
            out.push(path);
        }
    }
    Ok(())
}

fn push_path_mismatches(
    root: &Path,
    path: &Path,
    descriptor: &EventProductDescriptor,
    errors: &mut Vec<EventProductTreeError>,
) {
    let relative = PathBuf::from(display_path(root, path));
    let parts: Vec<_> = relative
        .iter()
        .filter_map(|part| part.to_str())
        .collect();

    // events/<namespace>/<id>/<version>/product.json
    if parts.len() != 5 || parts[0] != "events" || parts[4] != "product.json" {
        errors.push(EventProductTreeError {
            code: "event_product.bad_path".to_string(),
            path: relative.display().to_string(),
            message: "path must be events/<namespace>/<id>/<version>/product.json".to_string(),
        });
        return;
    }

    let namespace = parts[1];
    let id = parts[2];
    let version = parts[3];

    if namespace != descriptor.contract.namespace {
        errors.push(EventProductTreeError {
            code: "event_product.namespace_mismatch".to_string(),
            path: relative.display().to_string(),
            message: format!(
                "path namespace '{namespace}' does not match contract.namespace '{}'",
                descriptor.contract.namespace
            ),
        });
    }
    if id != descriptor.contract.id {
        errors.push(EventProductTreeError {
            code: "event_product.id_mismatch".to_string(),
            path: relative.display().to_string(),
            message: format!(
                "path id '{id}' does not match contract.id '{}'",
                descriptor.contract.id
            ),
        });
    }
    if version != descriptor.contract.version {
        errors.push(EventProductTreeError {
            code: "event_product.version_mismatch".to_string(),
            path: relative.display().to_string(),
            message: format!(
                "path version '{version}' does not match contract.version '{}'",
                descriptor.contract.version
            ),
        });
    }
}

fn resolve_capability_refs(
    root: &Path,
    relative: &str,
    descriptor: &EventProductDescriptor,
    errors: &mut Vec<EventProductTreeError>,
) {
    for publisher in &descriptor.contract.publishers {
        if !capability_contract_exists(root, &publisher.capability_id, &publisher.version) {
            errors.push(EventProductTreeError {
                code: "event_product.publisher_unresolvable".to_string(),
                path: relative.to_string(),
                message: format!(
                    "publisher {}@{} does not resolve to capabilities/<namespace>/{}/{}",
                    publisher.capability_id,
                    publisher.version,
                    publisher.capability_id,
                    publisher.version
                ),
            });
        }
    }
    for subscriber in &descriptor.contract.subscribers {
        if !capability_contract_exists(root, &subscriber.capability_id, &subscriber.version) {
            errors.push(EventProductTreeError {
                code: "event_product.subscriber_unresolvable".to_string(),
                path: relative.to_string(),
                message: format!(
                    "subscriber {}@{} does not resolve to capabilities/<namespace>/{}/{}",
                    subscriber.capability_id,
                    subscriber.version,
                    subscriber.capability_id,
                    subscriber.version
                ),
            });
        }
    }
}

fn capability_contract_exists(root: &Path, capability_id: &str, version: &str) -> bool {
    let capabilities = root.join("capabilities");
    let Ok(namespaces) = fs::read_dir(&capabilities) else {
        return false;
    };
    for entry in namespaces.flatten() {
        let candidate = entry
            .path()
            .join(capability_id)
            .join(version)
            .join("contract.json");
        if candidate.is_file() {
            return true;
        }
    }
    false
}

fn push_descriptor_failures(
    relative: &str,
    failure: &EventProductValidationFailure,
    errors: &mut Vec<EventProductTreeError>,
) {
    for error in &failure.errors {
        errors.push(EventProductTreeError {
            code: format!("event_product.{}", error_code_slug(error.code)),
            path: format!("{relative}:{}", error.path),
            message: format!("{} ({})", error.message, error.remediation),
        });
    }
}

fn error_code_slug(code: EventProductErrorCode) -> &'static str {
    match code {
        EventProductErrorCode::MissingSupportRoute => "missing_support_route",
        EventProductErrorCode::InvalidSupportRoute => "invalid_support_route",
        EventProductErrorCode::MissingFieldClassification => "missing_field_classification",
        EventProductErrorCode::UnexpectedFieldClassification => "unexpected_field_classification",
        EventProductErrorCode::DuplicateFieldClassification => "duplicate_field_classification",
        EventProductErrorCode::MissingReplacement => "missing_replacement",
        EventProductErrorCode::UnexpectedReplacement => "unexpected_replacement",
        EventProductErrorCode::InvalidReplacement => "invalid_replacement",
        EventProductErrorCode::NonPastTenseName => "non_past_tense_name",
        EventProductErrorCode::ImmutableDescriptorConflict => "immutable_descriptor_conflict",
        EventProductErrorCode::MissingCloudEventsSource => "missing_cloud_events_source",
        EventProductErrorCode::InvalidCloudEventsSubjectField => {
            "invalid_cloud_events_subject_field"
        }
        EventProductErrorCode::MissingDeduplicationIdField => "missing_deduplication_id_field",
        EventProductErrorCode::MissingCorrelationIdField => "missing_correlation_id_field",
        EventProductErrorCode::MissingRetentionPolicy => "missing_retention_policy",
    }
}

fn deserialize_error(relative: &str, err: &JsonError) -> EventProductTreeError {
    EventProductTreeError {
        code: "event_product.invalid_json".to_string(),
        path: relative.to_string(),
        message: format!("unable to deserialize EventProductDescriptor: {err}"),
    }
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
