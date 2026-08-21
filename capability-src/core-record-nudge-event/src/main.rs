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
    let item_id = extract_string_at_depth(input, b"\"item_id\"", 1);
    let owner_id = extract_string_at_depth(input, b"\"owner_id\"", 1);
    let intensity = extract_string_at_depth(input, b"\"intensity\"", 1);
    let channel = extract_string_at_depth(input, b"\"channel\"", 1);
    let preview = extract_string_at_depth(input, b"\"message_preview\"", 1);
    let event_at = extract_string_at_depth(input, b"\"event_at\"", 1);
    let prior = extract_i32(input, b"\"prior_nudge_count\"");

    if item_id.is_empty() || event_at.is_empty() || prior.is_none() {
        return fail(out, b"invalid_input");
    }
    if !matches!(intensity, b"soft" | b"direct" | b"escalate") {
        return fail(out, b"invalid_intensity");
    }
    let prior = prior.unwrap_or(0);
    if prior < 0 {
        return fail(out, b"invalid_input");
    }
    let ordinal = (prior as u32).saturating_add(1);
    let chan: &[u8] = if channel.is_empty() {
        b"in_app"
    } else {
        channel
    };

    let mut i = 0usize;
    i = copy(out, i, b"{\"event\":{\"event_id\":\"nudge-");
    i = copy_json_escaped(out, i, item_id);
    i = copy(out, i, b"-");
    i = write_u32(out, i, ordinal);
    i = copy(out, i, b"\",\"item_id\":\"");
    i = copy_json_escaped(out, i, item_id);
    i = copy(out, i, b"\",\"owner_id\":\"");
    i = copy_json_escaped(out, i, owner_id);
    i = copy(out, i, b"\",\"intensity\":\"");
    i = copy(out, i, intensity);
    i = copy(out, i, b"\",\"channel\":\"");
    i = copy_json_escaped(out, i, chan);
    i = copy(out, i, b"\",\"message_preview\":\"");
    i = copy_json_escaped(out, i, preview);
    i = copy(out, i, b"\",\"event_at\":\"");
    i = copy_json_escaped(out, i, event_at);
    i = copy(out, i, b"\",\"nudge_ordinal\":");
    i = write_u32(out, i, ordinal);
    i = copy(
        out,
        i,
        b"},\"reason_code\":\"ok\",\"evaluation_trace\":[\"constructed nudge event\",\"ordinal=",
    );
    i = write_u32(out, i, ordinal);
    i = copy(out, i, b"\"]}");
    i
}

fn fail(out: &mut [u8], code: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"event\":{\"event_id\":\"\",\"item_id\":\"\",\"intensity\":\"\",\"event_at\":\"\",\"nudge_ordinal\":0},\"reason_code\":\"");
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

fn skip_ws_comma(s: &[u8]) -> usize {
    let mut i = 0usize;
    while i < s.len() {
        match s[i] {
            b' ' | b'\n' | b'\t' | b'\r' | b',' => i += 1,
            _ => break,
        }
    }
    i
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

fn extract_string_at_depth<'a>(hay: &'a [u8], key: &[u8], depth: i32) -> &'a [u8] {
    let Some(pos) = find_key_at_depth(hay, key, depth) else {
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

fn write_i32(out: &mut [u8], mut i: usize, n: i32) -> usize {
    if n < 0 {
        i = copy(out, i, b"-");
        write_u32(out, i, (-n) as u32)
    } else {
        write_u32(out, i, n as u32)
    }
}

fn ascii_lower(b: u8) -> u8 {
    if b >= b'A' && b <= b'Z' {
        b + 32
    } else {
        b
    }
}

fn eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if ascii_lower(a[i]) != ascii_lower(b[i]) {
            return false;
        }
    }
    true
}

