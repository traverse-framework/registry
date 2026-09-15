#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[cfg(test)]
extern crate std;

const MAX_INPUT: usize = 4_000_000;
const MAX_WAV: usize = 2_000_000;
const MAX_OUTPUT: usize = 4_000_000;
const MAX_SAMPLES: usize = 480_000;

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
static mut INPUT: [u8; MAX_INPUT + 1] = [0; MAX_INPUT + 1];
#[cfg(target_arch = "wasm32")]
static mut WAV: [u8; MAX_WAV] = [0; MAX_WAV];
#[cfg(target_arch = "wasm32")]
static mut OUTPUT: [u8; MAX_OUTPUT] = [0; MAX_OUTPUT];

#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    unsafe {
        let mut total = 0usize;
        let input_ptr = core::ptr::addr_of_mut!(INPUT).cast::<u8>();
        while total < MAX_INPUT + 1 {
            let mut count = 0usize;
            let vector = IovecMut {
                buffer: input_ptr.add(total),
                length: MAX_INPUT + 1 - total,
            };
            if fd_read(0, &vector, 1, &mut count) != 0 || count == 0 {
                break;
            }
            total += count;
        }
        let input = core::slice::from_raw_parts(input_ptr, total);
        let output = core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(OUTPUT).cast::<u8>(),
            MAX_OUTPUT,
        );
        let wav =
            core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(WAV).cast::<u8>(), MAX_WAV);
        let length = decode(input, output, wav);
        let mut written = 0usize;
        let vector = Iovec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length,
        };
        let _ = fd_write(1, &vector, 1, &mut written);
    }
}

fn decode(input: &[u8], output: &mut [u8], wav: &mut [u8]) -> usize {
    if input.len() > MAX_INPUT {
        return error(output, "input_limit_exceeded");
    }
    let Some(wav_len) = base64_value(input, wav) else {
        return error(output, "invalid_audio_bytes");
    };
    if wav_len > MAX_WAV {
        return error(output, "input_limit_exceeded");
    }
    let bytes = &wav[..wav_len];
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return error(output, "invalid_container");
    }
    let Some(riff_size) = u32_at(bytes, 4).map(|value| value as usize) else {
        return error(output, "invalid_container");
    };
    let Some(riff_end) = riff_size.checked_add(8) else {
        return error(output, "invalid_container");
    };
    if riff_end < 12 || riff_end > bytes.len() {
        return error(output, "invalid_container");
    }
    let mut cursor = 12usize;
    let mut format = None;
    let mut data = None;
    while cursor < riff_end {
        if riff_end - cursor < 8 {
            return error(output, "invalid_container");
        }
        let id = &bytes[cursor..cursor + 4];
        let Some(size) = u32_at(bytes, cursor + 4).map(|value| value as usize) else {
            return error(output, "invalid_container");
        };
        let start = cursor + 8;
        let Some(end) = start.checked_add(size) else {
            return error(output, "invalid_container");
        };
        if end > riff_end {
            return error(output, "invalid_container");
        }
        if id == b"fmt " {
            if format.is_some() || size < 16 {
                return error(output, "invalid_container");
            }
            let (Some(tag), Some(channels), Some(rate), Some(byte_rate), Some(align), Some(bits)) = (
                u16_at(bytes, start),
                u16_at(bytes, start + 2),
                u32_at(bytes, start + 4),
                u32_at(bytes, start + 8),
                u16_at(bytes, start + 12),
                u16_at(bytes, start + 14),
            ) else {
                return error(output, "invalid_container");
            };
            format = Some((tag, channels, rate, byte_rate, align, bits));
        } else if id == b"data" {
            if data.is_some() {
                return error(output, "invalid_container");
            }
            data = Some((start, size));
        }
        let Some(next) = end.checked_add(size & 1) else {
            return error(output, "invalid_container");
        };
        if next > riff_end {
            return error(output, "invalid_container");
        }
        cursor = next;
    }
    let Some((tag, channels, rate, byte_rate, align, bits)) = format else {
        return error(output, "invalid_container");
    };
    if tag != 1 || bits != 16 {
        return error(output, "unsupported_format");
    }
    if !(8_000..=192_000).contains(&rate) {
        return error(output, "unsupported_sample_rate");
    }
    if !(1..=2).contains(&channels) {
        return error(output, "unsupported_channel_count");
    }
    if align != channels * 2 || byte_rate != rate.saturating_mul(align as u32) {
        return error(output, "invalid_container");
    }
    let Some((data_start, data_size)) = data else {
        return error(output, "invalid_container");
    };
    if data_size == 0 || data_size % align as usize != 0 {
        return error(output, "invalid_pcm_frames");
    }
    let sample_count = data_size / 2;
    if sample_count > MAX_SAMPLES {
        return error(output, "output_limit_exceeded");
    }
    let mut writer = Writer {
        bytes: output,
        pos: 0,
    };
    writer.bytes(b"{\"result_class\":\"decoded\",\"sample_format\":\"s16le\",\"sample_rate_hz\":");
    writer.number(rate as u64);
    writer.bytes(b",\"channel_count\":");
    writer.number(channels as u64);
    writer.bytes(b",\"frame_count\":");
    writer.number((data_size / align as usize) as u64);
    writer.bytes(b",\"samples_s16\":[");
    for i in 0..sample_count {
        if i > 0 {
            writer.bytes(b",");
        }
        let at = data_start + i * 2;
        writer.signed(i16::from_le_bytes([bytes[at], bytes[at + 1]]) as i64);
    }
    writer.bytes(b"]}");
    writer.pos
}

