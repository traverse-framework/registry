//! CI binary: export `AsyncAPI` documents for every on-disk ECCA event product.
//!
//! Usage: `export_async_api [--root <repo-root>] [--out <output-dir>]`
//!
//! Defaults `--root` to the current working directory and `--out` to
//! `<root>/catalog/asyncapi`. Writes one
//! `<id>@<version>.json` file per `events/**/product.json`, regenerated from
//! the governed descriptor (specs/016 FR-015).

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use traverse_registry::export_async_api_tree;

fn main() -> ExitCode {
    let args = match parse_args(env::args().skip(1)) {
        ArgsOutcome::Help => {
            println!("Usage: export_async_api [--root <repo-root>] [--out <output-dir>]");
            return ExitCode::SUCCESS;
        }
        ArgsOutcome::Error(message) => {
            eprintln!("export_async_api: {message}");
            eprintln!("Usage: export_async_api [--root <repo-root>] [--out <output-dir>]");
            return ExitCode::from(2);
        }
        ArgsOutcome::Ok(args) => args,
    };

    let report = export_async_api_tree(&args.root, &args.out);
    if report.ok() {
        println!(
            "export_async_api: wrote {} document(s) under {}",
            report.written,
            args.out.display()
        );
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "export_async_api: failed with {} error(s); wrote {} document(s)",
        report.errors.len(),
        report.written
    );
    for error in &report.errors {
        eprintln!("{}: {}: {}", error.code, error.path, error.message);
    }
    ExitCode::from(1)
}

struct Args {
    root: PathBuf,
    out: PathBuf,
}

enum ArgsOutcome {
    Help,
    Ok(Args),
    Error(String),
}

fn parse_args(mut args: impl Iterator<Item = String>) -> ArgsOutcome {
    let mut root = match env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => return ArgsOutcome::Error(format!("unable to read cwd: {err}")),
    };
    let mut out: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => match args.next() {
                Some(value) => root = PathBuf::from(value),
                None => return ArgsOutcome::Error("--root requires a path argument".to_string()),
            },
            "--out" => match args.next() {
                Some(value) => out = Some(PathBuf::from(value)),
                None => return ArgsOutcome::Error("--out requires a path argument".to_string()),
            },
            "--help" | "-h" => return ArgsOutcome::Help,
            other => {
                return ArgsOutcome::Error(format!("unknown argument '{other}'"));
            }
        }
    }

    let out = out.unwrap_or_else(|| root.join("catalog/asyncapi"));
    ArgsOutcome::Ok(Args { root, out })
}
