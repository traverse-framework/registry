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
    let health = object_after_key_at_depth(input, b"\"health\"", 1);
    let config = object_after_key_at_depth(input, b"\"escalation_config\"", 1);
    if health.is_none() || config.is_none() {
        return fail(out, b"invalid_input");
    }
    let health = health.unwrap_or(b"{}");
    let config = config.unwrap_or(b"{}");

    let overdue = extract_i32(health, b"\"overdue_count\"").unwrap_or(-1);
    let on_track = parse_number_millis(health, b"\"on_track_pct\"");
    if overdue < 0 {
        return fail(out, b"invalid_input");
    }
    let overloaded = array_after_key_at_depth(health, b"\"overloaded_owners\"", 1).unwrap_or(b"[]");
    let overloaded_count = count_objects(overloaded);
    let top = array_after_key_at_depth(health, b"\"top_pressure_items\"", 1).unwrap_or(b"[]");

    let min_overdue = extract_i32(config, b"\"min_overdue_for_escalate\"").unwrap_or(2);
    let min_overloaded = extract_i32(config, b"\"min_overloaded_owners\"").unwrap_or(2);
    let max_on_track = parse_number_millis(config, b"\"max_on_track_pct_for_escalate\"").unwrap_or(500);
    let require_multi = extract_bool(config, b"\"require_multiple_signals\"").unwrap_or(true);

    let mut signals: [&[u8]; 3] = [b"", b"", b""];
    let mut signals_met = 0u32;
    if overdue >= min_overdue {
        signals[signals_met as usize] = b"overdue_count";
        signals_met += 1;
    }
    if overloaded_count as i32 >= min_overloaded {
        signals[signals_met as usize] = b"overloaded_owners";
        signals_met += 1;
    }
    if let Some(pct) = on_track {
        if pct <= max_on_track {
            signals[signals_met as usize] = b"on_track_pct";
            signals_met += 1;
        }
    }

    let escalate = if require_multi {
        signals_met >= 2
    } else {
        signals_met >= 1
    };
    let decision: &[u8] = if escalate {
        b"escalate" as &[u8]
    } else {
        b"digest" as &[u8]
    };

    let mut i = 0usize;
    i = copy(out, i, b"{\"decision\":\"");
    i = copy(out, i, decision);
    i = copy(out, i, b"\",\"signals_met\":");
    i = write_u32(out, i, signals_met);
    i = copy(out, i, b",\"signals\":[");
    for s in 0..signals_met as usize {
        if s > 0 {
            i = copy(out, i, b",");
        }
        i = copy(out, i, b"\"");
        i = copy(out, i, signals[s]);
        i = copy(out, i, b"\"");
    }
    i = copy(out, i, b"],\"escalate_item_ids\":");
    if escalate {
        // copy top pressure items array as-is (already JSON array of strings)
        i = copy(out, i, top);
    } else {
        i = copy(out, i, b"[]");
    }
    i = copy(out, i, b",\"reason_code\":\"ok\",\"evaluation_trace\":[\"signals_met=");
    i = write_u32(out, i, signals_met);
    i = copy(out, i, b"\",\"require_multiple=");
    i = copy(out, i, if require_multi { b"true" } else { b"false" });
    i = copy(out, i, b"\",\"decision=");
    i = copy(out, i, decision);
    i = copy(out, i, b"\"]}");
    i
}

fn count_objects(arr: &[u8]) -> u32 {
    let mut count = 0u32;
    let mut depth = 0i32;
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
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => {
                if depth == 1 {
                    count += 1;
                }
                depth += 1;
            }
            b'}' => depth -= 1,
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    count
}

