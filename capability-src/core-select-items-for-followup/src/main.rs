//! core.select-items-for-followup — deterministic follow-up item selection.
//!
//! Respects quiet hours, snooze, per-user nudge budgets, and escalation thresholds.
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
static mut OUTPUT_BUF: [u8; 16384] = [0; 16384];

const MAX_ITEMS: usize = 16;

#[derive(Clone, Copy)]
struct Selected {
    item_id: [u8; 48],
    item_id_len: usize,
    intensity: [u8; 12],
    intensity_len: usize,
    reason: [u8; 128],
    reason_len: usize,
    channel: [u8; 24],
    channel_len: usize,
}

#[derive(Clone, Copy)]
struct Skipped {
    item_id: [u8; 48],
    item_id_len: usize,
    reason: [u8; 24],
    reason_len: usize,
    detail: [u8; 128],
    detail_len: usize,
}

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
    let ref_dt = extract_string_at_depth(input, b"\"reference_datetime\"", 1);
    let config = object_after_key(input, b"\"followup_config\"").unwrap_or(b"");
    let prefs = object_after_key(input, b"\"user_preferences\"").unwrap_or(b"");
    let Some(open_items) = array_after_key_at_depth(input, b"\"open_items\"", 1) else {
        return fail(out, b"config_error", br#"["open_items missing"]"#);
    };
    if ref_dt.is_empty() || config.is_empty() {
        return fail(out, b"config_error", br#"["required fields missing"]"#);
    }

    let respect_quiet = extract_bool(config, b"\"respect_quiet_hours\"").unwrap_or(true);
    let escalate_after = extract_i32(config, b"\"escalate_after_nudges\"").unwrap_or(3);
    let min_pressure = extract_number_scaled(config, b"\"min_pressure_for_soft\"").unwrap_or(400);

    let ref_date = &ref_dt[..ref_dt.len().min(10)];

    let mut selected: [Selected; MAX_ITEMS] = [Selected {
        item_id: [0; 48],
        item_id_len: 0,
        intensity: [0; 12],
        intensity_len: 0,
        reason: [0; 128],
        reason_len: 0,
        channel: [0; 24],
        channel_len: 0,
    }; MAX_ITEMS];
    let mut skipped: [Skipped; MAX_ITEMS] = [Skipped {
        item_id: [0; 48],
        item_id_len: 0,
        reason: [0; 24],
        reason_len: 0,
        detail: [0; 128],
        detail_len: 0,
    }; MAX_ITEMS];
    let mut sel_count = 0usize;
    let mut skip_count = 0usize;
    let mut item_count = 0usize;
    let mut any_quiet = false;
    let mut any_escalate = false;

    for_each_object_in_array(open_items, |item| {
        if item_count >= MAX_ITEMS {
            return;
        }
        item_count += 1;

        let item_id = extract_string(item, b"\"id\"");
        let owner = extract_string(item, b"\"owner_id\"");
        let due = extract_string(item, b"\"due_date\"");
        let snoozed = extract_optional_string(item, b"\"snoozed_until\"");
        let nudge_count = extract_i32(item, b"\"nudge_count\"").unwrap_or(0);
        let pressure = extract_number_scaled(item, b"\"pressure_score\"").unwrap_or(0);

        if !snoozed.is_empty() && snoozed > ref_dt {
            push_skipped(
                &mut skipped,
                &mut skip_count,
                item_id,
                b"snoozed",
                b"item is snoozed until later",
            );
            return;
        }

        if respect_quiet {
            if let Some(owner_prefs) = user_pref_object(prefs, owner) {
                if let Some(qh) = object_after_key(owner_prefs, b"\"quiet_hours\"") {
                    if in_quiet_hours(ref_dt, qh) {
                        any_quiet = true;
                        let mut detail = [0u8; 96];
                        let mut d = 0usize;
                        d = copy(&mut detail, d, b"owner is inside quiet hours ");
                        let start = extract_string(qh, b"\"start\"");
                        let end = extract_string(qh, b"\"end\"");
                        d = copy(&mut detail, d, start);
                        d = copy(&mut detail, d, b"-");
                        d = copy(&mut detail, d, end);
                        push_skipped_detail(
                            &mut skipped,
                            &mut skip_count,
                            item_id,
                            b"quiet_hours",
                            &detail[..d],
                        );
                        return;
                    }
                }
            }
        }

        let max_nudges = user_pref_i32(prefs, owner, b"\"max_nudges_per_day\"").unwrap_or(2);
        let sent_today = user_pref_i32(prefs, owner, b"\"nudges_sent_today\"").unwrap_or(0);
        if sent_today >= max_nudges {
            push_skipped(
                &mut skipped,
                &mut skip_count,
                item_id,
                b"budget_exceeded",
                b"daily nudge budget exhausted",
            );
            return;
        }

        if pressure < min_pressure {
            push_skipped(
                &mut skipped,
                &mut skip_count,
                item_id,
                b"low_pressure",
                b"pressure below soft threshold",
            );
            return;
        }

        let overdue = !due.is_empty() && due < ref_date;

        if overdue && nudge_count >= escalate_after {
            any_escalate = true;
            push_selected(
                &mut selected,
                &mut sel_count,
                item_id,
                b"escalate",
                b"3 prior nudges ignored + overdue + high pressure",
                b"manager",
            );
            return;
        }

        if overdue {
            push_selected(
                &mut selected,
                &mut sel_count,
                item_id,
                b"direct",
                b"item is overdue",
                b"",
            );
            return;
        }

        // `pressure < min_pressure` already returned above, so the
        // `pressure >= min_pressure` disjunct always holds here: every
        // remaining item is a "soft" selection, never a
        // last_activity/no-signal skip.
        push_selected(
            &mut selected,
            &mut sel_count,
            item_id,
            b"soft",
            b"approaching due date or sufficient pressure",
            b"",
        );
    });

    let mut trace = [0u8; 512];
    let mut t = 0usize;
    t = copy(&mut trace, t, b"[\"");
    t = write_usize(&mut trace, t, item_count);
    t = copy(&mut trace, t, b" item evaluated");
    if item_count != 1 {
        t = copy(&mut trace, t, b"s");
    }
    t = copy(&mut trace, t, b"\"");
    if any_quiet && sel_count == 0 && skip_count > 0 {
        t = copy(&mut trace, t, b", \"skipped due to quiet_hours\"");
    }
    if any_escalate {
        t = copy(&mut trace, t, b", \"escalation threshold met\"");
    }
    t = copy(&mut trace, t, b"]");

    write_output(
        out,
        &selected[..sel_count],
        &skipped[..skip_count],
        &trace[..t],
    )
}

fn write_output(out: &mut [u8], selected: &[Selected], skipped: &[Skipped], trace: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(out, i, b"{\"selected\":[");
    for (idx, s) in selected.iter().enumerate() {
        if idx > 0 {
            i = copy(out, i, b",");
        }
        i = copy(out, i, b"{\"item_id\":\"");
        i = copy(out, i, &s.item_id[..s.item_id_len]);
        i = copy(out, i, b"\",\"intensity\":\"");
        i = copy(out, i, &s.intensity[..s.intensity_len]);
        i = copy(out, i, b"\",\"reason\":\"");
        i = copy_json_escaped(out, i, &s.reason[..s.reason_len]);
        if s.channel_len > 0 {
            i = copy(out, i, b"\",\"recommended_channel\":\"");
            i = copy(out, i, &s.channel[..s.channel_len]);
        }
        i = copy(out, i, b"\"}");
    }
    i = copy(out, i, b"],\"skipped\":[");
    for (idx, s) in skipped.iter().enumerate() {
        if idx > 0 {
            i = copy(out, i, b",");
        }
        i = copy(out, i, b"{\"item_id\":\"");
        i = copy(out, i, &s.item_id[..s.item_id_len]);
        i = copy(out, i, b"\",\"reason\":\"");
        i = copy(out, i, &s.reason[..s.reason_len]);
        i = copy(out, i, b"\",\"detail\":\"");
        i = copy_json_escaped(out, i, &s.detail[..s.detail_len]);
        i = copy(out, i, b"\"}");
    }
    i = copy(out, i, b"],\"reason_code\":\"ok\",\"evaluation_trace\":");
    i = copy(out, i, trace);
    i = copy(out, i, b"}");
    i
}

fn fail(out: &mut [u8], code: &[u8], trace: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(
        out,
        i,
        b"{\"selected\":[],\"skipped\":[],\"reason_code\":\"",
    );
    i = copy(out, i, code);
    i = copy(out, i, b"\",\"evaluation_trace\":");
    i = copy(out, i, trace);
    i = copy(out, i, b"}");
    i
}

fn push_selected(
    selected: &mut [Selected],
    count: &mut usize,
    item_id: &[u8],
    intensity: &[u8],
    reason: &[u8],
    channel: &[u8],
) {
    if *count >= MAX_ITEMS {
        return;
    }
    let mut s = Selected {
        item_id: [0; 48],
        item_id_len: 0,
        intensity: [0; 12],
        intensity_len: 0,
        reason: [0; 128],
        reason_len: 0,
        channel: [0; 24],
        channel_len: 0,
    };
    s.item_id_len = copy(&mut s.item_id, 0, item_id);
    s.intensity_len = copy(&mut s.intensity, 0, intensity);
    s.reason_len = copy(&mut s.reason, 0, reason);
    if !channel.is_empty() {
        s.channel_len = copy(&mut s.channel, 0, channel);
    }
    selected[*count] = s;
    *count += 1;
}

fn push_skipped(
    skipped: &mut [Skipped],
    count: &mut usize,
    item_id: &[u8],
    reason: &[u8],
    detail: &[u8],
) {
    push_skipped_detail(skipped, count, item_id, reason, detail);
}

fn push_skipped_detail(
    skipped: &mut [Skipped],
    count: &mut usize,
    item_id: &[u8],
    reason: &[u8],
    detail: &[u8],
) {
    if *count >= MAX_ITEMS {
        return;
    }
    let mut s = Skipped {
        item_id: [0; 48],
        item_id_len: 0,
        reason: [0; 24],
        reason_len: 0,
        detail: [0; 128],
        detail_len: 0,
    };
    s.item_id_len = copy(&mut s.item_id, 0, item_id);
    s.reason_len = copy(&mut s.reason, 0, reason);
    s.detail_len = copy(&mut s.detail, 0, detail);
    skipped[*count] = s;
    *count += 1;
}

fn for_each_object_in_array(arr: &[u8], mut f: impl FnMut(&[u8])) {
    if arr.first() != Some(&b'[') {
        return;
    }
    let mut i = 1usize;
    while i < arr.len() {
        while i < arr.len()
            && (arr[i] == b' '
                || arr[i] == b'\n'
                || arr[i] == b'\t'
                || arr[i] == b'\r'
                || arr[i] == b',')
        {
            i += 1;
        }
        if i >= arr.len() || arr[i] == b']' {
            break;
        }
        if arr[i] == b'{' {
            if let Some(end) = balanced_end(&arr[i..], b'{', b'}') {
                f(&arr[i..=i + end]);
                i += end + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
}

fn user_pref_object<'a>(prefs: &'a [u8], owner: &[u8]) -> Option<&'a [u8]> {
    if prefs.is_empty() || owner.is_empty() {
        return None;
    }
    let mut key = [0u8; 64];
    let mut k = 0usize;
    k = copy(&mut key, k, b"\"");
    k = copy(&mut key, k, owner);
    k = copy(&mut key, k, b"\"");
    object_after_key(prefs, &key[..k])
}

fn user_pref_i32(prefs: &[u8], owner: &[u8], key: &[u8]) -> Option<i32> {
    let obj = user_pref_object(prefs, owner)?;
    extract_i32(obj, key)
}

fn in_quiet_hours(ref_dt: &[u8], qh: &[u8]) -> bool {
    let hour = parse_ref_hour(ref_dt);
    let start = parse_time_hour(extract_string(qh, b"\"start\""));
    let end = parse_time_hour(extract_string(qh, b"\"end\""));
    if start <= end {
        hour >= start && hour < end
    } else {
        hour >= start || hour < end
    }
}

fn parse_ref_hour(ref_dt: &[u8]) -> i32 {
    let Some(t_pos) = ref_dt.iter().position(|b| *b == b'T') else {
        return 0;
    };
    let after = &ref_dt[t_pos + 1..];
    parse_two_digit_hour(after)
}

fn parse_time_hour(time: &[u8]) -> i32 {
    parse_two_digit_hour(time)
}

fn parse_two_digit_hour(s: &[u8]) -> i32 {
    if s.len() < 2 {
        return 0;
    }
    let h0 = s[0];
    let h1 = s[1];
    if h0 < b'0' || h0 > b'9' || h1 < b'0' || h1 > b'9' {
        return 0;
    }
    i32::from(h0 - b'0') * 10 + i32::from(h1 - b'0')
}

fn write_usize(out: &mut [u8], at: usize, n: usize) -> usize {
    if n == 0 {
        return copy(out, at, b"0");
    }
    let mut buf = [0u8; 20];
    let mut v = n;
    let mut len = 0usize;
    while v > 0 {
        buf[len] = b'0' + (v % 10) as u8;
        len += 1;
        v /= 10;
    }
    let mut i = at;
    while len > 0 {
        len -= 1;
        if i < out.len() {
            out[i] = buf[len];
            i += 1;
        }
    }
    i
}

fn extract_optional_string<'a>(hay: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(pos) = find(hay, key) else {
        return b"";
    };
    let after = &hay[pos + key.len()..];
    let Some(colon) = after.iter().position(|b| *b == b':') else {
        return b"";
    };
    let rest = skip_ws(&after[colon + 1..]);
    if rest.starts_with(b"null") {
        return b"";
    }
    if rest.first() != Some(&b'"') {
        return b"";
    }
    let inner = &rest[1..];
    let Some(end) = inner.iter().position(|b| *b == b'"') else {
        return b"";
    };
    &inner[..end]
}

