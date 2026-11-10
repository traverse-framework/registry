//! Publish-gate parse of on-disk capability records with the same
//! `traverse-contracts` parser Traverse consumers use (registry#656,
//! traverse decision 130).
//!
//! A record the registry publishes but `traverse_contracts::parse_contract`
//! rejects is unusable downstream: `traverse-cli app prepare` caches it, then
//! `app validate` fails. Spec 002 rules (for example the `provenance.source`
//! enum and required `author` / `created_at`) are therefore checked by the
//! parser itself, never by a second copy of the spec.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use traverse_contracts::parse_contract;

/// One record `traverse-contracts` could not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractParseError {
    pub code: String,
    pub path: String,
    pub message: String,
}

/// Result of parsing the gated capability records under a registry root.
#[derive(Debug, Default)]
pub struct ContractParseReport {
    pub parsed: usize,
    pub skipped_deprecated: usize,
    pub errors: Vec<ContractParseError>,
}

impl ContractParseReport {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Parses every active `capabilities/**/contract.json` under `root` (one with
/// no sibling `deprecated.json`), plus every record in `always_include`
/// (paths relative to `root`: the PR's new or changed records), with
/// [`traverse_contracts::parse_contract`].
///
/// Other deprecated records are skipped: they are immutable, stay resolvable
/// for old pins, and each names a parseable successor (spec 005).
#[must_use]
pub fn validate_capability_contract_parse_tree(
    root: &Path,
    always_include: &BTreeSet<PathBuf>,
) -> ContractParseReport {
    let mut report = ContractParseReport::default();
    let mut found = Vec::new();
    let capabilities_dir = root.join("capabilities");
    if capabilities_dir.is_dir()
        && let Err(error) = collect_contract_paths(&capabilities_dir, &mut found)
    {
        report.errors.push(error);
        return report;
    }
    found.sort();

    for path in found {
        let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let deprecated = path
            .parent()
            .is_some_and(|dir| dir.join("deprecated.json").is_file());
        if deprecated && !always_include.contains(&relative) {
            report.skipped_deprecated += 1;
            continue;
        }
        report.parsed += 1;
        let display = relative.display().to_string();
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) => {
                report.errors.push(ContractParseError {
                    code: "contract.unreadable".to_string(),
                    path: display,
                    message: format!("unable to read contract.json: {err}"),
                });
                continue;
            }
        };
        if let Err(failure) = parse_contract(&raw) {
            let message = failure
                .errors
                .iter()
                .map(|error| error.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            report.errors.push(ContractParseError {
                code: "contract.traverse_contracts_parse_failed".to_string(),
                path: display,
                message,
            });
        }
    }

    report
}

fn collect_contract_paths(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), ContractParseError> {
    let walk_error = |err: std::io::Error| ContractParseError {
        code: "contract.walk_failed".to_string(),
        path: dir.display().to_string(),
        message: format!("unable to walk capabilities directory: {err}"),
    };
    for entry in fs::read_dir(dir).map_err(walk_error)? {
        let path = entry.map_err(walk_error)?.path();
        if path.is_dir() {
            collect_contract_paths(&path, out)?;
        } else if path.file_name().and_then(|name| name.to_str()) == Some("contract.json") {
            out.push(path);
        }
    }
    Ok(())
}