fn base64_value(input: &[u8], output: &mut [u8]) -> Option<usize> {
    let key = find(input, b"\"wav_bytes_base64\"")?;
    let mut cursor = key + input[key..].iter().position(|byte| *byte == b':')? + 1;
    spaces(input, &mut cursor);
    if input.get(cursor) != Some(&b'"') {
        return None;
    }
    cursor += 1;
    let start = cursor;
    while input.get(cursor).is_some_and(|byte| *byte != b'"') {
        cursor += 1;
    }
    if input.get(cursor) != Some(&b'"') {
        return None;
    }
    let encoded = &input[start..cursor];
    if encoded.len() % 4 != 0 || encoded.is_empty() {
        return None;
    }
    let mut out = 0usize;
    for (index, q) in encoded.chunks_exact(4).enumerate() {
        let last = index + 1 == encoded.len() / 4;
        let a = digit(q[0])? as u32;
        let b = digit(q[1])? as u32;
        let c_pad = q[2] == b'=';
        let d_pad = q[3] == b'=';
        if c_pad && (!d_pad || !last) || d_pad && !last {
            return None;
        }
        let c = if c_pad { 0 } else { digit(q[2])? as u32 };
        let d = if d_pad { 0 } else { digit(q[3])? as u32 };
        if c_pad && b & 15 != 0 || d_pad && !c_pad && c & 3 != 0 {
            return None;
        }
        let value = a << 18 | b << 12 | c << 6 | d;
        let count = if c_pad {
            1
        } else if d_pad {
            2
        } else {
            3
        };
        if out + count > output.len() {
            return None;
        }
        output[out] = (value >> 16) as u8;
        if count > 1 {
            output[out + 1] = (value >> 8) as u8;
        }
        if count > 2 {
            output[out + 2] = value as u8;
        }
        out += count;
    }
    Some(out)
}

