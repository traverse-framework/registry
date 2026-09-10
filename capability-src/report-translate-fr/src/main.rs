//! Model-free EN→FR translator for `report.translate-fr`.
//!
//! Fourth node of the `report.*` chain (registry#430 / epic #426). Matches
//! PROJECT phrase templates and exact Fact templates first, then applies
//! ordered glossary substitution. No embeddings, model, network, randomness,
//! or host state.
//!
//! `1.1.0` (registry#441): the output also carries `structured_facts`
//! (echoed verbatim) and `summary_or_translation` (identical to
//! `translated_summary` -- the French text) so `report.format` can take
//! every input it needs from this single predecessor on the French path
//! under `browserLocalPlan`'s linear chain search. Inputs and translation
//! logic are unchanged.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{array_of_strings, object, Value};

#[derive(Clone, Copy)]
struct TranslationEntry {
    source: &'static str,
    target: &'static str,
}

fn translation_templates() -> [TranslationEntry; 15] {
    [
        TranslationEntry {
            source: "PROJECT shows how adaptive summarization can combine distributed sources into a richer narrative while still depending on runtime validation across N insight(s).",
            target: "PROJECT montre comment une synthese adaptative peut combiner des sources distribuees dans un recit plus riche tout en restant soumise a la validation du runtime sur N observation(s).",
        },
        TranslationEntry {
            source: "PROJECT combines distributed browser, edge, and cloud evidence into a deterministic operational summary with N validated insight(s).",
            target: "PROJECT combine des preuves distribuees du navigateur, de l'edge et du cloud dans un resume operationnel deterministe avec N observation(s) validee(s).",
        },
        TranslationEntry {
            source: "Fact: browser telemetry confirms the release candidate resolves checkout failures without increasing client memory usage.",
            target: "Fait : la telemetrie du navigateur confirme que la version candidate corrige les echecs de paiement sans augmenter l'utilisation memoire du client.",
        },
        TranslationEntry {
            source: "Fact: Browser telemetry shows strong adoption in three customer regions with localized interface demand.",
            target: "Fait : la telemetrie du navigateur montre une forte adoption dans trois regions clientes avec une demande pour une interface localisee.",
        },
        TranslationEntry {
            source: "Fact: Edge summaries report that French output improves stakeholder review time during rollout coordination.",
            target: "Fait : les syntheses edge indiquent que la sortie en francais ameliore le temps de revue des parties prenantes pendant la coordination du deploiement.",
        },
        TranslationEntry {
            source: "Fact: Cloud analysis indicates that the AI summarizer is currently healthy and produces richer executive narratives.",
            target: "Fait : l'analyse cloud indique que le resumeur IA est actuellement sain et produit des syntheses de direction plus riches.",
        },
        TranslationEntry {
            source: "Fact: The browser shell holds recent customer feedback snippets.",
            target: "Fait : le shell navigateur contient des extraits recents de retours clients.",
        },
        TranslationEntry {
            source: "Fact: An edge cache exposes regional rollout data with low latency.",
            target: "Fait : un cache edge expose les donnees de deploiement regional avec une faible latence.",
        },
        TranslationEntry {
            source: "Fact: A cloud record confirms that contract validation reduced incident handoff time.",
            target: "Fait : un enregistrement cloud confirme que la validation de contrat a reduit le temps de transfert des incidents.",
        },
        TranslationEntry {
            source: "Fact: Local browser data includes a release note timeline and user-facing metrics.",
            target: "Fait : les donnees locales du navigateur incluent une chronologie des notes de version et des metriques visibles par l'utilisateur.",
        },
        TranslationEntry {
            source: "Fact: Edge services expose compatibility summaries for active deployments.",
            target: "Fait : les services edge exposent des syntheses de compatibilite pour les deploiements actifs.",
        },
        TranslationEntry {
            source: "Fact: Cloud analysis indicates that deterministic summaries are still accurate enough for this request.",
            target: "Fait : l'analyse cloud indique que les resumes deterministes restent suffisamment precis pour cette demande.",
        },
        TranslationEntry {
            source: "Fact: edge execution keeps personalization latency below regional service-level objectives.",
            target: "Fait : l'execution en edge maintient la latence de personnalisation sous les objectifs de niveau de service regionaux.",
        },
        TranslationEntry {
            source: "Fact: cloud coordination keeps rollout policy changes synchronized across regions.",
            target: "Fait : la coordination cloud maintient les changements de politique de deploiement synchronises entre les regions.",
        },
        TranslationEntry {
            source: "Fact: regional rollout status is stable enough to shift from incident response to operational planning.",
            target: "Fait : l'etat du deploiement regional est assez stable pour passer de la reponse aux incidents a la planification operationnelle.",
        },
    ]
}

fn extract_first_number(text: &str) -> Option<String> {
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            return Some(current);
        }
    }
    if current.is_empty() {
        None
    } else {
        Some(current)
    }
}

fn render_project_translation(project: &str, insight_count: &str, target: &str) -> String {
    target
        .replace("PROJECT", project)
        .replace("N", insight_count)
}

