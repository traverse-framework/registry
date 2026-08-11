//! On-disk `AsyncAPI` export for every published ECCA event product
//! (`events/**/product.json` → `catalog/asyncapi/<id>@<version>.json`).
//!
//! Spec `016-ecca-event-product-adoption` FR-015: documents are always
//! regenerated from the governed descriptor via
//! [`crate::generate_async_api_document`]; nothing here accepts or stores a
//! hand-authored `AsyncAPI` document.

use crate::{EventProductDescriptor, generate_async_api_document};
use std::fs;
use std::path::{Path, PathBuf};

/// One actionable failure produced while exporting `AsyncAPI` documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsyncApiExportError {
    pub code: String,
    pub path: String,
    pub message: String,
}

/// Result of writing `AsyncAPI` documents for every on-disk event product.
#[derive(Debug, Default)]
pub struct AsyncApiExportReport {
    pub written: usize,
    pub errors: Vec<AsyncApiExportError>,
}

impl AsyncApiExportReport {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Walks `root/events/**/product.json`, generates an `AsyncAPI` 2.6.0 document
/// for each descriptor, and writes
/// `out_dir/<contract.id>@<contract.version>.json`.
#[must_use]
pub fn export_async_api_tree(root: &Path, out_dir: &Path) -> AsyncApiExportReport {
    let mut report = AsyncApiExportReport::default();
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

    if let Err(err) = fs::create_dir_all(out_dir) {
        report.errors.push(AsyncApiExportError {
            code: "async_api.out_dir_failed".to_string(),
            path: out_dir.display().to_string(),
            message: format!("unable to create output directory: {err}"),
        });
        return report;
    }

    for path in product_paths {
        let relative = display_path(root, &path);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                report.errors.push(AsyncApiExportError {
                    code: "async_api.unreadable".to_string(),
                    path: relative,
                    message: format!("unable to read product.json: {err}"),
                });
                continue;
            }
        };

        let descriptor: EventProductDescriptor = match serde_json::from_str(&raw) {
            Ok(descriptor) => descriptor,
            Err(err) => {
                report.errors.push(AsyncApiExportError {
                    code: "async_api.deserialize".to_string(),
                    path: relative,
                    message: format!("unable to deserialize EventProductDescriptor: {err}"),
                });
                continue;
            }
        };

        let document = generate_async_api_document(&descriptor);
        let filename = format!(
            "{}@{}.json",
            descriptor.contract.id, descriptor.contract.version
        );
        let out_path = out_dir.join(&filename);
        let body = match serde_json::to_string_pretty(&document) {
            Ok(body) => body,
            Err(err) => {
                report.errors.push(AsyncApiExportError {
                    code: "async_api.serialize".to_string(),
                    path: relative,
                    message: format!("unable to serialize AsyncAPI document: {err}"),
                });
                continue;
            }
        };

        if let Err(err) = fs::write(&out_path, format!("{body}\n")) {
            report.errors.push(AsyncApiExportError {
                code: "async_api.write_failed".to_string(),
                path: out_path.display().to_string(),
                message: format!("unable to write AsyncAPI document: {err}"),
            });
            continue;
        }

        report.written += 1;
    }

    report
}

fn collect_product_paths(events_dir: &Path) -> Result<Vec<PathBuf>, AsyncApiExportError> {
    let mut paths = Vec::new();
    collect_product_paths_rec(events_dir, events_dir, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_product_paths_rec(
    events_dir: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), AsyncApiExportError> {
    let entries = fs::read_dir(dir).map_err(|err| AsyncApiExportError {
        code: "async_api.walk_failed".to_string(),
        path: events_dir.display().to_string(),
        message: format!("unable to walk events directory: {err}"),
    })?;

    for entry in entries {
        let entry = entry.map_err(|err| AsyncApiExportError {
            code: "async_api.walk_failed".to_string(),
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

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |relative| relative.display().to_string(),
    )
}
