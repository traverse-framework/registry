#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;

const MAX_INPUT_BYTES: usize = 4_000_000;
const MAX_OUTPUT_BYTES: usize = 4_000_000;
const MAX_SAMPLES: usize = 480_000;
const MAX_WINDOWS: usize = 64;

#[repr(C)]
#[cfg(target_arch = "wasm32")]
struct Iovec {
    buffer: *const u8,
    length: usize,
}
#[repr(C)]
#[cfg(target_arch = "wasm32")]
struct IovecMut {
    buffer: *mut u8,
    length: usize,
}
#[link(wasm_import_module = "wasi_snapshot_preview1")]
#[cfg(target_arch = "wasm32")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IovecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const Iovec, count: usize, written: *mut usize) -> u32;
}

#[cfg(target_arch = "wasm32")]
static mut INPUT: [u8; MAX_INPUT_BYTES + 1] = [0; MAX_INPUT_BYTES + 1];
#[cfg(target_arch = "wasm32")]
static mut OUTPUT: [u8; MAX_OUTPUT_BYTES] = [0; MAX_OUTPUT_BYTES];
#[cfg(target_arch = "wasm32")]
static mut SAMPLES: [i16; MAX_SAMPLES] = [0; MAX_SAMPLES];

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        let input_ptr = core::ptr::addr_of_mut!(INPUT).cast::<u8>();
        loop {
            if total == MAX_INPUT_BYTES + 1 {
                break;
            }
            let mut count = 0usize;
            let vector = IovecMut {
                buffer: input_ptr.add(total),
                length: MAX_INPUT_BYTES + 1 - total,
            };
            if fd_read(0, &vector, 1, &mut count) != 0 || count == 0 {
                break;
            }
            total += count;
        }
        let input = core::slice::from_raw_parts(input_ptr, total);
        let output = core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(OUTPUT).cast::<u8>(),
            MAX_OUTPUT_BYTES,
        );
        let samples = core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(SAMPLES).cast::<i16>(),
            MAX_SAMPLES,
        );
        let written = window(input, output, samples);
        let mut count = 0usize;
        let vector = Iovec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: written,
        };
        let _ = fd_write(1, &vector, 1, &mut count);
    }
}

fn window(input: &[u8], output: &mut [u8], samples: &mut [i16]) -> usize {
    if input.len() > MAX_INPUT_BYTES {
        return error(output, "input_limit_exceeded");
    }
    let rate = integer(input, b"\"sample_rate_hz\"").unwrap_or(0);
    let channels = integer(input, b"\"channel_count\"").unwrap_or(0);
    let offset = integer(input, b"\"source_offset_ms\"").unwrap_or(-1);
    let window_ms = integer(input, b"\"window_duration_ms\"").unwrap_or(0);
    let hop_ms = integer(input, b"\"hop_duration_ms\"").unwrap_or(0);
    let final_chunk = boolean(input, b"\"is_final_chunk\"").unwrap_or(false);
    if !(8_000..=192_000).contains(&rate) {
        return error(output, "unsupported_sample_rate");
    }
    if !(1..=2).contains(&channels) {
        return error(output, "unsupported_channel_count");
    }
    if offset < 0 || window_ms < 1 || hop_ms < 1 || hop_ms > window_ms {
        return error(output, "invalid_window_policy");
    }
    let Some(sample_count) = sample_array(input, samples) else {
        return error(output, "invalid_samples");
    };
    if sample_count == 0 || sample_count % channels as usize != 0 {
        return error(output, "invalid_samples");
    }
    let frames = sample_count / channels as usize;
    let window_numerator = window_ms as u64 * rate as u64;
    let hop_numerator = hop_ms as u64 * rate as u64;
    if window_numerator % 1000 != 0 || hop_numerator % 1000 != 0 {
        return error(output, "unaligned_window_policy");
    }
    let window_frames_u64 = window_numerator / 1000;
    let hop_frames_u64 = hop_numerator / 1000;
    if window_frames_u64 == 0 || hop_frames_u64 == 0 {
        return error(output, "invalid_window_policy");
    }
    if window_frames_u64 > MAX_SAMPLES as u64 || hop_frames_u64 > MAX_SAMPLES as u64 {
        return error(output, "output_limit_exceeded");
    }
    let window_frames = window_frames_u64 as usize;
    let hop_frames = hop_frames_u64 as usize;
    let full_window_count = if frames < window_frames {
        0
    } else {
        1 + (frames - window_frames) / hop_frames
    };
    let has_partial_tail = frames < window_frames || (frames - window_frames) % hop_frames != 0;
    let count = if final_chunk {
        full_window_count + usize::from(has_partial_tail)
    } else {
        full_window_count
    };
    let window_samples = window_frames.saturating_mul(channels as usize);
    if count > MAX_WINDOWS || count.saturating_mul(window_samples) > MAX_SAMPLES {
        return error(output, "output_limit_exceeded");
    }
    let mut writer = Writer {
        bytes: output,
        position: 0,
    };
    writer.bytes(b"{\"result_class\":\"windowed\",\"source_offset_ms\":");
    writer.number(offset as u64);
    writer.bytes(b",\"sample_rate_hz\":");
    writer.number(rate as u64);
    writer.bytes(b",\"channel_count\":");
    writer.number(channels as u64);
    writer.bytes(b",\"window_duration_ms\":");
    writer.number(window_ms as u64);
    writer.bytes(b",\"hop_duration_ms\":");
    writer.number(hop_ms as u64);
    writer.bytes(b",\"consumed_frames\":");
    writer.number(if final_chunk {
        frames as u64
    } else {
        (count * hop_frames) as u64
    });
    writer.bytes(b",\"windows\":[");
    for index in 0..count {
        if index > 0 {
            writer.bytes(b",");
        }
        let start_frame = index * hop_frames;
        let valid_frames = core::cmp::min(window_frames, frames.saturating_sub(start_frame));
        let start_ms = offset as u64 + start_frame as u64 * 1000 / rate as u64;
        writer.bytes(b"{\"window_index\":");
        writer.number(index as u64);
        writer.bytes(b",\"start_ms\":");
        writer.number(start_ms);
        writer.bytes(b",\"valid_frames\":");
        writer.number(valid_frames as u64);
        writer.bytes(b",\"samples_s16\":[");
        for position in 0..window_samples {
            if position > 0 {
                writer.bytes(b",");
            }
            let source_index = start_frame * channels as usize + position;
            let value = if position / (channels as usize) < valid_frames {
                samples[source_index]
            } else {
                0
            };
            writer.signed(value as i64);
        }
        writer.bytes(b"]}");
    }
    writer.bytes(b"]}");
    writer.position
}

