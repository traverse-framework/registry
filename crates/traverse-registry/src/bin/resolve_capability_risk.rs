//! CI binary (Spec `024-capability-risk-classification-adoption` FR-004):
//! emit each published capability version's risk projection -- computed
//! through `traverse-contracts` -- for `gather_catalog_data.py` and
//! `build_index.py` to fold into `catalog.json` / `index.json`.
//!
//! Usage: `resolve_capability_risk [--root <repo-root>]`
//!
//! Output (stdout): `{"capabilities": [{"reference", "risk",
//! "is_automatic_eligible", "risk_source"}, ...]}` sorted by `reference`.
//! Exits non-zero (and writes nothing to stdout) if any contract is
//! unreadable, unparseable, missing identity, or declares a malformed `risk`
//! block -- a silently dropped record must never reach a consumer.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use traverse_registry::capability_risk::resolve_tree;

fn main() -> ExitCode {
    let root = match parse_args(env::args().skip(1)) {
        ArgsOutcome::Help => {
            println!("Usage: resolve_capability_risk [--root <repo-root>]");
            return ExitCode::SUCCESS;
        }
        ArgsOutcome::Error(message) => {
            eprintln!("resolve_capability_risk: {message}");
            eprintln!("Usage: resolve_capability_risk [--root <repo-root>]");
            return ExitCode::from(2);
        }
        ArgsOutcome::Root(root) => root,
    };

    match resolve_tree(&root) {
        Ok(projections) => {
            let payload = serde_json::json!({ "capabilities": projections });
            match serde_json::to_string_pretty(&payload) {
                Ok(text) => {
                    println!("{text}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("resolve_capability_risk: unable to serialize output: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Err(errors) => {
            eprintln!(
                "resolve_capability_risk: failed with {} error(s)",
                errors.len()
            );
            for error in &errors {
                eprintln!("  {}: {}", error.path, error.message);
            }
            ExitCode::from(1)
        }
    }
}

enum ArgsOutcome {
    Help,
    Root(PathBuf),
    Error(String),
}

fn parse_args(mut args: impl Iterator<Item = String>) -> ArgsOutcome {
    let mut root = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => return ArgsOutcome::Error(format!("unable to read cwd: {err}")),
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => match args.next() {
                Some(value) => root = PathBuf::from(value),
                None => return ArgsOutcome::Error("--root requires a path argument".to_string()),
            },
            "--help" | "-h" => return ArgsOutcome::Help,
            other => return ArgsOutcome::Error(format!("unknown argument '{other}'")),
        }
    }

    ArgsOutcome::Root(root)
}
