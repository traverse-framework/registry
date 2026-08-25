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
static mut OUTPUT: [u8; 512] = [0; 512];
const INPUT_CAPACITY: usize = 8192;
const OUTPUT_CAPACITY: usize = 512;

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
        let len = resolve(input, output);
        let mut written = 0usize;
        let v = IoVec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: len,
        };
        let _ = fd_write(1, &v, 1, &mut written);
    }
}

fn resolve(request: &[u8], out: &mut [u8]) -> usize {
    let candidate = match object_after(request, b"\"candidate\"") {
        Some(v) => v,
        None => return 0,
    };
    let policy = match object_after(request, b"\"policy\"") {
        Some(v) => v,
        None => return 0,
    };
    let evaluation = object_after(request, b"\"evaluation\"");
    let candidate_id = string_after(candidate, b"\"id\"");
    let candidate_sha = string_after(candidate, b"\"sha256\"");
    let status = string_after(candidate, b"\"status\"");
    let active_model = string_after(request, b"\"activeModelId\"");
    let minimum_precision = int_after(policy, b"\"minimum_precision_millis\"").unwrap_or(1001);
    let minimum_recall = int_after(policy, b"\"minimum_recall_millis\"").unwrap_or(1001);
    let precision = evaluation.and_then(|v| int_after(v, b"\"held_out_precision_millis\""));
    let recall = evaluation.and_then(|v| int_after(v, b"\"held_out_recall_millis\""));

    let mut at = 0usize;
    if status != b"verified" {
        at = copy(out, at, b"{\"decision\":\"reject\",\"reason\":\"artifact_not_verified\",\"active_model_id\":");
        at = write_string_or_null(out, at, active_model);
        return copy(out, at, b"}");
    }
    if precision.unwrap_or(-1) < minimum_precision || recall.unwrap_or(-1) < minimum_recall {
        at = copy(out, at, b"{\"decision\":\"reject\",\"reason\":\"evaluation_below_threshold\",\"active_model_id\":");
        at = write_string_or_null(out, at, active_model);
        return copy(out, at, b"}");
    }

    at = copy(out, at, b"{\"decision\":\"approve\",\"reason\":\"release_gate_passed\",\"active_model_id\":\"");
    at = copy_json(out, at, candidate_id);
    at = copy(out, at, b"\",\"previous_model_id\":");
    at = write_string_or_null(out, at, active_model);
    at = copy(out, at, b",\"release_id\":\"release-");
    at = copy_json(out, at, candidate_id);
    at = copy(out, at, b"-");
    at = copy_json(out, at, candidate_sha);
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
            b'{' => depth += 1,
            b'}' => {
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

fn int_after(s: &[u8], key: &[u8]) -> Option<i32> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    let mut n = 0i32;
    let mut count = 0;
    for &b in rest {
        if !(b'0'..=b'9').contains(&b) {
            break;
        }
        n = n.checked_mul(10)?.checked_add((b - b'0') as i32)?;
        count += 1;
    }
    if count == 0 { None } else { Some(n) }
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

fn write_string_or_null(out: &mut [u8], at: usize, s: &[u8]) -> usize {
    if s.is_empty() {
        copy(out, at, b"null")
    } else {
        let at = copy(out, at, b"\"");
        let at = copy_json(out, at, s);
        copy(out, at, b"\"")
    }
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
        let input = include_bytes!("../tests/uc01-approve.json");
        let mut out = [0u8; 65_536];
        let length = resolve(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-reject-unverified.json");
        let mut out = [0u8; 65_536];
        let length = resolve(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_3() {
        let input = include_bytes!("../tests/uc03-reject-threshold.json");
        let mut out = [0u8; 65_536];
        let length = resolve(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }


    #[test]
    fn handles_an_incomplete_request_without_panicking() {
        let mut out = [0u8; 65_536];
        let length = resolve(b"{}", &mut out);
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
            let length = resolve(input, &mut out);
            assert!(length <= out.len());
        }
    }

    #[test]
    fn remains_total_for_truncated_and_corrupted_real_requests() {
        for fixture in [include_bytes!("../tests/uc01-approve.json").as_slice(), include_bytes!("../tests/uc02-reject-unverified.json").as_slice(), include_bytes!("../tests/uc03-reject-threshold.json").as_slice()] {
            for end in 0..=fixture.len() {
                let mut out = [0u8; 65_536];
                let length = resolve(&fixture[..end], &mut out);
                assert!(length <= out.len());
            }
            for index in 0..core::cmp::min(fixture.len(), 512) {
                let mut corrupted = fixture.to_vec();
                corrupted[index] = b'?';
                let mut out = [0u8; 65_536];
                let length = resolve(&corrupted, &mut out);
                assert!(length <= out.len());
            }
        }
    }
}