fn try_project_template(text: &str) -> Option<String> {
    let templates = translation_templates();
    let insight_count = extract_first_number(text).unwrap_or_else(|| String::from("0"));

    if text.contains("combines distributed browser") {
        let project = text
            .split(" combines distributed browser")
            .next()
            .unwrap_or("The project");
        return Some(render_project_translation(
            project,
            &insight_count,
            templates[1].target,
        ));
    }

    if text.contains("shows how adaptive summarization") {
        let project = text
            .split(" shows how adaptive summarization")
            .next()
            .unwrap_or("The project");
        return Some(render_project_translation(
            project,
            &insight_count,
            templates[0].target,
        ));
    }

    None
}

fn exact_fact_template(text: &str) -> Option<&'static str> {
    for entry in translation_templates() {
        if entry.source.starts_with("Fact:") && entry.source == text {
            return Some(entry.target);
        }
    }
    None
}

fn glossary_translate(text: &str) -> String {
    let replacements = [
        ("Fact:", "Fait :"),
        ("browser telemetry", "telemetrie du navigateur"),
        ("release candidate", "version candidate"),
        ("checkout failures", "echecs de paiement"),
        ("client memory usage", "utilisation memoire du client"),
        ("edge execution", "execution en edge"),
        ("personalization latency", "latence de personnalisation"),
        (
            "regional service-level objectives",
            "objectifs de niveau de service regionaux",
        ),
        ("cloud coordination", "coordination cloud"),
        (
            "rollout policy changes",
            "changements de politique de deploiement",
        ),
        ("across regions", "entre les regions"),
        ("regional rollout status", "etat du deploiement regional"),
        ("incident response", "reponse aux incidents"),
        ("operational planning", "planification operationnelle"),
        ("strong adoption", "forte adoption"),
        ("customer regions", "regions clientes"),
        (
            "localized interface demand",
            "demande pour une interface localisee",
        ),
        ("French output", "sortie en francais"),
        (
            "stakeholder review time",
            "temps de revue des parties prenantes",
        ),
        ("rollout coordination", "coordination du deploiement"),
        ("AI summarizer", "resumeur IA"),
        (
            "richer executive narratives",
            "syntheses de direction plus riches",
        ),
        ("browser shell", "shell navigateur"),
        ("customer feedback snippets", "extraits de retours clients"),
        ("edge cache", "cache edge"),
        ("regional rollout data", "donnees de deploiement regional"),
        ("low latency", "faible latence"),
        ("cloud record", "enregistrement cloud"),
        ("contract validation", "validation de contrat"),
        ("incident handoff time", "temps de transfert des incidents"),
        ("compatibility summaries", "syntheses de compatibilite"),
        ("active deployments", "deploiements actifs"),
        ("deterministic summaries", "resumes deterministes"),
        ("distributed sources", "sources distribuees"),
        ("runtime validation", "validation du runtime"),
        (
            "deterministic operational summary",
            "resume operationnel deterministe",
        ),
        ("validated insight(s)", "observation(s) validee(s)"),
        ("insight(s)", "observation(s)"),
        ("project", "projet"),
        ("summary", "resume"),
    ];

    let mut translated = String::from(text);
    for (english, french) in replacements {
        translated = translated.replace(english, french);
        translated = translated.replace(&english.to_lowercase(), french);
    }
    translated
}

fn translate_one(text: &str) -> String {
    let normalized = text.trim();
    if normalized.is_empty() {
        return String::new();
    }

    if let Some(rendered) = try_project_template(normalized) {
        return rendered;
    }

    if let Some(target) = exact_fact_template(normalized) {
        return String::from(target);
    }

    glossary_translate(normalized)
}

