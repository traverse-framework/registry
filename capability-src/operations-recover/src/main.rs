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

static mut INPUT: [u8; 8192] = [0; 8192];
static mut OUTPUT: [u8; 2048] = [0; 2048];
const INPUT_CAPACITY: usize = 8192;
const OUTPUT_CAPACITY: usize = 2048;

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        loop {
            let mut n = 0usize;
            let v = IoVecMut {
                buffer: core::ptr::addr_of_mut!(INPUT).cast::<u8>().add(total),
                length: INPUT_CAPACITY - total,
            };
            if fd_read(0, &v, 1, &mut n) != 0 || n == 0 {
                break;
            }
            total += n;
            if total == INPUT_CAPACITY {
                break;
            }
        }

        let input = core::slice::from_raw_parts(core::ptr::addr_of!(INPUT).cast::<u8>(), total);
        let output = core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(OUTPUT).cast::<u8>(), OUTPUT_CAPACITY);
        let len = recover(input, output);
        let mut written = 0usize;
        let v = IoVec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: len,
        };
        let _ = fd_write(1, &v, 1, &mut written);
    }
}

fn recover(request: &[u8], out: &mut [u8]) -> usize {
    let policy = match object_after(request, b"\"policy\"") {
        Some(v) => v,
        None => return error_json(out, b"policy.version is required"),
    };
    let policy_version = string_after(policy, b"\"version\"");
    if policy_version.is_empty() {
        return error_json(out, b"policy.version is required");
    }

    let mut expected = [[0u8; 64]; 32];
    let mut completed = [[0u8; 64]; 32];
    let expected_count = parse_string_array(array_after(request, b"\"expectedIds\""), &mut expected);
    let completed_count = parse_string_array(array_after(request, b"\"completedIds\""), &mut completed);
    dedupe_and_sort(&mut expected[..expected_count]);
    dedupe_and_sort(&mut completed[..completed_count]);

    let mut replay = [[0u8; 64]; 32];
    let mut replay_count = 0usize;
    let mut i = 0usize;
    while i < expected_count {
        if !contains_cstr(&completed[..completed_count], &expected[i]) && !contains_cstr(&replay[..replay_count], &expected[i]) {
            replay[replay_count] = expected[i];
            replay_count += 1;
        }
        i += 1;
    }
    sort_fixed_strings(&mut replay[..replay_count]);

    let action = if replay_count > 0 {
        b"replay_missing_idempotently" as &[u8]
    } else {
        b"no_action" as &[u8]
    };

    let mut at = 0usize;
    at = copy(out, at, b"{\"policy_version\":\"");
    at = copy_json(out, at, policy_version);
    at = copy(out, at, b"\",\"replay_ids\":[");
    at = write_fixed_string_array(out, at, &replay[..replay_count]);
    at = copy(out, at, b"],\"action\":\"");
    at = copy_json(out, at, action);
    copy(out, at, b"\"}")
}

fn parse_string_array(array: Option<&[u8]>, entries: &mut [[u8; 64]; 32]) -> usize {
    let Some(array) = array else { return 0 };
    let mut rest = array;
    let mut count = 0usize;
    while let Some(start) = find(rest, b"\"") {
        if count == entries.len() {
            break;
        }
        let after = &rest[start + 1..];
        let Some(end) = after.iter().position(|b| *b == b'"') else { break };
        let value = &after[..end];
        let len = if value.len() < 63 { value.len() } else { 63 };
        entries[count][..len].copy_from_slice(&value[..len]);
        entries[count][len] = 0;
        count += 1;
        rest = &after[end + 1..];
    }
    count
}

fn dedupe_and_sort(entries: &mut [[u8; 64]]) {
    sort_fixed_strings(entries);
    let len = entries.len();
    let mut i = 0usize;
    while i + 1 < len {
        if cmp_cstr(&entries[i], &entries[i + 1]) == 0 {
            entries[i + 1] = [0u8; 64];
        }
        i += 1;
    }
    sort_fixed_strings(entries);
}

fn contains_cstr(haystack: &[[u8; 64]], needle: &[u8; 64]) -> bool {
    let mut i = 0usize;
    while i < haystack.len() {
        if cmp_cstr(&haystack[i], needle) == 0 {
            return true;
        }
        i += 1;
    }
    false
}

fn sort_fixed_strings(entries: &mut [[u8; 64]]) {
    let len = entries.len();
    let mut i = 0usize;
    while i < len {
        let mut j = i + 1;
        while j < len {
            if cmp_cstr(&entries[j], &entries[i]) < 0 {
                entries.swap(i, j);
            }
            j += 1;
        }
        i += 1;
    }
}

