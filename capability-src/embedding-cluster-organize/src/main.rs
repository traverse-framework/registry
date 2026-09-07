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
static mut OUTPUT: [u8; 2048] = [0; 2048];
const INPUT_CAPACITY: usize = 16384;
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
        let len = resolve(input, output);
        let mut written = 0usize;
        let v = IoVec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: len,
        };
        let _ = fd_write(1, &v, 1, &mut written);
    }
}

#[derive(Copy, Clone)]
struct Item<'a> {
    id: &'a [u8],
    embedding: [f32; 8],
    len: usize,
}

fn resolve(request: &[u8], out: &mut [u8]) -> usize {
    let policy = match object_after(request, b"\"policy\"") {
        Some(v) => v,
        None => return 0,
    };
    let policy_version = string_after(policy, b"\"version\"");
    let minimum_cosine = number_after(policy, b"\"minimum_cosine\"").unwrap_or(1.0);
    let items = parse_items(request);
    if items.count == 0 {
        return copy(out, 0, b"[]");
    }
    let base_len = items.items[0].len;
    let mut i = 0usize;
    while i < items.count {
        if items.items[i].id.is_empty() || items.items[i].len != base_len || base_len == 0 {
            return copy(out, 0, b"{\"error\":\"items require equal finite embeddings\"}");
        }
        i += 1;
    }

    let mut parent = [0usize; 16];
    i = 0;
    while i < items.count {
        parent[i] = i;
        i += 1;
    }
    let mut a = 0usize;
    while a < items.count {
        let mut b = a + 1;
        while b < items.count {
            if cosine(&items.items[a], &items.items[b]) >= minimum_cosine {
                join(&mut parent, a, b);
            }
            b += 1;
        }
        a += 1;
    }

    let mut groups = Groups::new();
    i = 0;
    while i < items.count {
        let root = find_root(&mut parent, i);
        groups.push(root, items.items[i].id);
        i += 1;
    }
    groups.sort();

    let mut at = 0usize;
    at = copy(out, at, b"[");
    i = 0;
    while i < groups.count {
        if i > 0 {
            at = copy(out, at, b",");
        }
        at = copy(out, at, b"{\"member_ids\":[");
        let group = &groups.groups[i];
        let mut j = 0usize;
        while j < group.count {
            if j > 0 {
                at = copy(out, at, b",");
            }
            at = copy(out, at, b"\"");
            at = copy_json(out, at, group.members[j]);
            at = copy(out, at, b"\"");
            j += 1;
        }
        at = copy(out, at, b"],\"representative_id\":\"");
        at = copy_json(out, at, group.members[0]);
        at = copy(out, at, b"\",\"policy_version\":\"");
        at = copy_json(out, at, policy_version);
        at = copy(out, at, b"\"}");
        i += 1;
    }
    copy(out, at, b"]")
}

struct ParsedItems<'a> {
    items: [Item<'a>; 16],
    count: usize,
}

impl<'a> ParsedItems<'a> {
    const fn new() -> Self {
        Self {
            items: [Item { id: b"", embedding: [0.0; 8], len: 0 }; 16],
            count: 0,
        }
    }
}

fn parse_items<'a>(request: &'a [u8]) -> ParsedItems<'a> {
    let mut parsed = ParsedItems::new();
    let Some(array) = array_after(request, b"\"items\"") else { return parsed };
    let mut pos = 0usize;
    while pos < array.len() && parsed.count < 16 {
      if array[pos] == b'{' {
        let slice = &array[pos..];
        let Some(end) = balanced_end(slice) else { break };
        let obj = &slice[..=end];
        let id = string_after(obj, b"\"id\"");
        let embedding = parse_embedding(obj, b"\"embedding\"");
        parsed.items[parsed.count] = Item { id, embedding: embedding.values, len: embedding.len };
        parsed.count += 1;
        pos += end;
      }
      pos += 1;
    }
    parsed
}

struct Embedding {
    values: [f32; 8],
    len: usize,
}

fn parse_embedding(s: &[u8], key: &[u8]) -> Embedding {
    let mut result = Embedding { values: [0.0; 8], len: 0 };
    let Some(array) = array_after(s, key) else { return result };
    let mut i = 0usize;
    while i < array.len() && result.len < 8 {
        if array[i].is_ascii_digit() || array[i] == b'-' {
            let (value, consumed) = parse_number_literal(&array[i..]);
            result.values[result.len] = value;
            result.len += 1;
            i += consumed;
            continue;
        }
        i += 1;
    }
    result
}

