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

static mut INPUT_BUF: [u8; 16384] = [0; 16384];
static mut OUTPUT_BUF: [u8; 8192] = [0; 8192];

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
    let item = object_after_key_at_depth(input, b"\"item\"", 1);
    let config = object_after_key_at_depth(input, b"\"quality_config\"", 1);
    if item.is_none() || config.is_none() {
        return fail(out, b"invalid_input", b"");
    }
    let item = item.unwrap_or(b"{}");
    let config = config.unwrap_or(b"{}");
    let item_id = extract_string(item, b"\"id\"");
    let status = extract_string(item, b"\"status\"");
    if item_id.is_empty() {
        return fail(out, b"invalid_input", b"");
    }
    if status != b"done" && status != b"completed" {
        return fail(out, b"invalid_status", item_id);
    }

    let note = extract_string(item, b"\"completion_note\"");
    let note_len = note.len() as u32;
    let evidence = array_after_key_at_depth(item, b"\"evidence_refs\"", 1).unwrap_or(b"[]");
    let evidence_count = count_array_strings(evidence);
    let pressure_millis = parse_number_millis(item, b"\"pressure_score\"").unwrap_or(0);

    let high_thresh = parse_number_millis(config, b"\"high_pressure_threshold\"").unwrap_or(700);
    let require_ev =
        extract_bool(config, b"\"require_evidence_when_high_pressure\"").unwrap_or(true);
    let min_note = extract_i32(config, b"\"min_note_length\"").unwrap_or(8) as u32;

    let high_pressure = pressure_millis >= high_thresh;
    let short_note = note_len < min_note;
    let missing_evidence = evidence_count == 0;

    let mut gaps: [&[u8]; 2] = [b"", b""];
    let mut gap_n = 0usize;
    if short_note {
        gaps[gap_n] = b"short_note";
        gap_n += 1;
    }
    if high_pressure && require_ev && missing_evidence {
        gaps[gap_n] = b"missing_evidence";
        gap_n += 1;
    }

    // Score: base 0.5 + note contribution + evidence contribution - penalties
    let mut score: u32 = 500;
    if note_len >= min_note {
        score = score.saturating_add(200);
    }
    if note_len >= min_note.saturating_mul(3) {
        score = score.saturating_add(100);
    }
    if evidence_count > 0 {
        score = score.saturating_add(200);
    }
    if short_note {
        score = score.saturating_sub(300);
    }
    if high_pressure && require_ev && missing_evidence {
        score = score.saturating_sub(300);
    }
    if score > 1000 {
        score = 1000;
    }

    let verdict: &[u8] = if gap_n == 0 {
        b"pass" as &[u8]
    } else if high_pressure && require_ev && missing_evidence {
        b"needs_evidence" as &[u8]
    } else if short_note && missing_evidence {
        b"fail" as &[u8]
    } else {
        b"needs_evidence" as &[u8]
    };

    let mut score_buf = [0u8; 16];
    let score_len = format_score_millis(&mut score_buf, score);

    let mut i = 0usize;
    i = copy(out, i, b"{\"item_id\":\"");
    i = copy_json_escaped(out, i, item_id);
    i = copy(out, i, b"\",\"quality_score\":");
    i = copy(out, i, &score_buf[..score_len]);
    i = copy(out, i, b",\"verdict\":\"");
    i = copy(out, i, verdict);
    i = copy(out, i, b"\",\"gaps\":[");
    for g in 0..gap_n {
        if g > 0 {
            i = copy(out, i, b",");
        }
        i = copy(out, i, b"\"");
        i = copy(out, i, gaps[g]);
        i = copy(out, i, b"\"");
    }
    i = copy(
        out,
        i,
        b"],\"reason_code\":\"ok\",\"evaluation_trace\":[\"high_pressure=",
    );
    i = copy(out, i, if high_pressure { b"true" } else { b"false" });
    i = copy(out, i, b"\",\"evidence=");
    i = write_u32(out, i, evidence_count);
    i = copy(out, i, b"\",\"note_len=");
    i = write_u32(out, i, note_len);
    i = copy(out, i, b"\"]}");
    i
}

fn count_array_strings(arr: &[u8]) -> u32 {
    let mut count = 0u32;
    let mut in_str = false;
    let mut i = 0usize;
    while i < arr.len() {
        let b = arr[i];
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
                count += 1;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_str = true;
        }
        i += 1;
    }
    count
}

fn fail(out: &mut [u8], code: &[u8], item_id: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"item_id\":\"");
    i = copy_json_escaped(out, i, item_id);
    i = copy(
        out,
        i,
        b"\",\"quality_score\":0,\"verdict\":\"fail\",\"gaps\":[],\"reason_code\":\"",
    );
    i = copy(out, i, code);
    i = copy(out, i, b"\",\"evaluation_trace\":[]}");
    i
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

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
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
    let rest = skip_ws(&after_key[colon + 1..]);
    if rest.first() != Some(&b'"') {
        return b"";
    }
    let rest = &rest[1..];
    let Some(end) = rest.iter().position(|b| *b == b'"') else {
        return b"";
    };
    &rest[..end]
}