fn cmp_cstr(a: &[u8; 64], b: &[u8; 64]) -> i32 {
    let mut i = 0usize;
    while i < 64 {
        let av = a[i];
        let bv = b[i];
        if av == 0 && bv == 0 {
            return 0;
        }
        if av == 0 {
            return if bv == 0 { 0 } else { -1 };
        }
        if bv == 0 {
            return 1;
        }
        if av < bv {
            return -1;
        }
        if av > bv {
            return 1;
        }
        i += 1;
    }
    0
}

fn write_fixed_string_array(out: &mut [u8], mut at: usize, entries: &[[u8; 64]]) -> usize {
    let mut first = true;
    let mut i = 0usize;
    while i < entries.len() {
        if entries[i][0] != 0 {
            if !first {
                at = copy(out, at, b",");
            }
            first = false;
            at = copy(out, at, b"\"");
            let mut j = 0usize;
            while j < 64 && entries[i][j] != 0 {
                at = copy(out, at, &[entries[i][j]]);
                j += 1;
            }
            at = copy(out, at, b"\"");
        }
        i += 1;
    }
    at
}

fn error_json(out: &mut [u8], message: &[u8]) -> usize {
    let mut at = 0usize;
    at = copy(out, at, b"{\"error\":\"");
    at = copy_json(out, at, message);
    copy(out, at, b"\"}")
}

fn skip(mut s: &[u8]) -> &[u8] {
    while s.first().is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\r' | b'\t')) {
        s = &s[1..];
    }
    s
}

fn find(s: &[u8], key: &[u8]) -> Option<usize> {
    s.windows(key.len()).position(|w| w == key)
}

fn balanced_end(s: &[u8]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (i, &b) in s.iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
            continue;
        }
        match b {
            b'"' => quoted = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn object_after<'a>(s: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'{') {
        return None;
    }
    Some(&rest[..=balanced_end(rest)?])
}

fn array_after<'a>(s: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'[') {
        return None;
    }
    Some(&rest[..=balanced_end(rest)?])
}

fn string_after<'a>(s: &'a [u8], key: &[u8]) -> &'a [u8] {
    let Some(p) = find(s, key) else { return b"" };
    let Some(c) = s[p + key.len()..].iter().position(|b| *b == b':') else { return b"" };
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.first() != Some(&b'"') {
        return b"";
    }
    let rest = &rest[1..];
    match rest.iter().position(|b| *b == b'"') {
        Some(end) => &rest[..end],
        None => b"",
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

fn copy_json(out: &mut [u8], mut at: usize, s: &[u8]) -> usize {
    for &b in s {
        at = match b {
            b'"' => copy(out, at, b"\\\""),
            b'\\' => copy(out, at, b"\\\\"),
            _ => copy(out, at, &[b]),
        };
    }
    at
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exercises_runtime_fixture_1() {
        let input = include_bytes!("../tests/uc01-missing-work.json");
        let mut out = [0u8; 65_536];
        let length = recover(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-noop.json");
        let mut out = [0u8; 65_536];
        let length = recover(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }


    #[test]
    fn handles_an_incomplete_request_without_panicking() {
        let mut out = [0u8; 65_536];
        let length = recover(b"{}", &mut out);
        assert!(length <= out.len());
    }

    #[test]
    fn rejects_malformed_json_shapes_without_panicking() {
        for input in [
            b"".as_slice(), b"{".as_slice(), b"[]".as_slice(), b"null".as_slice(),
            br#"{"x":null}"#.as_slice(), br#"{"x":[]}"#.as_slice(),
            br#"{"x":{}}"#.as_slice(), br#"{"items":[]}"#.as_slice(),
            br#"{"policy":{}}"#.as_slice(), br#"{"location":{}}"#.as_slice(),
        ] {
            let mut out = [0u8; 65_536];
            let length = recover(input, &mut out);
            assert!(length <= out.len());
        }
    }

    #[test]
    fn remains_total_for_truncated_and_corrupted_real_requests() {
        for fixture in [include_bytes!("../tests/uc01-missing-work.json").as_slice(), include_bytes!("../tests/uc02-noop.json").as_slice()] {
            for end in 0..=fixture.len() {
                let mut out = [0u8; 65_536];
                let length = recover(&fixture[..end], &mut out);
                assert!(length <= out.len());
            }
            for index in 0..core::cmp::min(fixture.len(), 512) {
                let mut corrupted = fixture.to_vec();
                corrupted[index] = b'?';
                let mut out = [0u8; 65_536];
                let length = recover(&corrupted, &mut out);
                assert!(length <= out.len());
            }
        }
    }
}