fn parse_number_literal(s: &[u8]) -> (f32, usize) {
    let mut i = 0usize;
    let mut sign = 1.0f32;
    if s.first() == Some(&b'-') {
        sign = -1.0;
        i += 1;
    }
    let mut whole = 0f32;
    while i < s.len() && s[i].is_ascii_digit() {
        whole = whole * 10.0 + (s[i] - b'0') as f32;
        i += 1;
    }
    let mut frac = 0f32;
    let mut scale = 1f32;
    if i < s.len() && s[i] == b'.' {
        i += 1;
        while i < s.len() && s[i].is_ascii_digit() {
            frac = frac * 10.0 + (s[i] - b'0') as f32;
            scale *= 10.0;
            i += 1;
        }
    }
    (sign * (whole + frac / scale), i)
}

fn cosine(a: &Item<'_>, b: &Item<'_>) -> f32 {
    let mut dot = 0f32;
    let mut aa = 0f32;
    let mut bb = 0f32;
    let mut i = 0usize;
    while i < a.len {
        dot += a.embedding[i] * b.embedding[i];
        aa += a.embedding[i] * a.embedding[i];
        bb += b.embedding[i] * b.embedding[i];
        i += 1;
    }
    if aa == 0.0 || bb == 0.0 {
        0.0
    } else {
        dot / sqrt(aa * bb)
    }
}

fn sqrt(x: f32) -> f32 {
    let mut guess = if x > 1.0 { x } else { 1.0 };
    let mut i = 0usize;
    while i < 8 {
        guess = 0.5 * (guess + x / guess);
        i += 1;
    }
    guess
}

fn find_root(parent: &mut [usize; 16], index: usize) -> usize {
    if parent[index] == index {
        index
    } else {
        let root = find_root(parent, parent[index]);
        parent[index] = root;
        root
    }
}

fn join(parent: &mut [usize; 16], a: usize, b: usize) {
    let ra = find_root(parent, a);
    let rb = find_root(parent, b);
    if ra != rb {
        parent[rb] = ra;
    }
}

#[derive(Copy, Clone)]
struct Group<'a> {
    root: usize,
    members: [&'a [u8]; 16],
    count: usize,
}

struct Groups<'a> {
    groups: [Group<'a>; 16],
    count: usize,
}

impl<'a> Groups<'a> {
    const fn new() -> Self {
        const EMPTY: Group<'static> = Group { root: 0, members: [b""; 16], count: 0 };
        Self { groups: [EMPTY; 16], count: 0 }
    }

    fn push(&mut self, root: usize, id: &'a [u8]) {
        let mut i = 0usize;
        while i < self.count {
            if self.groups[i].root == root {
                self.groups[i].members[self.groups[i].count] = id;
                self.groups[i].count += 1;
                return;
            }
            i += 1;
        }
        self.groups[self.count] = Group { root, members: [b""; 16], count: 1 };
        self.groups[self.count].members[0] = id;
        self.count += 1;
    }

    fn sort(&mut self) {
        let mut i = 0usize;
        while i < self.count {
            sort_members(&mut self.groups[i]);
            i += 1;
        }
        i = 0;
        while i < self.count {
            let mut j = i + 1;
            while j < self.count {
                if self.groups[j].members[0] < self.groups[i].members[0] {
                    let tmp = self.groups[i];
                    self.groups[i] = self.groups[j];
                    self.groups[j] = tmp;
                }
                j += 1;
            }
            i += 1;
        }
    }
}

fn sort_members(group: &mut Group<'_>) {
    let mut i = 0usize;
    while i < group.count {
        let mut j = i + 1;
        while j < group.count {
            if group.members[j] < group.members[i] {
                let tmp = group.members[i];
                group.members[i] = group.members[j];
                group.members[j] = tmp;
            }
            j += 1;
        }
        i += 1;
    }
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

fn number_after(s: &[u8], key: &[u8]) -> Option<f32> {
    let p = find(s, key)?;
    let c = s[p + key.len()..].iter().position(|b| *b == b':')?;
    let rest = skip(&s[p + key.len() + c + 1..]);
    if rest.is_empty() {
        return None;
    }
    Some(parse_number_literal(rest).0)
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
        let input = include_bytes!("../tests/uc01-group.json");
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
        for fixture in [include_bytes!("../tests/uc01-group.json").as_slice()] {
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
