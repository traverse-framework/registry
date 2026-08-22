//! core.generate-nudge-message — pure intensity×tone message templates.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[repr(C)]
struct IoVec {
    buffer: *const u8,
    length: usize,
}
#[repr(C)]
struct IoVecMut {
    buffer: *mut u8,
    length: usize,
}

#[cfg(not(test))]
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IoVecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const IoVec, count: usize, written: *mut usize) -> u32;
}

static mut INPUT_BUF: [u8; 8192] = [0; 8192];
static mut OUTPUT_BUF: [u8; 4096] = [0; 4096];

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        loop {
            let vec = IoVecMut {
                buffer: INPUT_BUF.as_mut_ptr().add(total),
                length: INPUT_BUF.len() - total,
            };
            let mut n = 0usize;
            if fd_read(0, &vec, 1, &mut n) != 0 || n == 0 {
                break;
            }
            total += n;
            if total >= INPUT_BUF.len() {
                break;
            }
        }
        let out_len = evaluate(&INPUT_BUF[..total], &mut OUTPUT_BUF);
        let out = IoVec {
            buffer: OUTPUT_BUF.as_ptr(),
            length: out_len,
        };
        let mut written = 0usize;
        let _ = fd_write(1, &out, 1, &mut written);
    }
}

pub unsafe fn evaluate(input: &[u8], out: &mut [u8]) -> usize {
    let item = object_after_key(input, b"\"item\"").unwrap_or(b"");
    let intensity = extract_string_at_depth(input, b"\"intensity\"", 1);
    let config = object_after_key(input, b"\"message_config\"").unwrap_or(b"");
    if item.is_empty() || intensity.is_empty() || config.is_empty() {
        return fail(out, b"config_error", br#"["missing required fields"]"#);
    }
    if !matches!(intensity, b"soft" | b"direct" | b"escalate") {
        return fail(
            out,
            b"invalid_intensity",
            br#"["intensity must be soft|direct|escalate"]"#,
        );
    }
    let tone = extract_string(config, b"\"tone\"");
    let tone = if tone.is_empty() { b"friendly" } else { tone };
    let include_due = extract_bool(config, b"\"include_due_date\"").unwrap_or(true);
    let owner = extract_string(item, b"\"owner_name\"");
    let title = extract_string(item, b"\"title\"");
    let due = extract_string(item, b"\"due_date\"");
    if title.is_empty() {
        return fail(out, b"config_error", br#"["item.title required"]"#);
    }

    // Build message into a temp buffer.
    let mut msg = [0u8; 1024];
    let mut m = 0usize;
    match (intensity, tone) {
        (b"soft", _) => {
            m = copy(&mut msg, m, b"Hey ");
            m = copy(&mut msg, m, if owner.is_empty() { b"there" } else { owner });
            m = copy(&mut msg, m, b" - friendly reminder that \"");
            m = copy(&mut msg, m, title);
            m = copy(&mut msg, m, b"\"");
            if include_due && !due.is_empty() {
                m = copy(&mut msg, m, b" is due ");
                m = copy(&mut msg, m, due);
            }
            m = copy(&mut msg, m, b". Let us know if you need anything!");
        }
        (b"direct", _) => {
            m = copy(&mut msg, m, b"");
            if !owner.is_empty() {
                m = copy(&mut msg, m, owner);
                m = copy(&mut msg, m, b": ");
            }
            m = copy(&mut msg, m, b"Please complete \"");
            m = copy(&mut msg, m, title);
            m = copy(&mut msg, m, b"\"");
            if include_due && !due.is_empty() {
                m = copy(&mut msg, m, b" by ");
                m = copy(&mut msg, m, due);
            }
            m = copy(&mut msg, m, b".");
        }
        _ => {
            m = copy(&mut msg, m, b"Escalation: \"");
            m = copy(&mut msg, m, title);
            m = copy(&mut msg, m, b"\" still needs attention");
            if !owner.is_empty() {
                m = copy(&mut msg, m, b" from ");
                m = copy(&mut msg, m, owner);
            }
            if include_due && !due.is_empty() {
                m = copy(&mut msg, m, b" (due ");
                m = copy(&mut msg, m, due);
                m = copy(&mut msg, m, b")");
            }
            m = copy(&mut msg, m, b".");
        }
    }

    let mut prev = [0u8; 256];
    let mut p = 0usize;
    p = copy(&mut prev, p, b"Reminder: ");
    p = copy(&mut prev, p, title);

    let mut i = 0usize;
    i = copy(out, i, b"{\"message\":\"");
    i = copy_json_escaped(out, i, &msg[..m]);
    i = copy(out, i, b"\",\"preview\":\"");
    i = copy_json_escaped(out, i, &prev[..p]);
    i = copy(
        out,
        i,
        b"\",\"reason_code\":\"ok\",\"evaluation_trace\":[\"intensity=",
    );
    i = copy(out, i, intensity);
    i = copy(out, i, b"\",\"tone=");
    i = copy(out, i, tone);
    i = copy(out, i, b"\"]}");
    i
}

fn fail(out: &mut [u8], code: &[u8], trace: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(
        out,
        i,
        b"{\"message\":\"\",\"preview\":\"\",\"reason_code\":\"",
    );
    i = copy(out, i, code);
    i = copy(out, i, b"\",\"evaluation_trace\":");
    i = copy(out, i, trace);
    i = copy(out, i, b"}");
    i
}

fn copy_json_escaped(out: &mut [u8], mut i: usize, s: &[u8]) -> usize {
    for &b in s {
        match b {
            b'"' => {
                i = copy(out, i, b"\\\"");
            }
            b'\\' => {
                i = copy(out, i, b"\\\\");
            }
            _ => {
                if i < out.len() {
                    out[i] = b;
                    i += 1;
                }
            }
        }
    }
    i
}

fn object_after_key<'a>(hay: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find_key_at_depth(hay, key, 1)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    if rest.first() != Some(&b'{') {
        return None;
    }
    let end = balanced_end(rest, b'{', b'}')?;
    Some(&rest[..=end])
}

fn skip_ws(s: &[u8]) -> &[u8] {
    let mut rest = s;
    while rest
        .first()
        .is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\t' | b'\r'))
    {
        rest = &rest[1..];
    }
    rest
}

fn balanced_end(s: &[u8], open: u8, close: u8) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i < s.len() {
        let b = s[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn extract_string<'a>(hay: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(pos) = find(hay, key) else {
        return b"";
    };
    string_value_after(&hay[pos + key.len()..])
}

fn extract_string_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> &'a [u8] {
    let Some(pos) = find_key_at_depth(hay, key, depth) else {
        return b"";
    };
    string_value_after(&hay[pos + key.len()..])
}

