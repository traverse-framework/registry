#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

const INPUT_LIMIT: usize = 1_000_000;
const OUTPUT_LIMIT: usize = 32_768;
const WINDOW_LIMIT: usize = 4096;

#[repr(C)] struct Iovec { buffer: *const u8, length: usize }
#[repr(C)] struct IovecMut { buffer: *mut u8, length: usize }
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IovecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const Iovec, count: usize, written: *mut usize) -> u32;
}
static mut INPUT: [u8; INPUT_LIMIT] = [0; INPUT_LIMIT];
static mut OUTPUT: [u8; OUTPUT_LIMIT] = [0; OUTPUT_LIMIT];

#[unsafe(no_mangle)] pub extern "C" fn _start() {
    unsafe {
        let input_ptr = core::ptr::addr_of_mut!(INPUT).cast::<u8>();
        let mut length = 0usize;
        loop {
            let mut read = 0usize;
            let vec = IovecMut { buffer: input_ptr.add(length), length: INPUT_LIMIT - length };
            if fd_read(0, &vec, 1, &mut read) != 0 || read == 0 { break; }
            length += read;
            if length == INPUT_LIMIT { break; }
        }
        let input = core::slice::from_raw_parts(input_ptr, length);
        let output = core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(OUTPUT).cast::<u8>(), OUTPUT_LIMIT);
        let written = classify(input, output);
        let vec = Iovec { buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(), length: written };
        let mut ignored = 0usize;
        let _ = fd_write(1, &vec, 1, &mut ignored);
    }
}

fn find(input: &[u8], key: &[u8], from: usize) -> Option<usize> {
    input.get(from..)?.windows(key.len()).position(|w| w == key).map(|p| p + from)
}
fn integer(input: &[u8], key: &[u8], from: usize) -> Option<i32> {
    let p = find(input, key, from)?;
    let colon = input[p + key.len()..].iter().position(|b| *b == b':')?;
    let mut rest = &input[p + key.len() + colon + 1..];
    while rest.first().is_some_and(|b| b.is_ascii_whitespace()) { rest = &rest[1..]; }
    let mut value = 0i32; let mut any = false;
    for b in rest { if !b.is_ascii_digit() { break; } value = value.checked_mul(10)?.checked_add((*b - b'0') as i32)?; any = true; }
    if any { Some(value) } else { None }
}
fn quality_state(input: &[u8], from: usize) -> &'static [u8] {
    let end = find(input, b"}", from).unwrap_or(input.len());
    let object = &input[from..end];
    if object.windows(6).any(|w| w == b"quiet\"") { b"silence" }
    else if object.windows(8).any(|w| w == b"clipped\"") { b"clipped" }
    else if object.windows(19).any(|w| w == b"background_dominant") { b"background" }
    else { b"possible-event" }
}
fn put(output: &mut [u8], at: &mut usize, bytes: &[u8]) { if *at + bytes.len() <= output.len() { output[*at..*at + bytes.len()].copy_from_slice(bytes); *at += bytes.len(); } }
fn number(output: &mut [u8], at: &mut usize, mut value: i32) { if value == 0 { put(output, at, b"0"); return; } let mut digits = [0u8; 12]; let mut count = 0; while value > 0 { digits[count] = b'0' + (value % 10) as u8; value /= 10; count += 1; } while count > 0 { count -= 1; put(output, at, &digits[count..count + 1]); } }
fn emit_interval(output: &mut [u8], at: &mut usize, first: bool, start: i32, end: i32, activity: &[u8], count: i32) {
    if !first { put(output, at, b","); }
    put(output, at, b"{\"start_ms\":"); number(output, at, start); put(output, at, b",\"end_ms\":"); number(output, at, end); put(output, at, b",\"activity\":\""); put(output, at, activity); put(output, at, b"\",\"window_count\":"); number(output, at, count); put(output, at, b"}");
}
fn classify(input: &[u8], output: &mut [u8]) -> usize {
    let mut at = 0usize; put(output, &mut at, b"{\"policy_version\":\"activity-1\",\"intervals\":[");
    let (mut cursor, mut windows, mut first) = (0usize, 0usize, true);
    let (mut start, mut end, mut count) = (0i32, 0i32, 0i32); let mut activity: &[u8] = b"";
    loop {
        let Some(position) = find(input, b"\"start_ms\"", cursor) else { break; };
        if windows >= WINDOW_LIMIT { return error(output, b"window_limit_exceeded"); }
        let next_start = integer(input, b"\"start_ms\"", position).unwrap_or(-1);
        let next_end = integer(input, b"\"end_ms\"", position).unwrap_or(-1);
        if next_start < 0 || next_end <= next_start { return error(output, b"invalid_window_interval"); }
        let next_activity = quality_state(input, position);
        if count > 0 && next_activity == activity && end == next_start { end = next_end; count += 1; }
        else { if count > 0 { emit_interval(output, &mut at, first, start, end, activity, count); first = false; } start = next_start; end = next_end; count = 1; activity = next_activity; }
        windows += 1; cursor = position + 10;
    }
    if count > 0 { emit_interval(output, &mut at, first, start, end, activity, count); first = false; }
    put(output, &mut at, b"],\"represented_window_count\":"); number(output, &mut at, windows as i32); put(output, &mut at, b"}"); at
}
fn error(output: &mut [u8], code: &[u8]) -> usize { let mut at = 0; put(output, &mut at, b"{\"error\":\""); put(output, &mut at, code); put(output, &mut at, b"\"}"); at }
#[cfg(not(test))]
#[panic_handler] fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop {} }