fn extract_string<'a>(hay: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(pos) = find(hay, key) else {
        return b"";
    };
    string_value_after(&hay[pos + key.len()..])
}

fn object_after_key_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> Option<&'a [u8]> {
    let pos = find_key_at_depth(hay, key, depth)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    if rest.first() != Some(&b'{') {
        return None;
    }
    let end = balanced_end(rest, b'{', b'}')?;
    Some(&rest[..=end])
}

fn array_after_key_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> Option<&'a [u8]> {
    let pos = find_key_at_depth(hay, key, depth)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    if rest.first() != Some(&b'[') {
        return None;
    }
    let end = balanced_end(rest, b'[', b']')?;
    Some(&rest[..=end])
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

fn extract_i32(hay: &[u8], key: &[u8]) -> Option<i32> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    parse_i32(rest)
}

fn parse_i32(rest: &[u8]) -> Option<i32> {
    if rest.is_empty() {
        return None;
    }
    let mut neg = false;
    let mut j = 0usize;
    if rest[0] == b'-' {
        neg = true;
        j = 1;
    }
    if j >= rest.len() || rest[j] < b'0' || rest[j] > b'9' {
        return None;
    }
    let mut n: i32 = 0;
    while j < rest.len() && rest[j] >= b'0' && rest[j] <= b'9' {
        n = n * 10 + (rest[j] - b'0') as i32;
        j += 1;
    }
    Some(if neg { -n } else { n })
}

fn parse_number_millis(hay: &[u8], key: &[u8]) -> Option<u32> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    if rest.is_empty() {
        return None;
    }
    let mut whole: u32 = 0;
    let mut frac: u32 = 0;
    let mut frac_digits = 0u32;
    let mut seen_dot = false;
    let mut j = 0usize;
    while j < rest.len() {
        let b = rest[j];
        if b == b',' || b == b'}' || b == b']' || b == b' ' || b == b'\n' {
            break;
        }
        if b == b'.' {
            seen_dot = true;
            j += 1;
            continue;
        }
        if b < b'0' || b > b'9' {
            break;
        }
        let digit = (b - b'0') as u32;
        if seen_dot {
            if frac_digits < 3 {
                frac = frac * 10 + digit;
                frac_digits += 1;
            }
        } else {
            whole = whole * 10 + digit;
        }
        j += 1;
    }
    while frac_digits < 3 {
        frac *= 10;
        frac_digits += 1;
    }
    Some(whole * 1000 + frac)
}

fn copy(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let end = at + bytes.len();
    if end > out.len() {
        return at;
    }
    out[at..end].copy_from_slice(bytes);
    end
}