fn digit(b: u8) -> Option<u8> {
    match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
fn find(input: &[u8], key: &[u8]) -> Option<usize> {
    input.windows(key.len()).position(|part| part == key)
}
fn spaces(input: &[u8], at: &mut usize) {
    while input
        .get(*at)
        .is_some_and(|b| matches!(*b, b' ' | b'\n' | b'\r' | b'\t'))
    {
        *at += 1;
    }
}
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        *bytes.get(at + 3)?,
    ]))
}
fn error(out: &mut [u8], class: &str) -> usize {
    let mut w = Writer { bytes: out, pos: 0 };
    w.bytes(b"{\"result_class\":\"");
    w.bytes(class.as_bytes());
    w.bytes(b"\"}");
    w.pos
}
struct Writer<'a> {
    bytes: &'a mut [u8],
    pos: usize,
}
impl Writer<'_> {
    fn bytes(&mut self, b: &[u8]) {
        let end = self.pos.saturating_add(b.len());
        if end <= self.bytes.len() {
            self.bytes[self.pos..end].copy_from_slice(b);
            self.pos = end;
        }
    }
    fn number(&mut self, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (value % 10) as u8;
            n += 1;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            self.bytes(&digits[n..=n]);
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
        base64_value, decode, digit, error, find, spaces, u16_at, u32_at, Writer, MAX_INPUT,
        MAX_OUTPUT, MAX_WAV,
    };
    use std::{format, string::String, vec, vec::Vec};

    fn run(input: &[u8]) -> String {
        let mut output = vec![0u8; MAX_OUTPUT];
        let mut wav = vec![0u8; MAX_WAV];
        let length = decode(input, &mut output, &mut wav);
        String::from_utf8(output[..length].to_vec()).expect("JSON output")
    }

    fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        if payload.len() % 2 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn wave(
        tag: u16,
        channels: u16,
        rate: u32,
        bits: u16,
        samples: &[i16],
        data_first: bool,
        odd_metadata: bool,
    ) -> Vec<u8> {
        let align = channels * (bits / 8);
        let mut fmt = vec![0u8; 16];
        fmt[0..2].copy_from_slice(&tag.to_le_bytes());
        fmt[2..4].copy_from_slice(&channels.to_le_bytes());
        fmt[4..8].copy_from_slice(&rate.to_le_bytes());
        fmt[8..12].copy_from_slice(&rate.saturating_mul(align as u32).to_le_bytes());
        fmt[12..14].copy_from_slice(&align.to_le_bytes());
        fmt[14..16].copy_from_slice(&bits.to_le_bytes());
        let mut data = Vec::new();
        for sample in samples {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let mut body = b"WAVE".to_vec();
        if data_first {
            body.extend_from_slice(&chunk(b"data", &data));
        }
        body.extend_from_slice(&chunk(b"fmt ", &fmt));
        if odd_metadata {
            body.extend_from_slice(&chunk(b"JUNK", &[0x45]));
        }
        if !data_first {
            body.extend_from_slice(&chunk(b"data", &data));
        }
        let mut output = b"RIFF".to_vec();
        output.extend_from_slice(&(body.len() as u32).to_le_bytes());
        output.extend_from_slice(&body);
        output
    }

    fn encode(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for group in bytes.chunks(3) {
            let a = group[0] as u32;
            let b = group.get(1).copied().unwrap_or(0) as u32;
            let c = group.get(2).copied().unwrap_or(0) as u32;
            let bits = a << 16 | b << 8 | c;
            out.push(TABLE[((bits >> 18) & 63) as usize]);
            out.push(TABLE[((bits >> 12) & 63) as usize]);
            out.push(if group.len() > 1 {
                TABLE[((bits >> 6) & 63) as usize]
            } else {
                b'='
            });
            out.push(if group.len() > 2 {
                TABLE[(bits & 63) as usize]
            } else {
                b'='
            });
        }
        String::from_utf8(out).unwrap()
    }

    fn request(wav: &[u8]) -> String {
        format!("{{\"wav_bytes_base64\":\"{}\"}}", encode(wav))
    }

    fn wrap_body(body: &[u8]) -> Vec<u8> {
        let mut result = b"RIFF".to_vec();
        result.extend_from_slice(&(body.len() as u32).to_le_bytes());
        result.extend_from_slice(body);
        result
    }

    #[test]
    fn decodes_pcm16_and_scans_chunk_order_and_padding() {
        for wav in [
            wave(1, 1, 8000, 16, &[0, 2000], false, false),
            wave(1, 2, 16000, 16, &[-2, 3], true, true),
        ] {
            let output = run(request(&wav).as_bytes());
            assert!(output.contains("\"result_class\":\"decoded\""));
            assert!(output.contains("\"sample_format\":\"s16le\""));
        }
    }

    #[test]
    fn rejects_malformed_container_profiles_and_bounds() {
        assert!(run(b"{}").contains("invalid_audio_bytes"));
        assert!(run(br#"{"wav_bytes_base64":"***"}"#).contains("invalid_audio_bytes"));
        assert!(run(br#"{"wav_bytes_base64":"A==="}"#).contains("invalid_audio_bytes"));
        assert!(run(request(b"not-wave").as_bytes()).contains("invalid_container"));

        let valid = wave(1, 1, 8000, 16, &[1, 2], false, false);
        let mut bad_magic = valid.clone();
        bad_magic[0] = b'X';
        assert!(run(request(&bad_magic).as_bytes()).contains("invalid_container"));
        let mut bad_wave = valid.clone();
        bad_wave[8] = b'X';
        assert!(run(request(&bad_wave).as_bytes()).contains("invalid_container"));
        let mut bad_size = valid.clone();
        bad_size[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(run(request(&bad_size).as_bytes()).contains("invalid_container"));
        let mut bad_align = valid.clone();
        bad_align[32..34].copy_from_slice(&4u16.to_le_bytes());
        assert!(run(request(&bad_align).as_bytes()).contains("invalid_container"));
        let mut bad_fmt = wave(1, 1, 8000, 16, &[1, 2], false, false);
        bad_fmt[16..20].copy_from_slice(&15u32.to_le_bytes());
        assert!(run(request(&bad_fmt).as_bytes()).contains("invalid_container"));
        for (tag, channels, rate, bits, expected) in [
            (3, 1, 8000, 16, "unsupported_format"),
            (1, 1, 4000, 16, "unsupported_sample_rate"),
            (1, 3, 8000, 16, "unsupported_channel_count"),
            (1, 1, 8000, 8, "unsupported_format"),
        ] {
            assert!(run(
                request(&wave(tag, channels, rate, bits, &[1, 2], false, false)).as_bytes()
            )
            .contains(expected));
        }
        assert!(
            run(request(&wave(1, 1, 8000, 16, &[], false, false)).as_bytes())
                .contains("invalid_pcm_frames")
        );
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(&chunk(b"data", &[1, 0]));
        assert!(run(request(&wrap_body(&body)).as_bytes()).contains("invalid_container"));
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(&chunk(b"fmt ", &valid[20..36]));
        assert!(run(request(&wrap_body(&body)).as_bytes()).contains("invalid_container"));
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(&chunk(b"fmt ", &[0; 16]));
        body.extend_from_slice(&chunk(b"data", &[1, 0]));
        body.extend_from_slice(&chunk(b"data", &[2, 0]));
        assert!(run(request(&wrap_body(&body)).as_bytes()).contains("invalid_container"));
        let truncated_header = b"WAVEJUNK".to_vec();
        assert!(
            run(request(&wrap_body(&truncated_header)).as_bytes()).contains("invalid_container")
        );
        let mut oversized_chunk = b"WAVEJUNK".to_vec();
        oversized_chunk.extend_from_slice(&8u32.to_le_bytes());
        assert!(run(request(&wrap_body(&oversized_chunk)).as_bytes()).contains("invalid_container"));
        let mut missing_pad = b"WAVEJUNK".to_vec();
        missing_pad.extend_from_slice(&1u32.to_le_bytes());
        missing_pad.push(1);
        assert!(run(request(&wrap_body(&missing_pad)).as_bytes()).contains("invalid_container"));
        let too_many = vec![1i16; 480_002];
        assert!(
            run(request(&wave(1, 1, 8000, 16, &too_many, false, false)).as_bytes())
                .contains("output_limit_exceeded")
        );
        let large_input = vec![b' '; MAX_INPUT + 1];
        assert!(run(&large_input).contains("input_limit_exceeded"));
    }

    #[test]
    fn exercises_base64_binary_reader_and_writer_edges() {
        assert_eq!(digit(b'A'), Some(0));
        assert_eq!(digit(b'/'), Some(63));
        assert_eq!(digit(b'?'), None);
        assert_eq!(find(b"abc", b"b"), Some(1));
        assert_eq!(find(b"abc", b"z"), None);
        let mut at = 0;
        spaces(b" \n\t", &mut at);
        assert_eq!(at, 3);
        assert_eq!(u16_at(b"\x01\x02", 0), Some(513));
        assert_eq!(u16_at(b"\x01", 0), None);
        assert_eq!(u32_at(b"\x01\x02\x03\x04", 0), Some(67_305_985));
        assert_eq!(u32_at(b"\x01", 0), None);
        let mut decoded = [0u8; 8];
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":"AQI="}"#, &mut decoded),
            Some(2)
        );
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":"AB=="}"#, &mut decoded),
            None
        );
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":4}"#, &mut decoded),
            None
        );
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":"unfinished"#, &mut decoded),
            None
        );
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":"AAAA"}"#, &mut decoded[..1]),
            None
        );
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":"AA==AAAA"}"#, &mut decoded),
            None
        );
        assert_eq!(
            base64_value(br#"{"wav_bytes_base64":"AAA=AAAA"}"#, &mut decoded),
            None
        );
        let huge = vec![0u8; MAX_WAV + 1];
        let large_request = request(&huge);
        let mut larger_wav = vec![0u8; MAX_WAV + 1];
        let mut large_output = vec![0u8; MAX_OUTPUT];
        let length = decode(large_request.as_bytes(), &mut large_output, &mut larger_wav);
        assert!(String::from_utf8(large_output[..length].to_vec())
            .unwrap()
            .contains("input_limit_exceeded"));
        let mut short = [0u8; 1];
        assert_eq!(error(&mut short, "long error"), 0);
        let mut bytes = [0u8; 24];
        let mut writer = Writer {
            bytes: &mut bytes,
            pos: 0,
        };
        writer.signed(-7);
        assert_eq!(&writer.bytes[..writer.pos], b"-7");
    }
}
