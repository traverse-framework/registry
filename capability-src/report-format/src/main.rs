//! Real, deterministic markdown report formatter for `report.format`.
//!
//! Fifth node of the `report.*` chain (registry#431 / epic #426). Renders
//! structured facts and a summary/translation into a fixed-section report
//! with stable whitespace. No model, network, randomness, or host state.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;

use wasi_capability_runtime::{object, Value};

/// Fixed title line for every report.
const TITLE: &str = "# Rollout Status Report";

fn project_line(project_name: Option<&str>) -> String {
    let name = project_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("(unnamed)");
    format!("Project: {name}")
}

fn build_report(
    structured_facts: &[String],
    summary_or_translation: &str,
    project_name: Option<&str>,
) -> String {
    let mut out = String::new();
    out.push_str(TITLE);
    out.push('\n');
    out.push_str(&project_line(project_name));
    out.push('\n');
    out.push_str(&format!("Insights ({}):", structured_facts.len()));
    out.push('\n');
    for fact in structured_facts {
        out.push_str("- ");
        out.push_str(fact);
        out.push('\n');
    }
    out.push_str("Summary:");
    out.push('\n');
    out.push_str(summary_or_translation);
    out.push('\n');
    out
}

fn format_report(input: Value) -> Value {
    let structured_facts = input
        .get("structured_facts")
        .map(Value::string_array)
        .unwrap_or_default();
    let summary_or_translation = input
        .get("summary_or_translation")
        .and_then(Value::as_str)
        .unwrap_or("");
    let project_name = input.get("project_name").and_then(Value::as_str);

    let report = build_report(&structured_facts, summary_or_translation, project_name);
    object(alloc::vec![("report", Value::String(report))])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(format_report);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("parse");
        format_report(input)
    }

    fn report_of(out: &Value) -> &str {
        out.get("report").unwrap().as_str().unwrap()
    }

    #[test]
    fn zero_facts_renders_insights_zero() {
        let out = call(
            r#"{"structured_facts":[],"summary_or_translation":"Nothing to report.","project_name":"Acme"}"#,
        );
        assert_eq!(
            report_of(&out),
            "# Rollout Status Report\nProject: Acme\nInsights (0):\nSummary:\nNothing to report.\n"
        );
    }

    #[test]
    fn missing_project_name_uses_unnamed() {
        let out = call(r#"{"structured_facts":["a"],"summary_or_translation":"s"}"#);
        assert!(report_of(&out).contains("Project: (unnamed)\n"));
        assert!(!report_of(&out).contains("Project: a"));
    }

    #[test]
    fn blank_project_name_uses_unnamed() {
        let out =
            call(r#"{"structured_facts":[],"summary_or_translation":"s","project_name":"  "}"#);
        assert!(report_of(&out).contains("Project: (unnamed)\n"));
    }

    #[test]
    fn multiline_summary_preserved_verbatim() {
        let out = call(
            r#"{"structured_facts":["one"],"summary_or_translation":"line1\nline2\nline3","project_name":"Demo"}"#,
        );
        let r = report_of(&out);
        assert!(r.contains("Summary:\nline1\nline2\nline3\n"));
        assert!(r.ends_with("line3\n"));
    }

    #[test]
    fn ordering_preserved() {
        let out = call(
            r#"{"structured_facts":["alpha","beta","gamma"],"summary_or_translation":"done","project_name":"Order"}"#,
        );
        let r = report_of(&out);
        let a = r.find("- alpha\n").expect("alpha");
        let b = r.find("- beta\n").expect("beta");
        let c = r.find("- gamma\n").expect("gamma");
        assert!(a < b && b < c);
        assert!(r.contains("Insights (3):\n"));
    }

    #[test]
    fn full_happy_path_exact_bytes() {
        let out = call(
            r#"{"structured_facts":["Fact: browser — up","Fact: edge — warm"],"summary_or_translation":"Acme looks healthy.","project_name":"Acme Rollout"}"#,
        );
        assert_eq!(
            report_of(&out),
            "# Rollout Status Report\nProject: Acme Rollout\nInsights (2):\n- Fact: browser — up\n- Fact: edge — warm\nSummary:\nAcme looks healthy.\n"
        );
    }

    #[test]
    fn single_trailing_newline() {
        let out =
            call(r#"{"structured_facts":[],"summary_or_translation":"x","project_name":"P"}"#);
        let r = report_of(&out);
        assert!(r.ends_with('\n'));
        assert!(!r.ends_with("\n\n"));
    }

    #[test]
    fn missing_keys_default_safely() {
        let out = call(r#"{}"#);
        assert_eq!(
            report_of(&out),
            "# Rollout Status Report\nProject: (unnamed)\nInsights (0):\nSummary:\n\n"
        );
    }

    #[test]
    fn determinism_byte_for_byte() {
        let json = r#"{"structured_facts":["a","b"],"summary_or_translation":"sum\nline","project_name":"Demo"}"#;
        assert_eq!(
            wasi_capability_runtime::write_json(&call(json)),
            wasi_capability_runtime::write_json(&call(json)),
        );
    }

    #[test]
    fn output_depends_on_input() {
        let a = report_of(&call(
            r#"{"structured_facts":["a"],"summary_or_translation":"one"}"#,
        ))
        .to_string();
        let b = report_of(&call(
            r#"{"structured_facts":["b"],"summary_or_translation":"two"}"#,
        ))
        .to_string();
        assert_ne!(a, b);
    }

    #[test]
    fn title_line_is_fixed() {
        let out = call(r#"{"structured_facts":[],"summary_or_translation":""}"#);
        assert!(report_of(&out).starts_with("# Rollout Status Report\n"));
    }
}
