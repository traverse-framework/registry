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

const MAX_MEMBERS: usize = 32;
const MAX_PARTS: usize = 32;
const STR_MAX: usize = 96;

struct Member {
    id: [u8; STR_MAX],
    id_len: usize,
    name: [u8; STR_MAX],
    name_len: usize,
    email: [u8; STR_MAX],
    email_len: usize,
}

impl Copy for Member {}
impl Clone for Member {
    fn clone(&self) -> Self {
        *self
    }
}

pub unsafe fn evaluate(input: &[u8], out: &mut [u8]) -> usize {
    let raw = array_after_key_at_depth(input, b"\"raw_participants\"", 1);
    let members_arr = array_after_key_at_depth(input, b"\"workspace_members\"", 1);
    let config = object_after_key_at_depth(input, b"\"normalize_config\"", 1);
    if raw.is_none() || members_arr.is_none() || config.is_none() {
        return fail(out, b"invalid_input");
    }
    let raw = raw.unwrap_or(b"[]");
    let members_arr = members_arr.unwrap_or(b"[]");
    let _config = config.unwrap_or(b"{}");

    let mut members = [Member {
        id: [0; STR_MAX],
        id_len: 0,
        name: [0; STR_MAX],
        name_len: 0,
        email: [0; STR_MAX],
        email_len: 0,
    }; MAX_MEMBERS];
    let mut member_count = 0usize;
    let mut pos = 1usize;
    while pos < members_arr.len() && member_count < MAX_MEMBERS {
        pos += skip_ws_comma(&members_arr[pos..]);
        if pos >= members_arr.len() || members_arr[pos] == b']' {
            break;
        }
        if members_arr[pos] != b'{' {
            break;
        }
        let Some(end) = balanced_end(&members_arr[pos..], b'{', b'}') else {
            break;
        };
        let obj = &members_arr[pos..=pos + end];
        pos += end + 1;
        let id = extract_string(obj, b"\"id\"");
        let name = extract_string(obj, b"\"name\"");
        let email = extract_string(obj, b"\"email\"");
        if id.is_empty() || id.len() > STR_MAX {
            continue;
        }
        let m = &mut members[member_count];
        m.id[..id.len()].copy_from_slice(id);
        m.id_len = id.len();
        let n = trim_ascii(name);
        if n.len() <= STR_MAX {
            m.name[..n.len()].copy_from_slice(n);
            m.name_len = n.len();
        }
        let elen = normalize_email(email, &mut m.email);
        m.email_len = elen;
        member_count += 1;
    }

    let mut i = 0usize;
    i = copy(out, i, b"{\"participants\":[");
    let mut matched = 0u32;
    let mut unmatched = 0u32;
    let mut total = 0u32;
    let mut first = true;
    let mut unmatched_seq = 0u32;
    pos = 1usize;
    while pos < raw.len() && total < MAX_PARTS as u32 {
        pos += skip_ws_comma(&raw[pos..]);
        if pos >= raw.len() || raw[pos] == b']' {
            break;
        }
        if raw[pos] != b'{' {
            break;
        }
        let Some(end) = balanced_end(&raw[pos..], b'{', b'}') else {
            break;
        };
        let obj = &raw[pos..=pos + end];
        pos += end + 1;
        total += 1;

        let name = trim_ascii(extract_string(obj, b"\"name\""));
        let email_raw = extract_string(obj, b"\"email\"");
        let mut email_norm = [0u8; STR_MAX];
        let email_len = if email_raw.is_empty() {
            0
        } else {
            normalize_email(email_raw, &mut email_norm)
        };

        let mut match_id: &[u8] = b"";
        let mut match_name: &[u8] = b"";
        let mut match_email: &[u8] = b"";
        let mut method: &[u8] = b"none";

        if email_len > 0 {
            for m in members.iter().take(member_count) {
                if m.email_len == email_len && m.email[..m.email_len] == email_norm[..email_len] {
                    match_id = &m.id[..m.id_len];
                    match_name = &m.name[..m.name_len];
                    match_email = &m.email[..m.email_len];
                    method = b"email";
                    break;
                }
            }
        }
        if method == b"none" && !name.is_empty() {
            for m in members.iter().take(member_count) {
                if m.name_len > 0 && eq_ignore_case(&m.name[..m.name_len], name) {
                    match_id = &m.id[..m.id_len];
                    match_name = &m.name[..m.name_len];
                    match_email = &m.email[..m.email_len];
                    method = b"name";
                    break;
                }
            }
        }

        if !first {
            i = copy(out, i, b",");
        }
        first = false;
        i = copy(out, i, b"{\"participant_id\":\"");
        if method == b"none" {
            unmatched += 1;
            unmatched_seq += 1;
            i = copy(out, i, b"unmatched-");
            i = write_u32(out, i, unmatched_seq);
            i = copy(out, i, b"\",\"display_name\":\"");
            i = copy_json_escaped(out, i, name);
            i = copy(out, i, b"\",\"email\":");
            if email_len == 0 {
                i = copy(out, i, b"null");
            } else {
                i = copy(out, i, b"\"");
                i = copy_json_escaped(out, i, &email_norm[..email_len]);
                i = copy(out, i, b"\"");
            }
            i = copy(out, i, b",\"match_method\":\"none\"}");
        } else {
            matched += 1;
            i = copy(out, i, match_id);
            i = copy(out, i, b"\",\"display_name\":\"");
            i = copy_json_escaped(
                out,
                i,
                if match_name.is_empty() {
                    name
                } else {
                    match_name
                },
            );
            i = copy(out, i, b"\",\"email\":");
            if match_email.is_empty() {
                i = copy(out, i, b"null");
            } else {
                i = copy(out, i, b"\"");
                i = copy_json_escaped(out, i, match_email);
                i = copy(out, i, b"\"");
            }
            i = copy(out, i, b",\"match_method\":\"");
            i = copy(out, i, method);
            i = copy(out, i, b"\"}");
        }
    }

    i = copy(out, i, b"],\"matched_count\":");
    i = write_u32(out, i, matched);
    i = copy(out, i, b",\"unmatched_count\":");
    i = write_u32(out, i, unmatched);
    i = copy(
        out,
        i,
        b",\"reason_code\":\"ok\",\"evaluation_trace\":[\"normalized ",
    );
    i = write_u32(out, i, total);
    i = copy(out, i, b" participants\",\"matched ");
    i = write_u32(out, i, matched);
    i = copy(out, i, b"\"]}");
    i
}