fn copy_json_escaped(out: &mut [u8], mut i: usize, s: &[u8]) -> usize {
    for &b in s {
        match b {
            b'"' => i = copy(out, i, b"\\\""),
            b'\\' => i = copy(out, i, b"\\\\"),
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

fn write_u32(out: &mut [u8], mut i: usize, mut n: u32) -> usize {
    if n == 0 {
        if i < out.len() {
            out[i] = b'0';
            return i + 1;
        }
        return i;
    }
    let mut digits = [0u8; 10];
    let mut d = 0usize;
    while n > 0 {
        digits[d] = b'0' + (n % 10) as u8;
        n /= 10;
        d += 1;
    }
    while d > 0 {
        d -= 1;
        if i < out.len() {
            out[i] = digits[d];
            i += 1;
        }
    }
    i
}

fn format_score_millis(out: &mut [u8], millis: u32) -> usize {
    let whole = millis / 1000;
    let frac = millis % 1000;
    let mut i = write_u32(out, 0, whole);
    i = copy(out, i, b".");
    // always 3 digits for determinism in contract examples we may trim; write without trailing zeros carefully
    // Use up to 3 digits, trim trailing zeros but keep at least one if frac!=0? Contract examples use 0.785 / 1.0
    if frac == 0 {
        i = copy(out, i, b"0");
        return i;
    }
    let d0 = (frac / 100) as u8;
    let d1 = ((frac / 10) % 10) as u8;
    let d2 = (frac % 10) as u8;
    out[i] = b'0' + d0;
    i += 1;
    if d1 != 0 || d2 != 0 {
        out[i] = b'0' + d1;
        i += 1;
        if d2 != 0 {
            out[i] = b'0' + d2;
            i += 1;
        }
    }
    i
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
        let out = run("{\"item\":{\"id\":\"ai-1\",\"title\":\"Send the revised proposal\",\"status\":\"done\",\"pressure_score\":0.9,\"completion_note\":\"Sent revised proposal to stakeholders\",\"evidence_refs\":[\"doc-123\"]},\"quality_config\":{\"version\":\"1.0\",\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_02_happy() {
        let out = run("{\"item\":{\"id\":\"ai-2\",\"title\":\"Fix production bug\",\"status\":\"done\",\"pressure_score\":0.95,\"completion_note\":\"done\",\"evidence_refs\":[]},\"quality_config\":{\"version\":\"1.0\",\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_03_sad() {
        let out = run("{\"item\":{\"id\":\"ai-1\",\"title\":\"X\",\"status\":\"open\",\"pressure_score\":0.5,\"completion_note\":\"still working\",\"evidence_refs\":[]},\"quality_config\":{\"version\":\"1.0\",\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"reason_code\":\"invalid_status\""),
            "expected invalid_status in {out}"
        );
    }

    #[test]
    fn use_case_04_sad() {
        let out = run("{\"item\":{\"id\":\"\",\"title\":\"X\",\"status\":\"done\",\"pressure_score\":0.5,\"completion_note\":\"done enough\",\"evidence_refs\":[]},\"quality_config\":{\"version\":\"1.0\",\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn missing_item_or_config_yields_invalid_input() {
        let out = run("{}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn short_note_and_missing_evidence_under_low_pressure_fails() {
        let out = run("{\"item\":{\"id\":\"ai-1\",\"status\":\"done\",\"pressure_score\":0.1,\"completion_note\":\"hi\",\"evidence_refs\":[]},\"quality_config\":{\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"verdict\":\"fail\""),
            "expected fail verdict in {out}"
        );
        assert!(out.contains("\"gaps\":[\"short_note\"]"));
    }

    #[test]
    fn short_note_and_missing_evidence_under_high_pressure_needs_evidence_with_both_gaps() {
        let out = run("{\"item\":{\"id\":\"ai-1\",\"status\":\"done\",\"pressure_score\":0.9,\"completion_note\":\"hi\",\"evidence_refs\":[]},\"quality_config\":{\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"verdict\":\"needs_evidence\""),
            "expected needs_evidence verdict in {out}"
        );
        assert!(out.contains("\"gaps\":[\"short_note\",\"missing_evidence\"]"));
    }

    #[test]
    fn short_note_with_evidence_present_needs_evidence() {
        let out = run("{\"item\":{\"id\":\"ai-1\",\"status\":\"done\",\"pressure_score\":0.1,\"completion_note\":\"hi\",\"evidence_refs\":[\"doc-1\"]},\"quality_config\":{\"high_pressure_threshold\":0.7,\"require_evidence_when_high_pressure\":true,\"min_note_length\":8}}");
        assert!(
            out.contains("\"verdict\":\"needs_evidence\""),
            "expected needs_evidence verdict in {out}"
        );
        assert!(out.contains("\"gaps\":[\"short_note\"]"));
    }

    #[test]
    fn extract_bool_handles_false_and_neither() {
        assert_eq!(extract_bool(b"\"k\":false", b"\"k\""), Some(false));
        assert_eq!(extract_bool(b"\"k\":maybe", b"\"k\""), None);
    }

    #[test]
    fn extract_i32_handles_none() {
        assert_eq!(extract_i32(b"{}", b"\"missing\""), None);
        assert_eq!(extract_i32(b"\"k\":oops", b"\"k\""), None);
    }

    #[test]
    fn parse_number_millis_handles_missing_and_non_digit() {
        assert_eq!(parse_number_millis(b"{}", b"\"missing\""), None);
        assert_eq!(parse_number_millis(b"\"k\":oops", b"\"k\""), Some(0));
    }

    #[test]
    fn object_after_key_at_depth_handles_missing_and_non_object() {
        assert_eq!(object_after_key_at_depth(b"{}", b"\"missing\"", 1), None);
        assert_eq!(object_after_key_at_depth(b"\"k\":5", b"\"k\"", 0), None);
    }

    #[test]
    fn array_after_key_at_depth_handles_non_array() {
        assert_eq!(array_after_key_at_depth(b"\"k\":5", b"\"k\"", 0), None);
    }

    #[test]
    fn string_value_after_handles_missing_colon_quote_and_terminator() {
        assert_eq!(string_value_after(b"no colon here"), b"");
        assert_eq!(string_value_after(b":not-a-quote"), b"");
        assert_eq!(string_value_after(b":\"unterminated"), b"");
    }

    #[test]
    fn balanced_end_returns_none_when_unterminated() {
        assert_eq!(balanced_end(b"{\"a\":\"b\"", b'{', b'}'), None);
    }

    #[test]
    fn find_key_at_depth_skips_escaped_characters_in_strings() {
        assert_eq!(
            find_key_at_depth(b"\"a\":\"x\\\"y\",\"b\":1", b"\"b\"", 0),
            Some(11)
        );
    }

    #[test]
    fn count_array_strings_skips_escaped_quotes() {
        assert_eq!(count_array_strings(b"[\"a\\\"b\",\"c\"]"), 2);
    }
}
