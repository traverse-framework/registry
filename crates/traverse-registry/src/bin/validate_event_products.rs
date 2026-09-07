//! CI binary: validate every on-disk ECCA event product under `events/`.
//!
//! Usage: `validate_event_products [--root <repo-root>]`
//!
//! Defaults `--root` to the current working directory. Exits non-zero when any
//! product fails descriptor validation, path identity checks, or
//! publisher/subscriber capability resolution.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use traverse_registry::validate_event_product_tree;

fn main() -> ExitCode {
    let root = match parse_args(env::args().skip(1)) {
        ArgsOutcome::Help => {
            println!("Usage: validate_event_products [--root <repo-root>]");
            return ExitCode::SUCCESS;
        }
        ArgsOutcome::Error(message) => {
            eprintln!("validate_event_products: {message}");
            eprintln!("Usage: validate_event_products [--root <repo-root>]");
            return ExitCode::from(2);
        }
        ArgsOutcome::Root(root) => root,
    };

    let report = validate_event_product_tree(&root);
    if report.ok() {
        println!(
            "validate_event_products: passed ({} product(s))",
            report.validated
        );
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "validate_event_products: failed with {} error(s) across {} product(s)",
        report.errors.len(),
        report.validated
    );
    for error in &report.errors {
        eprintln!("{}: {}: {}", error.code, error.path, error.message);
    }
    ExitCode::from(1)
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
            other => {
                return ArgsOutcome::Error(format!("unknown argument '{other}'"));
            }
        }
    }

    ArgsOutcome::Root(root)
}