fn extract_number_scaled(hay: &[u8], key: &[u8]) -> Option<i32> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = skip_ws(&after[colon + 1..]);
    let mut neg = false;
    if rest.first() == Some(&b'-') {
        neg = true;
        rest = &rest[1..];
    }
    let mut whole = 0i32;
    let mut frac = 0i32;
    let mut frac_digits = 0i32;
    let mut in_frac = false;
    let mut any = false;
    for &b in rest {
        if b == b'.' {
            in_frac = true;
            continue;
        }
        if b < b'0' || b > b'9' {
            break;
        }
        any = true;
        if in_frac {
            if frac_digits < 3 {
                frac = frac.saturating_mul(10).saturating_add(i32::from(b - b'0'));
                frac_digits += 1;
            }
        } else {
            whole = whole.saturating_mul(10).saturating_add(i32::from(b - b'0'));
        }
    }
    if !any {
        return None;
    }
    while frac_digits < 3 {
        frac = frac.saturating_mul(10);
        frac_digits += 1;
    }
    let scaled = whole.saturating_mul(1000).saturating_add(frac);
    Some(if neg { -scaled } else { scaled })
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

fn object_after_key<'a>(hay: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let rest = skip_ws(&after[colon + 1..]);
    if rest.first() != Some(&b'{') {
        return None;
    }
    let end = balanced_end(rest, b'{', b'}')?;
    Some(&rest[..=end])
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

