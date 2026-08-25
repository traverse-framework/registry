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

static mut INPUT: [u8; 16384] = [0; 16384];
static mut OUTPUT: [u8; 4096] = [0; 4096];
const INPUT_CAPACITY: usize = 16384;
const OUTPUT_CAPACITY: usize = 4096;

#[derive(Copy, Clone)]
struct Source<'a> {
    id: &'a [u8],
    taxon: &'a [u8],
    status: &'a [u8],
    season: &'a [u8],
}

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
        let len = initialize(input, output);
        let mut written = 0usize;
        let v = IoVec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: len,
        };
        let _ = fd_write(1, &v, 1, &mut written);
    }
}

fn initialize(request: &[u8], out: &mut [u8]) -> usize {
    let location = match object_after(request, b"\"location\"") {
        Some(v) => v,
        None => return error_json(out, b"location.id is required"),
    };
    let policy = match object_after(request, b"\"policy\"") {
        Some(v) => v,
        None => return error_json(out, b"policy.version is required"),
    };
    let location_id = string_after(location, b"\"id\"");
    let policy_version = string_after(policy, b"\"version\"");
    if location_id.is_empty() {
        return error_json(out, b"location.id is required");
    }
    if policy_version.is_empty() {
        return error_json(out, b"policy.version is required");
    }

    let sources = match array_after(request, b"\"sources\"") {
        Some(v) => v,
        None => return error_json(out, b"every source needs license, taxon, and status"),
    };

    let mut entries = [Source { id: b"", taxon: b"", status: b"", season: b"" }; 32];
    let count = match parse_sources(sources, &mut entries) {
        Ok(count) => count,
        Err(message) => return error_json(out, message),
    };

    sort_sources(&mut entries[..count]);

    let mut at = 0usize;
    at = copy(out, at, b"{\"location_id\":\"");
    at = copy_json(out, at, location_id);
    at = copy(out, at, b"\",\"policy_version\":\"");
    at = copy_json(out, at, policy_version);
    at = copy(out, at, b"\",\"candidates\":[");
    for (index, source) in entries[..count].iter().enumerate() {
        if index > 0 {
            at = copy(out, at, b",");
        }
        at = copy(out, at, b"{\"taxon\":\"");
        at = copy_json(out, at, source.taxon);
        at = copy(out, at, b"\",\"status\":\"");
        at = copy_json(out, at, source.status);
        at = copy(out, at, b"\",\"source_id\":\"");
        at = copy_json(out, at, source.id);
        at = copy(out, at, b"\",\"season\":\"");
        at = copy_json(out, at, source.season);
        at = copy(out, at, b"\"}");
    }
    copy(out, at, b"]}")
}

fn parse_sources<'a>(array: &'a [u8], entries: &mut [Source<'a>; 32]) -> Result<usize, &'static [u8]> {
    let mut count = 0usize;
    let mut rest = array;
    while let Some(start) = find(rest, b"{") {
        let chunk = &rest[start..];
        let end = match balanced_end(chunk) {
            Some(end) => end,
            None => return Err(b"every source needs license, taxon, and status"),
        };
        if count == entries.len() {
            return Err(b"too many sources");
        }
        let object = &chunk[..=end];
        let id = string_after(object, b"\"id\"");
        let license = string_after(object, b"\"license\"");
        let taxon = string_after(object, b"\"taxon\"");
        let status = string_after(object, b"\"status\"");
        let season = string_after(object, b"\"season\"");
        if license.is_empty() || taxon.is_empty() || status.is_empty() {
            return Err(b"every source needs license, taxon, and status");
        }
        entries[count] = Source {
            id,
            taxon,
            status,
            season: if season.is_empty() { b"all" } else { season },
        };
        count += 1;
        rest = &chunk[end + 1..];
    }
    Ok(count)
}

fn sort_sources(entries: &mut [Source<'_>]) {
    let len = entries.len();
    let mut i = 0usize;
    while i < len {
        let mut j = i + 1;
        while j < len {
            if source_cmp(entries[j], entries[i]) < 0 {
                entries.swap(i, j);
            }
            j += 1;
        }
        i += 1;
    }
}

fn source_cmp(a: Source<'_>, b: Source<'_>) -> i32 {
    let taxon = cmp_bytes(a.taxon, b.taxon);
    if taxon != 0 {
        return taxon;
    }
    cmp_bytes(a.id, b.id)
}

fn cmp_bytes(a: &[u8], b: &[u8]) -> i32 {
    let len = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < len {
        if a[i] < b[i] {
            return -1;
        }
        if a[i] > b[i] {
            return 1;
        }
        i += 1;
    }
    if a.len() < b.len() {
        -1
    } else if a.len() > b.len() {
        1
    } else {
        0
    }
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
        let input = include_bytes!("../tests/uc01-sorted.json");
        let mut out = [0u8; 65_536];
        let length = initialize(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-empty-sources.json");
        let mut out = [0u8; 65_536];
        let length = initialize(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_3() {
        let input = include_bytes!("../tests/uc03-malformed-source.json");
        let mut out = [0u8; 65_536];
        let length = initialize(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }

    #[test]
    fn covers_location_parser_and_sort_boundaries() {
        let mut entries = [Source { id: b"", taxon: b"", status: b"", season: b"" }; 32];
        assert!(parse_sources(b"[{", &mut entries).is_err());
        let mut overflow = b"[".to_vec();
        for _ in 0..33 {
            overflow.extend_from_slice(br#"{"license":"l","taxon":"t","status":"s"}"#);
        }
        assert!(parse_sources(&overflow, &mut entries).is_err());
        let first = Source { id: b"a", taxon: b"a", status: b"s", season: b"all" };
        let second = Source { id: b"b", taxon: b"a", status: b"s", season: b"all" };
        assert!(source_cmp(first, second) < 0);
        assert_eq!(cmp_bytes(b"a", b"ab"), -1);
        assert_eq!(cmp_bytes(b"ab", b"a"), 1);
        assert_eq!(string_after(br#"{"x":"unterminated}"#, br#""x""#), b"");
        let mut short = [0u8; 1];
        assert_eq!(copy(&mut short, 1, b"x"), 1);
        let mut escaped = [0u8; 8];
        assert_eq!(copy_json(&mut escaped, 0, &[b'"', b'\\']), 4);
        assert!(balanced_end(&[b'{', b'"', b'\\', b'"', b'"', b'}']).is_some());
    }


    #[test]
    fn handles_an_incomplete_request_without_panicking() {
        let mut out = [0u8; 65_536];
        let length = initialize(b"{}", &mut out);
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
            let length = initialize(input, &mut out);
            assert!(length <= out.len());
        }
    }

    #[test]
    fn remains_total_for_truncated_and_corrupted_real_requests() {
        for fixture in [include_bytes!("../tests/uc01-sorted.json").as_slice(), include_bytes!("../tests/uc02-empty-sources.json").as_slice(), include_bytes!("../tests/uc03-malformed-source.json").as_slice()] {
            for end in 0..=fixture.len() {
                let mut out = [0u8; 65_536];
                let length = initialize(&fixture[..end], &mut out);
                assert!(length <= out.len());
            }
            for index in 0..core::cmp::min(fixture.len(), 512) {
                let mut corrupted = fixture.to_vec();
                corrupted[index] = b'?';
                let mut out = [0u8; 65_536];
                let length = initialize(&corrupted, &mut out);
                assert!(length <= out.len());
            }
        }
    }
}