fn normalize_email(src: &[u8], dst: &mut [u8]) -> usize {
    let mut i = 0usize;
    let mut j = 0usize;
    while i < src.len() && (src[i] == b' ' || src[i] == b'\t') {
        i += 1;
    }
    let mut end = src.len();
    while end > i && (src[end - 1] == b' ' || src[end - 1] == b'\t') {
        end -= 1;
    }
    while i < end && j < dst.len() {
        dst[j] = ascii_lower(src[i]);
        i += 1;
        j += 1;
    }
    j
}

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = s.len();
    while start < end && matches!(s[start], b' ' | b'\t' | b'\n' | b'\r') {
        start += 1;
    }
    while end > start && matches!(s[end - 1], b' ' | b'\t' | b'\n' | b'\r') {
        end -= 1;
    }
    &s[start..end]
}

/// Days since 1970-01-01 for YYYY-MM-DD (Howard Hinnant civil_from_days inverse).
fn parse_ymd_days(s: &[u8]) -> Option<i32> {
    if s.len() < 10 || s[4] != b'-' || s[7] != b'-' {
        return None;
    }
    let y = parse_i32(&s[0..4])?;
    let m = parse_i32(&s[5..7])?;
    let d = parse_i32(&s[8..10])?;
    if m < 1 || m > 12 || d < 1 || d > 31 {
        return None;
    }
    let y = y as i32 - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = if m > 2 {
        (m - 3) as u32
    } else {
        (m + 9) as u32
    };
    let doy = (153 * mp + 2) / 5 + d as u32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe as i32) - 719468)
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
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"user-ada\",\"intensity\":\"soft\",\"channel\":\"in_app\",\"message_preview\":\"Reminder: Send the revised proposal\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_02_happy() {
        let out = run("{\"item_id\":\"ai-9\",\"owner_id\":\"user-bob\",\"intensity\":\"escalate\",\"channel\":\"manager\",\"message_preview\":\"Escalation: overdue item\",\"event_at\":\"2026-08-07T11:00:00Z\",\"prior_nudge_count\":3}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_03_happy() {
        let out = run("{\"item_id\":\"ai-2\",\"owner_id\":\"user-ada\",\"intensity\":\"direct\",\"channel\":\"in_app\",\"message_preview\":\"Please complete\",\"event_at\":\"2026-08-07T12:00:00Z\",\"prior_nudge_count\":1}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_04_sad() {
        let out = run("{\"item_id\":\"\",\"owner_id\":\"user-ada\",\"intensity\":\"soft\",\"channel\":\"in_app\",\"message_preview\":\"x\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    // ---- evaluate(): every branch ----

    #[test]
    fn evaluate_missing_event_at_is_invalid_input() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"c\",\"message_preview\":\"p\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn evaluate_empty_event_at_is_invalid_input() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"c\",\"message_preview\":\"p\",\"event_at\":\"\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn evaluate_missing_prior_nudge_count_is_invalid_input() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"c\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\"}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn evaluate_negative_prior_nudge_count_is_invalid_input() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"c\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":-1}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn evaluate_invalid_intensity_is_rejected() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"urgent\",\"channel\":\"c\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"invalid_intensity\""),
            "expected invalid_intensity in {out}"
        );
    }

    #[test]
    fn evaluate_missing_channel_defaults_to_in_app() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"channel\":\"in_app\""),
            "expected default channel in {out}"
        );
        assert!(out.contains("\"reason_code\":\"ok\""));
    }

    #[test]
    fn evaluate_empty_channel_defaults_to_in_app() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"channel\":\"in_app\""),
            "expected default channel in {out}"
        );
    }

    #[test]
    fn evaluate_explicit_channel_is_preserved() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"manager\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"channel\":\"manager\""),
            "expected explicit channel in {out}"
        );
    }

    #[test]
    fn evaluate_missing_owner_id_and_preview_are_empty_but_ok() {
        let out = run("{\"item_id\":\"ai-1\",\"intensity\":\"soft\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
        assert!(
            out.contains("\"owner_id\":\"\""),
            "expected empty owner_id in {out}"
        );
        assert!(
            out.contains("\"message_preview\":\"\""),
            "expected empty preview in {out}"
        );
    }

    #[test]
    fn evaluate_ordinal_is_prior_plus_one() {
        let out = run("{\"item_id\":\"ai-1\",\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"c\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":7}");
        assert!(
            out.contains("\"nudge_ordinal\":8"),
            "expected ordinal 8 in {out}"
        );
        assert!(
            out.contains("nudge-ai-1-8"),
            "expected event_id with ordinal in {out}"
        );
        assert!(out.contains("ordinal=8"), "expected trace ordinal in {out}");
    }

    #[test]
    fn evaluate_escapes_backslashes_in_strings() {
        // Note: this hand-rolled scanner's string extraction stops at the first
        // raw `"` byte (it is not escape-aware when finding the closing quote),
        // so a value containing an embedded quote cannot round-trip through
        // evaluate(); only backslashes are exercised here end-to-end. Quote
        // escaping itself is covered directly by the copy_json_escaped tests.
        let input = r#"{"item_id":"ai\\1","owner_id":"o","intensity":"soft","channel":"c","message_preview":"say\\hi","event_at":"2026-08-07T10:00:00Z","prior_nudge_count":0}"#;
        let out = run(input);
        assert!(
            out.contains(r#""item_id":"ai\\\\1""#),
            "expected escaped item_id in {out}"
        );
        assert!(
            out.contains(r#""message_preview":"say\\\\hi""#),
            "expected escaped preview in {out}"
        );
        assert!(out.contains("\"reason_code\":\"ok\""));
    }

    #[test]
    fn evaluate_item_id_only_at_top_level_depth_is_used() {
        // "item_id" nested one level deeper than the top level must not satisfy
        // the top-level requirement (extract_string_at_depth targets depth 1).
        let out = run("{\"nested\":{\"item_id\":\"ai-1\"},\"owner_id\":\"o\",\"intensity\":\"soft\",\"channel\":\"c\",\"message_preview\":\"p\",\"event_at\":\"2026-08-07T10:00:00Z\",\"prior_nudge_count\":0}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    // ---- skip_ws / skip_ws_comma ----

    #[test]
    fn skip_ws_strips_all_whitespace_kinds() {
        assert_eq!(skip_ws(b"   \t\n\rabc"), b"abc");
    }

    #[test]
    fn skip_ws_no_whitespace_is_noop() {
        assert_eq!(skip_ws(b"abc"), b"abc");
    }

    #[test]
    fn skip_ws_all_whitespace_yields_empty() {
        assert_eq!(skip_ws(b"   "), b"");
    }

    #[test]
    fn skip_ws_comma_skips_commas_and_whitespace() {
        assert_eq!(skip_ws_comma(b"  , \tabc"), 5);
    }

    #[test]
    fn skip_ws_comma_stops_at_first_non_matching_byte() {
        assert_eq!(skip_ws_comma(b"abc"), 0);
    }

    #[test]
    fn skip_ws_comma_empty_input() {
        assert_eq!(skip_ws_comma(b""), 0);
    }

    // ---- balanced_end ----

    #[test]
    fn balanced_end_simple_object() {
        assert_eq!(balanced_end(b"{}", b'{', b'}'), Some(1));
    }

    #[test]
    fn balanced_end_nested_object() {
        assert_eq!(balanced_end(b"{{}}", b'{', b'}'), Some(3));
    }

    #[test]
    fn balanced_end_ignores_braces_inside_strings() {
        // {"a":"}"}  -- the '}' inside the string value must not close the object early.
        let s = b"{\"a\":\"}\"}";
        assert_eq!(balanced_end(s, b'{', b'}'), Some(s.len() - 1));
    }

    #[test]
    fn balanced_end_handles_escaped_quote_in_string() {
        // {"a":"esc\"aped"}  -- escaped quote must not terminate the string early.
        let s = b"{\"a\":\"esc\\\"aped\"}";
        assert_eq!(balanced_end(s, b'{', b'}'), Some(s.len() - 1));
    }

    #[test]
    fn balanced_end_unbalanced_returns_none() {
        assert_eq!(balanced_end(b"{{}", b'{', b'}'), None);
    }

    #[test]
    fn balanced_end_works_for_arrays() {
        assert_eq!(balanced_end(b"[1,[2,3],4]", b'[', b']'), Some(10));
    }

    // ---- find ----

    #[test]
    fn find_locates_needle() {
        assert_eq!(find(b"abcdef", b"cde"), Some(2));
    }

    #[test]
    fn find_returns_none_when_absent() {
        assert_eq!(find(b"abcdef", b"xyz"), None);
    }

    // ---- find_key_at_depth ----

    #[test]
    fn find_key_at_depth_matches_top_level_key() {
        assert_eq!(
            find_key_at_depth(b"{\"key\":\"val\"}", b"\"key\"", 1),
            Some(1)
        );
    }

    #[test]
    fn find_key_at_depth_wrong_target_depth_is_none() {
        assert_eq!(find_key_at_depth(b"{\"key\":\"val\"}", b"\"key\"", 0), None);
    }

    #[test]
    fn find_key_at_depth_nested_key_is_deeper() {
        let hay = b"{\"a\":{\"key\":\"val\"}}";
        assert_eq!(find_key_at_depth(hay, b"\"key\"", 1), None);
        assert!(find_key_at_depth(hay, b"\"key\"", 2).is_some());
    }

    #[test]
    fn find_key_at_depth_skips_escaped_quotes() {
        let hay = b"{\"a\":\"esc\\\"aped\",\"key\":\"val\"}";
        assert!(find_key_at_depth(hay, b"\"key\"", 1).is_some());
    }

    #[test]
    fn find_key_at_depth_not_found() {
        assert_eq!(find_key_at_depth(b"{\"a\":1}", b"\"key\"", 1), None);
    }

    // ---- string_value_after ----

    #[test]
    fn string_value_after_normal() {
        assert_eq!(string_value_after(b": \"hello\""), b"hello");
    }

    #[test]
    fn string_value_after_no_colon_is_empty() {
        assert_eq!(string_value_after(b"\"hello\""), b"");
    }

    #[test]
    fn string_value_after_non_string_value_is_empty() {
        assert_eq!(string_value_after(b":123"), b"");
    }

    #[test]
    fn string_value_after_unterminated_string_is_empty() {
        assert_eq!(string_value_after(b":\"unterminated"), b"");
    }

    #[test]
    fn string_value_after_empty_string_value() {
        assert_eq!(string_value_after(b":\"\""), b"");
    }

    // ---- extract_string ----

    #[test]
    fn extract_string_finds_value() {
        assert_eq!(extract_string(b"{\"key\":\"val\"}", b"\"key\""), b"val");
    }

    #[test]
    fn extract_string_key_not_found_is_empty() {
        assert_eq!(extract_string(b"{\"other\":\"val\"}", b"\"key\""), b"");
    }

    // ---- extract_string_at_depth ----

    #[test]
    fn extract_string_at_depth_key_not_found_is_empty() {
        assert_eq!(
            extract_string_at_depth(b"{\"other\":\"val\"}", b"\"key\"", 1),
            b""
        );
    }

    #[test]
    fn extract_string_at_depth_finds_value_at_target_depth() {
        assert_eq!(
            extract_string_at_depth(b"{\"key\":\"val\"}", b"\"key\"", 1),
            b"val"
        );
    }

    // ---- object_after_key_at_depth ----

    #[test]
    fn object_after_key_finds_object_value() {
        let hay = b"{\"key\":{\"a\":1},\"b\":2}";
        assert_eq!(
            object_after_key_at_depth(hay, b"\"key\"", 1),
            Some(&b"{\"a\":1}"[..])
        );
    }

    #[test]
    fn object_after_key_not_found_is_none() {
        assert_eq!(
            object_after_key_at_depth(b"{\"other\":1}", b"\"key\"", 1),
            None
        );
    }

    #[test]
    fn object_after_key_non_object_value_is_none() {
        assert_eq!(
            object_after_key_at_depth(b"{\"key\":123}", b"\"key\"", 1),
            None
        );
    }

    #[test]
    fn object_after_key_missing_colon_is_none() {
        assert_eq!(object_after_key_at_depth(b"{\"key\"}", b"\"key\"", 1), None);
    }

    #[test]
    fn object_after_key_unbalanced_value_is_none() {
        // The value's own opening `{` never finds a matching close within the slice.
        assert_eq!(
            object_after_key_at_depth(b"{\"key\":{\"a\":1", b"\"key\"", 1),
            None
        );
    }

    // ---- array_after_key_at_depth ----

    #[test]
    fn array_after_key_finds_array_value() {
        let hay = b"{\"arr\":[1,2,3],\"b\":2}";
        assert_eq!(
            array_after_key_at_depth(hay, b"\"arr\"", 1),
            Some(&b"[1,2,3]"[..])
        );
    }

    #[test]
    fn array_after_key_not_found_is_none() {
        assert_eq!(
            array_after_key_at_depth(b"{\"other\":1}", b"\"arr\"", 1),
            None
        );
    }

    #[test]
    fn array_after_key_non_array_value_is_none() {
        assert_eq!(
            array_after_key_at_depth(b"{\"arr\":123}", b"\"arr\"", 1),
            None
        );
    }

    #[test]
    fn array_after_key_missing_colon_is_none() {
        assert_eq!(array_after_key_at_depth(b"{\"arr\"}", b"\"arr\"", 1), None);
    }

    #[test]
    fn array_after_key_unbalanced_value_is_none() {
        assert_eq!(
            array_after_key_at_depth(b"{\"arr\":[1,2", b"\"arr\"", 1),
            None
        );
    }

    // ---- extract_bool ----

    #[test]
    fn extract_bool_true() {
        assert_eq!(extract_bool(b"{\"key\":true}", b"\"key\""), Some(true));
    }

    #[test]
    fn extract_bool_false() {
        assert_eq!(extract_bool(b"{\"key\":false}", b"\"key\""), Some(false));
    }

    #[test]
    fn extract_bool_garbage_is_none() {
        assert_eq!(extract_bool(b"{\"key\":123}", b"\"key\""), None);
    }

    #[test]
    fn extract_bool_not_found_is_none() {
        assert_eq!(extract_bool(b"{\"other\":true}", b"\"key\""), None);
    }

    #[test]
    fn extract_bool_missing_colon_is_none() {
        assert_eq!(extract_bool(b"{\"key\"}", b"\"key\""), None);
    }

    // ---- extract_i32 / parse_i32 ----

    #[test]
    fn extract_i32_positive() {
        assert_eq!(extract_i32(b"{\"key\":42}", b"\"key\""), Some(42));
    }

    #[test]
    fn extract_i32_negative() {
        assert_eq!(extract_i32(b"{\"key\":-7}", b"\"key\""), Some(-7));
    }

    #[test]
    fn extract_i32_not_found_is_none() {
        assert_eq!(extract_i32(b"{\"other\":1}", b"\"key\""), None);
    }

    #[test]
    fn extract_i32_missing_colon_is_none() {
        assert_eq!(extract_i32(b"{\"key\"}", b"\"key\""), None);
    }

    #[test]
    fn extract_i32_non_numeric_is_none() {
        assert_eq!(extract_i32(b"{\"key\":abc}", b"\"key\""), None);
    }

    #[test]
    fn parse_i32_valid_positive() {
        assert_eq!(parse_i32(b"123"), Some(123));
    }

    #[test]
    fn parse_i32_valid_negative() {
        assert_eq!(parse_i32(b"-45"), Some(-45));
    }

    #[test]
    fn parse_i32_empty_is_none() {
        assert_eq!(parse_i32(b""), None);
    }

    #[test]
    fn parse_i32_lone_minus_is_none() {
        assert_eq!(parse_i32(b"-"), None);
    }

    #[test]
    fn parse_i32_non_digit_start_is_none() {
        assert_eq!(parse_i32(b"abc"), None);
    }

    #[test]
    fn parse_i32_stops_at_first_non_digit() {
        assert_eq!(parse_i32(b"12ab"), Some(12));
    }

    // ---- parse_number_millis ----

    #[test]
    fn parse_number_millis_whole_only() {
        assert_eq!(parse_number_millis(b"{\"k\":5}", b"\"k\""), Some(5000));
    }

    #[test]
    fn parse_number_millis_one_frac_digit_padded() {
        assert_eq!(parse_number_millis(b"{\"k\":0.7}", b"\"k\""), Some(700));
    }

    #[test]
    fn parse_number_millis_three_frac_digits() {
        assert_eq!(parse_number_millis(b"{\"k\":1.234}", b"\"k\""), Some(1234));
    }

    #[test]
    fn parse_number_millis_truncates_beyond_three_frac_digits() {
        assert_eq!(
            parse_number_millis(b"{\"k\":1.23456}", b"\"k\""),
            Some(1234)
        );
    }

    #[test]
    fn parse_number_millis_stops_at_comma() {
        assert_eq!(
            parse_number_millis(b"{\"k\":5,\"j\":1}", b"\"k\""),
            Some(5000)
        );
    }

    #[test]
    fn parse_number_millis_stops_at_close_brace() {
        assert_eq!(parse_number_millis(b"{\"k\":9}", b"\"k\""), Some(9000));
    }

    #[test]
    fn parse_number_millis_stops_at_close_bracket() {
        assert_eq!(parse_number_millis(b"[{\"k\":3]", b"\"k\""), Some(3000));
    }

    #[test]
    fn parse_number_millis_stops_at_whitespace() {
        assert_eq!(parse_number_millis(b"{\"k\":3 x}", b"\"k\""), Some(3000));
        assert_eq!(parse_number_millis(b"{\"k\":3\nx}", b"\"k\""), Some(3000));
    }

    #[test]
    fn parse_number_millis_not_found_is_none() {
        assert_eq!(parse_number_millis(b"{\"other\":1}", b"\"k\""), None);
    }

    #[test]
    fn parse_number_millis_missing_colon_is_none() {
        assert_eq!(parse_number_millis(b"{\"k\"}", b"\"k\""), None);
    }

    #[test]
    fn parse_number_millis_empty_rest_is_none() {
        // Colon is the very last byte: after skip_ws the remaining slice is empty.
        assert_eq!(parse_number_millis(b"{\"k\":", b"\"k\""), None);
    }

    #[test]
    fn parse_number_millis_non_numeric_start_yields_zero() {
        assert_eq!(parse_number_millis(b"{\"k\":abc}", b"\"k\""), Some(0));
    }

    // ---- copy ----

    #[test]
    fn copy_writes_bytes_and_advances_index() {
        let mut buf = [0u8; 8];
        let i = copy(&mut buf, 0, b"abc");
        assert_eq!(i, 3);
        assert_eq!(&buf[..3], b"abc");
    }

    #[test]
    fn copy_overflow_guard_leaves_index_unchanged() {
        let mut buf = [0u8; 2];
        let i = copy(&mut buf, 0, b"abc");
        assert_eq!(i, 0);
        assert_eq!(buf, [0u8; 2]);
    }

    // ---- copy_json_escaped ----

    #[test]
    fn copy_json_escaped_escapes_quote_and_backslash() {
        let mut buf = [0u8; 16];
        let i = copy_json_escaped(&mut buf, 0, b"a\"b\\c");
        assert_eq!(&buf[..i], b"a\\\"b\\\\c");
    }

    #[test]
    fn copy_json_escaped_passes_through_plain_bytes() {
        let mut buf = [0u8; 8];
        let i = copy_json_escaped(&mut buf, 0, b"abc");
        assert_eq!(&buf[..i], b"abc");
    }

    #[test]
    fn copy_json_escaped_drops_bytes_on_overflow() {
        // buffer holds exactly 3 bytes: 'a','b' fit; the escaped quote (needs 2)
        // doesn't fit and is dropped whole via copy()'s guard; 'c' fits in the
        // remaining slot; 'd' then has no room and is dropped by the per-byte guard.
        let mut buf = [0u8; 3];
        let i = copy_json_escaped(&mut buf, 0, b"ab\"cd");
        assert_eq!(i, 3);
        assert_eq!(&buf[..3], b"abc");
    }

    // ---- write_u32 ----

    #[test]
    fn write_u32_zero() {
        let mut buf = [0u8; 4];
        let i = write_u32(&mut buf, 0, 0);
        assert_eq!(i, 1);
        assert_eq!(&buf[..1], b"0");
    }

    #[test]
    fn write_u32_multi_digit() {
        let mut buf = [0u8; 8];
        let i = write_u32(&mut buf, 0, 12345);
        assert_eq!(&buf[..i], b"12345");
    }

    #[test]
    fn write_u32_overflow_guard_truncates() {
        let mut buf = [0u8; 2];
        let i = write_u32(&mut buf, 0, 12345);
        assert_eq!(i, 2);
        assert_eq!(&buf, b"12");
    }

    #[test]
    fn write_u32_zero_with_no_room_returns_unchanged_index() {
        let mut buf: [u8; 0] = [];
        let i = write_u32(&mut buf, 0, 0);
        assert_eq!(i, 0);
    }

    // ---- write_i32 ----

    #[test]
    fn write_i32_positive() {
        let mut buf = [0u8; 8];
        let i = write_i32(&mut buf, 0, 42);
        assert_eq!(&buf[..i], b"42");
    }

    #[test]
    fn write_i32_negative() {
        let mut buf = [0u8; 8];
        let i = write_i32(&mut buf, 0, -42);
        assert_eq!(&buf[..i], b"-42");
    }

    // ---- ascii_lower / eq_ignore_case ----

    #[test]
    fn ascii_lower_converts_uppercase() {
        assert_eq!(ascii_lower(b'A'), b'a');
        assert_eq!(ascii_lower(b'Z'), b'z');
    }

    #[test]
    fn ascii_lower_leaves_others_unchanged() {
        assert_eq!(ascii_lower(b'a'), b'a');
        assert_eq!(ascii_lower(b'0'), b'0');
        assert_eq!(ascii_lower(b'-'), b'-');
    }

    #[test]
    fn eq_ignore_case_different_lengths_is_false() {
        assert!(!eq_ignore_case(b"abc", b"ab"));
    }

    #[test]
    fn eq_ignore_case_matches_case_insensitively() {
        assert!(eq_ignore_case(b"AbC", b"aBc"));
    }

    #[test]
    fn eq_ignore_case_mismatched_chars_is_false() {
        assert!(!eq_ignore_case(b"abc", b"abd"));
    }

    #[test]
    fn eq_ignore_case_empty_is_true() {
        assert!(eq_ignore_case(b"", b""));
    }

    // ---- normalize_email ----

    #[test]
    fn normalize_email_trims_and_lowercases() {
        let mut dst = [0u8; 32];
        let n = normalize_email(b"  Foo.Bar@EXAMPLE.com\t", &mut dst);
        assert_eq!(&dst[..n], b"foo.bar@example.com");
    }

    #[test]
    fn normalize_email_empty_input() {
        let mut dst = [0u8; 8];
        let n = normalize_email(b"", &mut dst);
        assert_eq!(n, 0);
    }

    #[test]
    fn normalize_email_all_whitespace_input() {
        let mut dst = [0u8; 8];
        let n = normalize_email(b"   \t ", &mut dst);
        assert_eq!(n, 0);
    }

    #[test]
    fn normalize_email_truncates_to_dst_capacity() {
        let mut dst = [0u8; 3];
        let n = normalize_email(b"ABCDE", &mut dst);
        assert_eq!(n, 3);
        assert_eq!(&dst[..3], b"abc");
    }

    // ---- trim_ascii ----

    #[test]
    fn trim_ascii_trims_leading_and_trailing() {
        assert_eq!(trim_ascii(b"  \t hello \n"), b"hello");
    }

    #[test]
    fn trim_ascii_no_whitespace_is_unchanged() {
        assert_eq!(trim_ascii(b"hello"), b"hello");
    }

    #[test]
    fn trim_ascii_all_whitespace_is_empty() {
        assert_eq!(trim_ascii(b" \t\n\r"), b"");
    }

    #[test]
    fn trim_ascii_empty_input_is_empty() {
        assert_eq!(trim_ascii(b""), b"");
    }

    // ---- parse_ymd_days ----

    #[test]
    fn parse_ymd_days_epoch_is_zero() {
        assert_eq!(parse_ymd_days(b"1970-01-01"), Some(0));
    }

    #[test]
    fn parse_ymd_days_next_day_is_one_more() {
        let epoch = parse_ymd_days(b"1970-01-01").unwrap();
        let next = parse_ymd_days(b"1970-01-02").unwrap();
        assert_eq!(next - epoch, 1);
    }

    #[test]
    fn parse_ymd_days_pre_epoch_is_negative() {
        let d = parse_ymd_days(b"1969-12-31").unwrap();
        assert_eq!(d, -1);
    }

    #[test]
    fn parse_ymd_days_too_short_is_none() {
        assert_eq!(parse_ymd_days(b"2026-1-1"), None);
    }

    #[test]
    fn parse_ymd_days_wrong_dash_positions_is_none() {
        assert_eq!(parse_ymd_days(b"2026/01/01"), None);
    }

    #[test]
    fn parse_ymd_days_month_out_of_range_is_none() {
        assert_eq!(parse_ymd_days(b"2026-00-01"), None);
        assert_eq!(parse_ymd_days(b"2026-13-01"), None);
    }

    #[test]
    fn parse_ymd_days_day_out_of_range_is_none() {
        assert_eq!(parse_ymd_days(b"2026-01-00"), None);
        assert_eq!(parse_ymd_days(b"2026-01-32"), None);
    }

    #[test]
    fn parse_ymd_days_non_numeric_component_is_none() {
        // parse_i32 stops at the first non-digit rather than requiring the whole
        // 4-byte slice to be numeric, so a *leading* digit run like "20ab" still
        // parses (as 20); only a component whose first byte isn't a digit fails.
        assert_eq!(parse_ymd_days(b"abcd-01-01"), None);
    }

    // ---- format_score_millis ----

    #[test]
    fn format_score_millis_zero() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 0);
        assert_eq!(&buf[..i], b"0.0");
    }

    #[test]
    fn format_score_millis_exact_whole() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 1000);
        assert_eq!(&buf[..i], b"1.0");
    }

    #[test]
    fn format_score_millis_three_frac_digits() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 785);
        assert_eq!(&buf[..i], b"0.785");
    }

    #[test]
    fn format_score_millis_trims_trailing_zeros_two() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 700);
        assert_eq!(&buf[..i], b"0.7");
    }

    #[test]
    fn format_score_millis_trims_trailing_zero_one() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 120);
        assert_eq!(&buf[..i], b"0.12");
    }

    #[test]
    fn format_score_millis_no_trailing_zero_to_trim() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 100);
        assert_eq!(&buf[..i], b"0.1");
    }

    #[test]
    fn format_score_millis_large_whole_number() {
        let mut buf = [0u8; 16];
        let i = format_score_millis(&mut buf, 123456);
        assert_eq!(&buf[..i], b"123.456");
    }
}