fn fail(out: &mut [u8], code: &[u8]) -> usize {
    let mut i = 0usize;
    i = copy(
        out,
        i,
        b"{\"participants\":[],\"matched_count\":0,\"unmatched_count\":0,\"reason_code\":\"",
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
        let out = run("{\"raw_participants\":[{\"name\":\"Ada Lovelace\",\"email\":\"Ada@Loop.Dev\"},{\"name\":\"bob smith\",\"email\":\"bob@loop.dev\"},{\"name\":\"Unknown Person\",\"email\":\"stranger@example.com\"}],\"workspace_members\":[{\"id\":\"user-ada\",\"name\":\"Ada Lovelace\",\"email\":\"ada@loop.dev\"},{\"id\":\"user-bob\",\"name\":\"Bob Smith\",\"email\":\"bob@loop.dev\"}],\"normalize_config\":{\"version\":\"1.0\"}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn use_case_02_happy() {
        let out = run("{\"raw_participants\":[{\"name\":\"Ada Lovelace\",\"email\":null}],\"workspace_members\":[{\"id\":\"user-ada\",\"name\":\"Ada Lovelace\",\"email\":\"ada@loop.dev\"}],\"normalize_config\":{\"version\":\"1.0\"}}");
        assert!(
            out.contains("\"reason_code\":\"ok\""),
            "expected ok in {out}"
        );
    }

    #[test]
    fn missing_required_fields_yields_invalid_input() {
        let out = run("{\"workspace_members\":[],\"normalize_config\":{}}");
        assert!(
            out.contains("\"reason_code\":\"invalid_input\""),
            "expected invalid_input in {out}"
        );
    }

    #[test]
    fn member_with_missing_name_or_email_falls_back_on_match() {
        let out = run("{\"raw_participants\": [ {\"name\":\"Dave X\",\"email\":\"dave@x.com\"},{\"name\":\"Carol\",\"email\":null} ],\"workspace_members\": [ {\"id\":\"user-dave\",\"name\":\"\",\"email\":\"dave@x.com\"},{\"id\":\"user-carol\",\"name\":\"Carol\",\"email\":\"\"} ],\"normalize_config\": {\"version\":\"1.0\"}}");
        assert!(
            out.contains("\"display_name\":\"Dave X\""),
            "expected name fallback to raw participant's own name in {out}"
        );
        assert!(
            out.contains("\"email\":null"),
            "expected null email for member matched with no email on file in {out}"
        );
    }

    #[test]
    fn member_with_empty_or_too_long_id_is_skipped() {
        let long_id = "x".repeat(200);
        let input = format!(
            "{{\"raw_participants\":[{{\"name\":\"Solo\",\"email\":\"solo@x.com\"}}],\"workspace_members\":[{{\"id\":\"\",\"name\":\"NoId\",\"email\":\"noid@x.com\"}},{{\"id\":\"{long_id}\",\"name\":\"TooLong\",\"email\":\"solo@x.com\"}}],\"normalize_config\":{{\"version\":\"1.0\"}}}}"
        );
        let out = run(&input);
        assert!(
            out.contains("\"match_method\":\"none\""),
            "expected no match since both members were skipped in {out}"
        );
    }

    #[test]
    fn non_object_member_element_stops_scanning() {
        let out = run("{\"raw_participants\":[{\"name\":\"A\",\"email\":\"a@x.com\"}],\"workspace_members\":[{\"id\":\"user-a\",\"name\":\"A\",\"email\":\"a@x.com\"},42],\"normalize_config\":{\"version\":\"1.0\"}}");
        assert!(
            out.contains("\"match_method\":\"email\""),
            "expected the first member to still be usable in {out}"
        );
    }

    #[test]
    fn non_object_participant_element_stops_scanning() {
        let out = run("{\"raw_participants\":[{\"name\":\"A\",\"email\":\"a@x.com\"},42],\"workspace_members\":[{\"id\":\"user-a\",\"name\":\"A\",\"email\":\"a@x.com\"}],\"normalize_config\":{\"version\":\"1.0\"}}");
        assert!(
            out.contains("\"matched_count\":1"),
            "expected scanning to stop at the non-object element in {out}"
        );
    }

    #[test]
    fn unterminated_member_object_stops_scanning() {
        let out = run("{\"raw_participants\":[{\"name\":\"A\",\"email\":\"a@x.com\"}],\"normalize_config\":{\"version\":\"1.0\"},\"workspace_members\":[{\"id\":\"user-a\",\"name\":\"A\",\"email\":\"a@x.com\"},{\"id\":\"user-b\"]}");
        assert!(
            out.contains("\"match_method\":\"email\""),
            "expected the first member to still be usable in {out}"
        );
    }

    #[test]
    fn unterminated_participant_object_stops_scanning() {
        let out = run("{\"workspace_members\":[{\"id\":\"user-a\",\"name\":\"A\",\"email\":\"a@x.com\"}],\"normalize_config\":{\"version\":\"1.0\"},\"raw_participants\":[{\"name\":\"A\",\"email\":\"a@x.com\"},{\"name\":\"B\"]}");
        assert!(
            out.contains("\"matched_count\":1"),
            "expected scanning to stop at the unterminated element in {out}"
        );
    }

    #[test]
    fn member_clone_is_a_bitwise_copy() {
        let m = Member {
            id: [1u8; STR_MAX],
            id_len: 1,
            name: [2u8; STR_MAX],
            name_len: 1,
            email: [3u8; STR_MAX],
            email_len: 1,
        };
        let cloned = m.clone();
        assert_eq!(cloned.id_len, m.id_len);
        assert_eq!(cloned.email[0], 3u8);
    }

    #[test]
    fn string_value_after_handles_missing_colon_quote_and_terminator() {
        assert_eq!(string_value_after(b"no colon here"), b"");
        assert_eq!(string_value_after(b":not-a-quote"), b"");
        assert_eq!(string_value_after(b":\"unterminated"), b"");
        assert_eq!(string_value_after(b":\"ok\""), b"ok");
    }

    #[test]
    fn extract_string_returns_empty_when_key_missing() {
        assert_eq!(extract_string(b"{\"other\":\"x\"}", b"\"missing\""), b"");
    }

    #[test]
    fn object_after_key_at_depth_returns_none_for_non_object_value() {
        assert_eq!(
            object_after_key_at_depth(b"\"item\":5", b"\"item\"", 0),
            None
        );
    }

    #[test]
    fn array_after_key_at_depth_returns_none_for_non_array_value() {
        assert_eq!(
            array_after_key_at_depth(b"\"item\":5", b"\"item\"", 0),
            None
        );
    }

    #[test]
    fn skip_ws_trims_leading_whitespace() {
        assert_eq!(skip_ws(b"   abc"), b"abc");
        assert_eq!(skip_ws(b"abc"), b"abc");
    }

    #[test]
    fn balanced_end_returns_none_when_unterminated() {
        assert_eq!(balanced_end(b"{\"a\":\"b\"", b'{', b'}'), None);
    }
}