fn find_key_at_depth(hay: &[u8], key: &[u8], target_depth: i32) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut i = 0usize;
    while i + key.len() <= hay.len() {
        let b = hay[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => {
                if depth == target_depth && hay[i..].starts_with(key) {
                    return Some(i);
                }
                in_str = true;
            }
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    None
}

fn string_value_after<'a>(after_key: &'a [u8]) -> &'a [u8] {
    let Some(colon) = after_key.iter().position(|b| *b == b':') else {
        return b"";
    };
    let mut rest = skip_ws(&after_key[colon + 1..]);
    if rest.first() != Some(&b'"') {
        return b"";
    }
    rest = &rest[1..];
    let Some(end) = rest.iter().position(|b| *b == b'"') else {
        return b"";
    };
    &rest[..end]
}

fn extract_bool(hay: &[u8], key: &[u8]) -> Option<bool> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    if rest.starts_with(b"true") {
        Some(true)
    } else if rest.starts_with(b"false") {
        Some(false)
    } else {
        None
    }
}

fn copy(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let end = at + bytes.len();
    if end > out.len() {
        return at;
    }
    out[at..end].copy_from_slice(bytes);
    end
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod catalog_coverage_tests {
    use super::*;

    fn run(input: &str) -> String {
        let mut out = vec![0u8; 65536];
        let n = unsafe { evaluate(input.as_bytes(), &mut out) };
        String::from_utf8_lossy(&out[..n]).into_owned()
    }

    #[test]
    fn use_case_01_happy() {
        let out = run("{\"item\":{\"id\":\"ai-1\",\"title\":\"Send the revised proposal\",\"owner_name\":\"Ada\",\"due_date\":\"2026-08-09\",\"status\":\"open\",\"nudge_count\":0},\"intensity\":\"soft\",\"message_config\":{\"version\":\"1.0\",\"tone\":\"friendly\",\"include_due_date\":true,\"language\":\"en\"}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_02_happy() {
        let out = run("{\"item\":{\"id\":\"ai-9\",\"title\":\"Close security review\",\"owner_name\":\"Bob\",\"due_date\":\"2026-08-01\",\"status\":\"open\",\"nudge_count\":3},\"intensity\":\"escalate\",\"message_config\":{\"version\":\"1.0\",\"tone\":\"direct\",\"include_due_date\":true,\"language\":\"en\"}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_03_happy() {
        let out = run("{\"item\":{\"id\":\"ai-2\",\"title\":\"Ship docs\",\"owner_name\":\"Ada\",\"due_date\":\"2026-08-12\",\"status\":\"open\",\"nudge_count\":1},\"intensity\":\"direct\",\"message_config\":{\"version\":\"1.0\",\"tone\":\"neutral\",\"include_due_date\":true,\"language\":\"en\"}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_04_sad() {
        let out = run("{\"item\":{\"id\":\"ai-x\",\"title\":\"\",\"owner_name\":\"Ada\",\"due_date\":\"2026-08-09\",\"status\":\"open\",\"nudge_count\":0},\"intensity\":\"soft\",\"message_config\":{\"version\":\"1.0\",\"tone\":\"friendly\",\"include_due_date\":true,\"language\":\"en\"}}");
        assert!(
            out.contains("\"reason_code\":\"config_error\""),
            "expected config_error in {out}"
        );
    }

    #[test]
    fn missing_top_level_fields_yields_config_error() {
        let out = run("{}");
        assert!(
            out.contains("\"reason_code\":\"config_error\""),
            "expected config_error in {out}"
        );
    }

    #[test]
    fn invalid_intensity_is_rejected() {
        let out =
            run("{\"item\":{\"title\":\"T\"},\"intensity\":\"urgent\",\"message_config\":{}}");
        assert!(
            out.contains("\"reason_code\":\"invalid_intensity\""),
            "expected invalid_intensity in {out}"
        );
    }

    #[test]
    fn soft_message_without_owner_or_due_date_uses_defaults() {
        let out = run("{\"item\":{\"title\":\"Ship docs\"},\"intensity\":\"soft\",\"message_config\":{\"include_due_date\":false}}");
        assert!(out.contains("Hey there"));
        assert!(!out.contains(" is due "));
    }

    #[test]
    fn direct_message_without_owner_or_due_date_omits_prefix_and_date() {
        let out = run("{\"item\":{\"title\":\"Ship docs\"},\"intensity\":\"direct\",\"message_config\":{\"include_due_date\":false}}");
        assert!(out.contains("Please complete"));
        assert!(!out.contains(" by "));
    }

    #[test]
    fn escalate_message_without_owner_or_due_date_omits_both() {
        let out = run("{\"item\":{\"title\":\"Ship docs\"},\"intensity\":\"escalate\",\"message_config\":{\"include_due_date\":false}}");
        assert!(out.contains("Escalation"));
        assert!(!out.contains(" from "));
        assert!(!out.contains(" (due "));
    }

    #[test]
    fn tone_defaults_to_friendly_when_missing() {
        let out = run("{\"item\":{\"title\":\"T\"},\"intensity\":\"soft\",\"message_config\":{}}");
        assert!(out.contains("\"tone=friendly\""));
    }

    #[test]
    fn object_after_key_handles_missing_and_non_object() {
        assert_eq!(object_after_key(b"{}", b"\"missing\""), None);
        assert_eq!(object_after_key(br#"{"k":5}"#, b"\"k\""), None);
    }

    #[test]
    fn balanced_end_returns_none_when_unterminated() {
        assert_eq!(balanced_end(b"{\"a\":\"b\"", b'{', b'}'), None);
    }

    #[test]
    fn string_value_after_handles_missing_colon_quote_and_terminator() {
        assert_eq!(string_value_after(b"no colon"), b"");
        assert_eq!(string_value_after(b":not-a-quote"), b"");
        assert_eq!(string_value_after(b":\"unterminated"), b"");
    }

    #[test]
    fn extract_bool_handles_false_and_neither() {
        assert_eq!(extract_bool(b"\"k\":false", b"\"k\""), Some(false));
        assert_eq!(extract_bool(b"\"k\":maybe", b"\"k\""), None);
    }
}