fn sample_array(input: &[u8], samples: &mut [i16]) -> Option<usize> {
    let key = find(input, b"\"samples_s16\"")?;
    let colon = input[key..].iter().position(|byte| *byte == b':')? + key;
    let mut index = colon + 1;
    skip_space(input, &mut index);
    if input.get(index) != Some(&b'[') {
        return None;
    }
    index += 1;
    let mut count = 0usize;
    loop {
        skip_space(input, &mut index);
        if input.get(index) == Some(&b']') {
            return Some(count);
        }
        if count == samples.len() {
            return None;
        }
        let (value, next) = signed_integer(input, index)?;
        if value < i16::MIN as i32 || value > i16::MAX as i32 {
            return None;
        }
        samples[count] = value as i16;
        count += 1;
        index = next;
        skip_space(input, &mut index);
        match input.get(index) {
            Some(b',') => index += 1,
            Some(b']') => return Some(count),
            _ => return None,
        }
    }
}

fn integer(input: &[u8], key: &[u8]) -> Option<i32> {
    let start = find(input, key)?;
    let colon = input[start..].iter().position(|byte| *byte == b':')? + start + 1;
    let mut index = colon;
    skip_space(input, &mut index);
    signed_integer(input, index).map(|result| result.0)
}

fn boolean(input: &[u8], key: &[u8]) -> Option<bool> {
    let start = find(input, key)?;
    let colon = input[start..].iter().position(|byte| *byte == b':')? + start + 1;
    let mut index = colon;
    skip_space(input, &mut index);
    if input.get(index..index + 4) == Some(b"true") {
        Some(true)
    } else if input.get(index..index + 5) == Some(b"false") {
        Some(false)
    } else {
        None
    }
}
fn signed_integer(input: &[u8], mut index: usize) -> Option<(i32, usize)> {
    let negative = input.get(index) == Some(&b'-');
    if negative {
        index += 1;
    }
    let start = index;
    let mut value = 0i32;
    while let Some(byte @ b'0'..=b'9') = input.get(index).copied() {
        value = value.checked_mul(10)?.checked_add((byte - b'0') as i32)?;
        index += 1;
    }
    if index == start {
        return None;
    }
    Some((if negative { -value } else { value }, index))
}
fn skip_space(input: &[u8], index: &mut usize) {
    while input
        .get(*index)
        .is_some_and(|byte| matches!(*byte, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *index += 1;
    }
}
fn find(input: &[u8], needle: &[u8]) -> Option<usize> {
    input
        .windows(needle.len())
        .position(|window| window == needle)
}

fn error(output: &mut [u8], code: &str) -> usize {
    let mut writer = Writer {
        bytes: output,
        position: 0,
    };
    writer.bytes(b"{\"result_class\":\"");
    writer.bytes(code.as_bytes());
    writer.bytes(b"\"}");
    writer.position
}
struct Writer<'a> {
    bytes: &'a mut [u8],
    position: usize,
}
impl Writer<'_> {
    fn bytes(&mut self, value: &[u8]) {
        let end = self.position.saturating_add(value.len());
        if end <= self.bytes.len() {
            self.bytes[self.position..end].copy_from_slice(value);
            self.position = end;
        }
    }
    fn number(&mut self, value: u64) {
        let mut digits = [0u8; 20];
        let mut count = 0usize;
        let mut value = value;
        loop {
            digits[count] = b'0' + (value % 10) as u8;
            count += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        while count > 0 {
            count -= 1;
            self.bytes(&digits[count..=count]);
        }
    }
    fn signed(&mut self, value: i64) {
        if value < 0 {
            self.bytes(b"-");
            self.number(value.unsigned_abs());
        } else {
            self.number(value as u64);
        }
    }
}
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::{
        boolean, error, find, integer, sample_array, signed_integer, skip_space, window, Writer,
        MAX_OUTPUT_BYTES, MAX_SAMPLES,
    };
    use std::{format, string::String, vec};

    fn run(input: &[u8]) -> String {
        let mut output = vec![0u8; MAX_OUTPUT_BYTES];
        let mut samples = vec![0i16; MAX_SAMPLES];
        let written = window(input, &mut output, &mut samples);
        String::from_utf8(output[..written].to_vec()).expect("JSON output")
    }

    fn request(
        samples: &str,
        rate: &str,
        channels: &str,
        offset: &str,
        window_ms: &str,
        hop_ms: &str,
        final_chunk: &str,
    ) -> String {
        format!("{{\"sample_rate_hz\":{rate},\"channel_count\":{channels},\"source_offset_ms\":{offset},\"window_duration_ms\":{window_ms},\"hop_duration_ms\":{hop_ms},\"is_final_chunk\":{final_chunk},\"samples_s16\":{samples}}}")
    }

    #[test]
    fn windows_overlap_and_zero_pad_final_tail() {
        let output =
            run(request("[1,2,3,4,5,6,7,8,9]", "8000", "1", "100", "1", "1", "true").as_bytes());
        assert!(output.contains("\"result_class\":\"windowed\""));
        assert!(output.contains("\"start_ms\":101"));
        assert!(output.contains("\"samples_s16\":[9,0,0,0,0,0,0,0]"));
    }

    #[test]
    fn rejects_invalid_profiles_alignment_samples_and_limits() {
        for (input, expected) in [
            (
                request("[1]", "7999", "1", "0", "1", "1", "true"),
                "unsupported_sample_rate",
            ),
            (
                request("[1,2,3]", "8000", "3", "0", "1", "1", "true"),
                "unsupported_channel_count",
            ),
            (
                request("[1]", "8000", "1", "-1", "1", "1", "true"),
                "invalid_window_policy",
            ),
            (
                request("[1]", "8000", "1", "0", "1", "2", "true"),
                "invalid_window_policy",
            ),
            (
                request("[1]", "44100", "1", "0", "1", "1", "true"),
                "unaligned_window_policy",
            ),
            (
                request("[32768]", "8000", "1", "0", "1", "1", "true"),
                "invalid_samples",
            ),
        ] {
            assert!(run(input.as_bytes()).contains(expected), "{input}");
        }
        assert!(
            run(request("[1]", "8000", "1", "0", "61000", "1", "true").as_bytes())
                .contains("output_limit_exceeded")
        );
        assert!(run(&vec![b' '; 4_000_001]).contains("input_limit_exceeded"));
    }

    #[test]
    fn exercises_scalar_parsers_and_writer_edges() {
        assert_eq!(integer(br#"{"n": -12}"#, b"\"n\""), Some(-12));
        assert_eq!(integer(b"{}", b"\"n\""), None);
        assert_eq!(boolean(br#"{"b":true}"#, b"\"b\""), Some(true));
        assert_eq!(boolean(br#"{"b":false}"#, b"\"b\""), Some(false));
        assert_eq!(boolean(br#"{"b":null}"#, b"\"b\""), None);
        assert_eq!(find(b"abc", b"z"), None);
        let mut at = 0;
        skip_space(b" \n\t", &mut at);
        assert_eq!(at, 3);
        assert_eq!(signed_integer(b"-", 0), None);
        assert_eq!(signed_integer(b"2147483648", 0), None);
        let mut parsed = [0i16; 2];
        assert_eq!(
            sample_array(br#"{"samples_s16":[1,2]}"#, &mut parsed),
            Some(2)
        );
        assert_eq!(
            sample_array(br#"{"samples_s16":[]}"#, &mut parsed),
            Some(0)
        );
        let mut short = [0u8; 1];
        assert_eq!(error(&mut short, "oversized"), 0);
        let mut bytes = [0u8; 24];
        let mut writer = Writer {
            bytes: &mut bytes,
            position: 0,
        };
        writer.signed(-9);
        assert_eq!(&writer.bytes[..writer.position], b"-9");
    }
}
