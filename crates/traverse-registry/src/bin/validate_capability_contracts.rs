//! CI binary: parse every active capability record (and every listed new or
//! changed record) under `capabilities/` with `traverse-contracts`.
//!
//! Usage: `validate_capability_contracts [--root <repo-root>] [<contract.json> ...]`
//!
//! Defaults `--root` to the current working directory. Positional paths are
//! relative to `--root` and are parsed even when deprecated. Exits non-zero
//! when any gated record fails to parse.

use std::collections::BTreeSet;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use traverse_registry::validate_capability_contract_parse_tree;

const USAGE: &str =
    "Usage: validate_capability_contracts [--root <repo-root>] [<contract.json> ...]";

fn main() -> ExitCode {
    let (root, include) = match parse_args(env::args().skip(1)) {
        ArgsOutcome::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        ArgsOutcome::Error(message) => {
            eprintln!("validate_capability_contracts: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        ArgsOutcome::Run { root, include } => (root, include),
    };

    let report = validate_capability_contract_parse_tree(&root, &include);
    if report.ok() {
        println!(
            "validate_capability_contracts: passed ({} parsed, {} deprecated skipped)",
            report.parsed, report.skipped_deprecated
        );
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "validate_capability_contracts: failed with {} error(s) across {} parsed record(s)",
        report.errors.len(),
        report.parsed
    );
    for error in &report.errors {
        eprintln!("{}: {}: {}", error.code, error.path, error.message);
    }
    ExitCode::from(1)
}

enum ArgsOutcome {
    Help,
    Run {
        root: PathBuf,
        include: BTreeSet<PathBuf>,
    },
    Error(String),
}

fn parse_args(mut args: impl Iterator<Item = String>) -> ArgsOutcome {
    let mut root = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => return ArgsOutcome::Error(format!("unable to read cwd: {err}")),
    };
    let mut include = BTreeSet::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => match args.next() {
                Some(value) => root = PathBuf::from(value),
                None => return ArgsOutcome::Error("--root requires a path argument".to_string()),
            },
            "--help" | "-h" => return ArgsOutcome::Help,
            other if other.starts_with('-') => {
                return ArgsOutcome::Error(format!("unknown argument '{other}'"));
            }
            path => {
                include.insert(PathBuf::from(path));
            }
        }
    }

    ArgsOutcome::Run { root, include }
}