fn translate(input: Value) -> Value {
    let summary = input.get("summary").and_then(Value::as_str).unwrap_or("");
    let structured_facts = input
        .get("structured_facts")
        .map(Value::string_array)
        .unwrap_or_default();

    let translated_summary = translate_one(summary);
    let translated_facts: Vec<String> = structured_facts
        .iter()
        .map(|fact| translate_one(fact))
        .collect();

    // `structured_facts` echoed verbatim and `summary_or_translation` set to the
    // French text so `report.format` can draw both of its inputs from this
    // single predecessor on the French path -- `browserLocalPlan` only extends a
    // chain by one predecessor that covers the whole remaining gap (registry#441).
    object(alloc::vec![
        ("structured_facts", array_of_strings(&structured_facts)),
        (
            "translated_summary",
            Value::String(translated_summary.clone()),
        ),
        ("translated_facts", array_of_strings(&translated_facts)),
        ("summary_or_translation", Value::String(translated_summary)),
    ])
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(translate);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("parse");
        translate(input)
    }

    fn summary_of(out: &Value) -> &str {
        out.get("translated_summary").unwrap().as_str().unwrap()
    }

    fn facts_of(out: &Value) -> Vec<String> {
        out.get("translated_facts").unwrap().string_array()
    }

    #[test]
    fn echoes_structured_facts_verbatim() {
        let fact = "Fact: An edge cache exposes regional rollout data with low latency.";
        let out = call(&alloc::format!(
            r#"{{"summary":"","structured_facts":["{fact}","",  "raw note"]}}"#
        ));
        // echoed field carries the ORIGINAL English facts, untouched.
        assert_eq!(
            out.get("structured_facts").unwrap().string_array(),
            vec![String::from(fact), String::new(), String::from("raw note"),]
        );
    }

    #[test]
    fn summary_or_translation_equals_translated_summary() {
        let out = call(
            r#"{"summary":"Acme combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 2 validated insights.","structured_facts":[]}"#,
        );
        assert_eq!(
            out.get("summary_or_translation").unwrap().as_str().unwrap(),
            summary_of(&out)
        );
        assert!(summary_of(&out).starts_with("Acme combine des preuves"));
    }

    #[test]
    fn echoes_empty_structured_facts() {
        let out = call(r#"{"summary":"","structured_facts":[]}"#);
        assert!(out
            .get("structured_facts")
            .unwrap()
            .string_array()
            .is_empty());
    }

    #[test]
    fn project_combines_template_fills_placeholders() {
        let out = call(
            r#"{"summary":"Acme Rollout combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 2 validated insights.","structured_facts":[]}"#,
        );
        assert_eq!(
            summary_of(&out),
            "Acme Rollout combine des preuves distribuees du navigateur, de l'edge et du cloud dans un resume operationnel deterministe avec 2 observation(s) validee(s)."
        );
        assert!(facts_of(&out).is_empty());
    }

    #[test]
    fn project_shows_how_template_fills_placeholders() {
        let out = call(
            r#"{"summary":"Beta Kit shows how adaptive summarization can combine distributed sources into a richer narrative while still depending on runtime validation across 7 insight(s).","structured_facts":[]}"#,
        );
        assert_eq!(
            summary_of(&out),
            "Beta Kit montre comment une synthese adaptative peut combiner des sources distribuees dans un recit plus riche tout en restant soumise a la validation du runtime sur 7 observation(s)."
        );
    }

    #[test]
    fn project_template_defaults_missing_number_to_zero() {
        let out = call(
            r#"{"summary":"Demo combines distributed browser, edge, and cloud evidence into a deterministic operational summary with validated insights.","structured_facts":[]}"#,
        );
        assert!(summary_of(&out).contains("avec 0 observation(s) validee(s)."));
    }

    #[test]
    fn exact_fact_template_match() {
        let fact = "Fact: An edge cache exposes regional rollout data with low latency.";
        let out = call(&alloc::format!(
            r#"{{"summary":"","structured_facts":["{fact}"]}}"#
        ));
        assert_eq!(summary_of(&out), "");
        assert_eq!(
            facts_of(&out),
            vec![String::from(
                "Fait : un cache edge expose les donnees de deploiement regional avec une faible latence."
            )]
        );
    }

    #[test]
    fn glossary_only_path() {
        let out = call(
            r#"{"summary":"Fact: browser telemetry and low latency matter.","structured_facts":["edge cache helps"]}"#,
        );
        assert_eq!(
            summary_of(&out),
            "Fait : telemetrie du navigateur and faible latence matter."
        );
        assert_eq!(facts_of(&out), vec![String::from("cache edge helps")]);
    }

    #[test]
    fn passthrough_when_no_glossary_hit() {
        let out = call(r#"{"summary":"zzzz unique token","structured_facts":["qqqq"]}"#);
        assert_eq!(summary_of(&out), "zzzz unique token");
        assert_eq!(facts_of(&out), vec![String::from("qqqq")]);
    }

    #[test]
    fn empty_string_stays_empty() {
        let out = call(r#"{"summary":"   ","structured_facts":["", "  "]}"#);
        assert_eq!(summary_of(&out), "");
        assert_eq!(facts_of(&out), vec![String::new(), String::new()]);
    }

    #[test]
    fn missing_keys_default_safely() {
        let out = call(r#"{}"#);
        assert_eq!(summary_of(&out), "");
        assert!(facts_of(&out).is_empty());
    }

    #[test]
    fn extract_first_number_at_end() {
        assert_eq!(
            extract_first_number("count is 42"),
            Some(String::from("42"))
        );
        assert_eq!(extract_first_number("no digits"), None);
    }

    #[test]
    fn determinism_byte_for_byte() {
        let json = r#"{"summary":"Acme combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 1 validated insight.","structured_facts":["Fact: The browser shell holds recent customer feedback snippets.","partial edge cache note"]}"#;
        assert_eq!(
            wasi_capability_runtime::write_json(&call(json)),
            wasi_capability_runtime::write_json(&call(json)),
        );
    }

    #[test]
    fn output_depends_on_input() {
        let a = wasi_capability_runtime::write_json(&call(
            r#"{"summary":"Acme combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 1 validated insight.","structured_facts":[]}"#,
        ));
        let b = wasi_capability_runtime::write_json(&call(
            r#"{"summary":"zzzz","structured_facts":[]}"#,
        ));
        assert_ne!(a, b);
    }
}