fn fail(out: &mut [u8], code: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"decision\":\"digest\",\"signals_met\":0,\"signals\":[],\"escalate_item_ids\":[],\"reason_code\":\"");
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
        let out = run("{\"health\":{\"total_open\":3,\"overdue_count\":1,\"on_track_pct\":66.6,\"overloaded_owners\":[{\"owner_id\":\"user-ada\",\"open_count\":2}],\"top_pressure_items\":[\"ai-3\",\"ai-1\"]},\"escalation_config\":{\"version\":\"1.0\",\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"max_on_track_pct_for_escalate\":50.0,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_02_happy() {
        let out = run("{\"health\":{\"total_open\":5,\"overdue_count\":3,\"on_track_pct\":40.0,\"overloaded_owners\":[{\"owner_id\":\"user-ada\",\"open_count\":3},{\"owner_id\":\"user-bob\",\"open_count\":2}],\"top_pressure_items\":[\"ai-9\",\"ai-7\"]},\"escalation_config\":{\"version\":\"1.0\",\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":2,\"max_on_track_pct_for_escalate\":50.0,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_03_sad() {
        let out = run("{\"health\":{\"total_open\":3,\"overdue_count\":-1,\"on_track_pct\":66.6,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"version\":\"1.0\",\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "expected invalid_input in {out}");
    }

    #[test]
    fn use_case_04_sad_missing_health_key() {
        let out = run("{\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "expected invalid_input in {out}");
    }

    #[test]
    fn use_case_05_sad_missing_escalation_config_key() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]}}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "expected invalid_input in {out}");
    }

    #[test]
    fn use_case_06_sad_escalation_config_not_an_object() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":\"oops\"}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "expected invalid_input in {out}");
    }

    #[test]
    fn use_case_07_happy_missing_on_track_pct_key() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_08_happy_single_signal_escalates_without_require_multiple() {
        let out = run("{\"health\":{\"overdue_count\":5,\"overloaded_owners\":[],\"top_pressure_items\":[\"ai-1\"]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":false}}");
        assert!(out.contains("\"decision\":\"escalate\""), "expected escalate in {out}");
        assert!(out.contains("\"require_multiple=false\""), "expected require_multiple=false in {out}");
    }

    #[test]
    fn use_case_09_happy_malformed_require_multiple_signals_falls_back_to_default() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":123}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_10_happy_tolerates_extra_whitespace_and_backslash_before_target_keys() {
        let out = run("{\"health\":{\"overdue_count\":1,\"note\":\"a\\\\b\",\"overloaded_owners\":[{\"owner_id\":\"user\\\\x\",\"open_count\":1}],\"top_pressure_items\":[]},\"escalation_config\":  {\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_11_sad_unbalanced_escalation_config_object() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "expected invalid_input in {out}");
    }

    #[test]
    fn use_case_12_happy_overloaded_owners_not_an_array_falls_back_to_empty() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":\"oops\",\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_13_happy_non_numeric_min_overdue_falls_back_to_default() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":\"bad\",\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_14_happy_on_track_pct_stops_at_trailing_garbage() {
        let out = run("{\"health\":{\"overdue_count\":1,\"on_track_pct\":66x,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn parse_i32_rejects_empty_input() {
        assert_eq!(parse_i32(b""), None);
    }

    #[test]
    fn parse_number_millis_rejects_a_key_with_nothing_after_its_colon() {
        assert_eq!(parse_number_millis(b"\"x\":", b"\"x\""), None);
    }

    #[test]
    fn use_case_15_happy_nested_object_inside_array_item() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[{\"owner_id\":\"x\",\"meta\":{\"note\":\"y\"}}],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_16_sad_escalation_config_key_without_colon() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\"}");
        assert!(out.contains("\"reason_code\":\"invalid_input\""), "expected invalid_input in {out}");
    }

    #[test]
    fn use_case_17_happy_missing_overloaded_owners_key() {
        let out = run("{\"health\":{\"overdue_count\":1,\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_18_happy_overloaded_owners_key_without_colon() {
        // overloaded_owners must be the last key in the whole document with
        // no colon anywhere after it -- the scanner searches the rest of
        // the input for the next colon, so an earlier key would otherwise
        // pick up a later, unrelated key's colon instead of finding none.
        let out = run("{\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true},\"health\":{\"overdue_count\":1,\"top_pressure_items\":[],\"overloaded_owners\"}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn array_after_key_at_depth_rejects_an_unclosed_array() {
        assert_eq!(
            array_after_key_at_depth(b"{\"a\":[1,2}", b"\"a\"", 1),
            None
        );
    }

    #[test]
    fn use_case_20_happy_missing_require_multiple_signals_key() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_21_happy_require_multiple_signals_key_without_colon() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\"}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_22_happy_missing_min_overdue_key() {
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_23_happy_min_overdue_key_without_colon() {
        // min_overdue_for_escalate must be the last key in the document
        // with no colon anywhere after it, for the same reason as
        // use_case_18 above.
        let out = run("{\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\"}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_24_happy_on_track_pct_key_without_colon() {
        // on_track_pct must be the last key in the document with no colon
        // anywhere after it, for the same reason as use_case_18 above.
        let out = run("{\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true},\"health\":{\"overdue_count\":1,\"overloaded_owners\":[],\"top_pressure_items\":[],\"on_track_pct\"}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn use_case_25_happy_on_track_pct_with_more_than_three_fractional_digits() {
        let out = run("{\"health\":{\"overdue_count\":1,\"on_track_pct\":66.123456,\"overloaded_owners\":[],\"top_pressure_items\":[]},\"escalation_config\":{\"min_overdue_for_escalate\":2,\"min_overloaded_owners\":1,\"require_multiple_signals\":true}}");
        assert!(out.contains("\"reason_code\":\"ok\""), "expected ok in {out}");
    }

    #[test]
    fn write_u32_drops_digits_that_do_not_fit_a_nonzero_value() {
        let mut out: [u8; 0] = [];
        assert_eq!(write_u32(&mut out, 0, 5), 0);
    }

    #[test]
    fn copy_truncates_instead_of_overflowing_the_output_buffer() {
        let mut out = [0u8; 2];
        let end = copy(&mut out, 1, b"abc");
        assert_eq!(end, 1, "copy must return the unchanged offset when it would overflow");
    }

    #[test]
    fn write_u32_writes_nothing_when_zero_does_not_fit_the_buffer() {
        let mut out: [u8; 0] = [];
        assert_eq!(write_u32(&mut out, 0, 0), 0);
    }

}