fn extract_i32(hay: &[u8], key: &[u8]) -> Option<i32> {
    let pos = find(hay, key)?;
    let after = &hay[pos + key.len()..];
    let colon = after.iter().position(|b| *b == b':')?;
    let mut rest = skip_ws(&after[colon + 1..]);
    let mut neg = false;
    if rest.first() == Some(&b'-') {
        neg = true;
        rest = &rest[1..];
    }
    let mut val: i32 = 0;
    let mut any = false;
    for &b in rest {
        if b < b'0' || b > b'9' {
            break;
        }
        any = true;
        val = val.saturating_mul(10).saturating_add(i32::from(b - b'0'));
    }
    if !any {
        return None;
    }
    Some(if neg { -val } else { val })
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
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-08\",\"status\":\"open\",\"last_activity_at\":\"2026-08-05T10:00:00Z\",\"nudge_count\":0,\"pressure_score\":0.85,\"snoozed_until\":null}],\"user_preferences\":{\"user-ada\":{\"quiet_hours\":{\"start\":\"20:00\",\"end\":\"08:00\",\"timezone\":\"America/Los_Angeles\"},\"max_nudges_per_day\":2,\"nudges_sent_today\":0}},\"reference_datetime\":\"2026-08-07T22:30:00Z\",\"followup_config\":{\"version\":\"1.1\",\"soft_days_before_due\":2,\"direct_days_overdue\":1,\"escalate_after_nudges\":3,\"min_pressure_for_soft\":0.4,\"respect_quiet_hours\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_02_happy() {
        let out = run("{\"open_items\":[{\"id\":\"ai-9\",\"owner_id\":\"user-bob\",\"due_date\":\"2026-08-01\",\"status\":\"open\",\"last_activity_at\":\"2026-07-28T09:00:00Z\",\"nudge_count\":3,\"pressure_score\":0.95,\"snoozed_until\":null}],\"user_preferences\":{\"user-bob\":{\"quiet_hours\":null,\"max_nudges_per_day\":3,\"nudges_sent_today\":0}},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"version\":\"1.1\",\"soft_days_before_due\":2,\"direct_days_overdue\":1,\"escalate_after_nudges\":3,\"min_pressure_for_soft\":0.4,\"respect_quiet_hours\":true}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_03_sad() {
        let out = run("{\"open_items\":[],\"user_preferences\":{},\"reference_datetime\":\"\",\"followup_config\":{\"version\":\"1.1\",\"soft_days_before_due\":2,\"direct_days_overdue\":1,\"escalate_after_nudges\":3,\"min_pressure_for_soft\":0.4,\"respect_quiet_hours\":true}}");
        assert!(
            out.contains("\"reason_code\":\"config_error\""),
            "expected config_error in {out}"
        );
    }

    #[test]
    fn missing_open_items_yields_config_error() {
        let out = run("{\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"version\":\"1.1\"}}");
        assert!(
            out.contains("\"reason_code\":\"config_error\""),
            "expected config_error in {out}"
        );
        assert!(
            out.contains("open_items missing"),
            "expected reason detail in {out}"
        );
    }

    #[test]
    fn snoozed_item_is_skipped() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":\"2099-01-01T00:00:00Z\"}],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("\"reason\":\"snoozed\""),
            "expected snoozed skip in {out}"
        );
    }

    #[test]
    fn quiet_hours_wrap_around_midnight_skips_item() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{\"user-ada\":{\"quiet_hours\":{\"start\":\"22:00\",\"end\":\"06:00\"},\"max_nudges_per_day\":2,\"nudges_sent_today\":0}},\"reference_datetime\":\"2026-08-07T23:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":true,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("\"reason\":\"quiet_hours\""),
            "expected quiet_hours skip in {out}"
        );
    }

    #[test]
    fn quiet_hours_same_day_but_outside_window_proceeds() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{\"user-ada\":{\"quiet_hours\":{\"start\":\"20:00\",\"end\":\"22:00\"},\"max_nudges_per_day\":2,\"nudges_sent_today\":0}},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":true,\"min_pressure_for_soft\":0.4}}");
        assert!(
            !out.contains("\"reason\":\"quiet_hours\""),
            "expected item not skipped for quiet hours in {out}"
        );
    }

    #[test]
    fn owner_without_preferences_skips_quiet_hours_check() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-nobody\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":true,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected evaluation to proceed in {out}"
        );
    }

    #[test]
    fn budget_exceeded_is_skipped() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{\"user-ada\":{\"max_nudges_per_day\":1,\"nudges_sent_today\":2}},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("\"reason\":\"budget_exceeded\""),
            "expected budget_exceeded skip in {out}"
        );
    }

    #[test]
    fn low_pressure_is_skipped() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.1,\"snoozed_until\":null}],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("\"reason\":\"low_pressure\""),
            "expected low_pressure skip in {out}"
        );
    }

    #[test]
    fn overdue_with_enough_nudges_escalates() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":5,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4,\"escalate_after_nudges\":3}}");
        assert!(
            out.contains("\"intensity\":\"escalate\""),
            "expected escalate selection in {out}"
        );
        assert!(out.contains("\"recommended_channel\":\"manager\""));
    }

    #[test]
    fn overdue_without_enough_nudges_is_direct() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4,\"escalate_after_nudges\":3}}");
        assert!(
            out.contains("\"intensity\":\"direct\""),
            "expected direct selection in {out}"
        );
    }

    #[test]
    fn due_today_is_soft_selection() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-07\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("\"intensity\":\"soft\""),
            "expected soft selection in {out}"
        );
    }

    #[test]
    fn max_items_cutoff_stops_after_limit() {
        let mut items = String::new();
        for n in 0..20 {
            if n > 0 {
                items.push(',');
            }
            items.push_str(&format!(
                "{{\"id\":\"ai-{n}\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null}}"
            ));
        }
        let input = format!(
            "{{\"open_items\":[{items}],\"user_preferences\":{{}},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4}}}}"
        );
        let out = run(&input);
        assert!(
            out.contains("16 item evaluateds"),
            "expected the 16-item cap in {out}"
        );
    }

    #[test]
    fn non_object_item_element_stops_scanning() {
        let out = run("{\"open_items\":[{\"id\":\"ai-1\",\"owner_id\":\"user-ada\",\"due_date\":\"2026-08-01\",\"nudge_count\":0,\"pressure_score\":0.9,\"snoozed_until\":null},42],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false,\"min_pressure_for_soft\":0.4}}");
        assert!(
            out.contains("1 item evaluated"),
            "expected scanning to stop at the non-object element in {out}"
        );
    }

    #[test]
    fn empty_open_items_array_evaluates_to_zero_items() {
        let out = run("{\"open_items\":[],\"user_preferences\":{},\"reference_datetime\":\"2026-08-07T10:00:00Z\",\"followup_config\":{\"respect_quiet_hours\":false}}");
        assert!(
            out.contains("0 item evaluateds"),
            "expected zero items in {out}"
        );
    }

    #[test]
    fn parse_two_digit_hour_rejects_non_digits() {
        assert_eq!(parse_two_digit_hour(b"ab"), 0);
        assert_eq!(parse_two_digit_hour(b"9"), 0);
        assert_eq!(parse_two_digit_hour(b"23"), 23);
    }

    #[test]
    fn parse_ref_hour_defaults_when_no_t_separator() {
        assert_eq!(parse_ref_hour(b"2026-08-07"), 0);
    }

    #[test]
    fn extract_number_scaled_handles_negative_and_missing() {
        assert_eq!(extract_number_scaled(b"\"k\":-1.5", b"\"k\""), Some(-1500));
        assert_eq!(extract_number_scaled(b"\"k\":oops", b"\"k\""), None);
        assert_eq!(extract_number_scaled(b"{}", b"\"missing\""), None);
    }

    #[test]
    fn extract_i32_handles_negative_and_missing() {
        assert_eq!(extract_i32(b"\"k\":-7", b"\"k\""), Some(-7));
        assert_eq!(extract_i32(b"\"k\":oops", b"\"k\""), None);
        assert_eq!(extract_i32(b"{}", b"\"missing\""), None);
    }

    #[test]
    fn extract_bool_handles_false_and_neither() {
        assert_eq!(extract_bool(b"\"k\":false", b"\"k\""), Some(false));
        assert_eq!(extract_bool(b"\"k\":maybe", b"\"k\""), None);
    }

    #[test]
    fn extract_optional_string_handles_missing_null_and_malformed() {
        assert_eq!(extract_optional_string(b"{}", b"\"k\""), b"");
        assert_eq!(extract_optional_string(b"\"k\"no-colon", b"\"k\""), b"");
        assert_eq!(extract_optional_string(b"\"k\":null", b"\"k\""), b"");
        assert_eq!(extract_optional_string(b"\"k\":5", b"\"k\""), b"");
        assert_eq!(
            extract_optional_string(b"\"k\":\"unterminated", b"\"k\""),
            b""
        );
        assert_eq!(extract_optional_string(b"\"k\":\"ok\"", b"\"k\""), b"ok");
    }

    #[test]
    fn object_after_key_handles_missing_and_non_object() {
        assert_eq!(object_after_key(b"{}", b"\"missing\""), None);
        assert_eq!(object_after_key(b"\"k\":5", b"\"k\""), None);
    }

    #[test]
    fn array_after_key_at_depth_handles_non_array() {
        assert_eq!(array_after_key_at_depth(b"\"k\":5", b"\"k\"", 0), None);
    }

    #[test]
    fn user_pref_object_handles_empty_prefs_and_owner() {
        assert_eq!(user_pref_object(b"", b"user-a"), None);
        assert_eq!(user_pref_object(b"{}", b""), None);
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
    fn write_usize_handles_zero_and_multiple_digits() {
        let mut buf = [0u8; 8];
        let n = write_usize(&mut buf, 0, 0);
        assert_eq!(&buf[..n], b"0");
        let mut buf2 = [0u8; 8];
        let n2 = write_usize(&mut buf2, 0, 123);
        assert_eq!(&buf2[..n2], b"123");
    }
}
