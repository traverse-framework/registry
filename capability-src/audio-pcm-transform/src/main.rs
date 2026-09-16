#![no_std]
#![no_main]

const MAX_INPUT_BYTES: usize = 4_000_000;
const MAX_OUTPUT_BYTES: usize = 4_000_000;
const MAX_SAMPLES: usize = 480_000;

#[repr(C)]
struct Iovec {
    buffer: *const u8,
    length: usize,
}
#[repr(C)]
struct IovecMut {
    buffer: *mut u8,
    length: usize,
}

#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IovecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const Iovec, count: usize, written: *mut usize) -> u32;
}

static mut INPUT: [u8; MAX_INPUT_BYTES + 1] = [0; MAX_INPUT_BYTES + 1];
static mut OUTPUT: [u8; MAX_OUTPUT_BYTES] = [0; MAX_OUTPUT_BYTES];
static mut SOURCE: [i16; MAX_SAMPLES] = [0; MAX_SAMPLES];
static mut TARGET: [i16; MAX_SAMPLES] = [0; MAX_SAMPLES];

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
        let source = core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(SOURCE).cast::<i16>(),
            MAX_SAMPLES,
        );
        let target = core::slice::from_raw_parts_mut(
            core::ptr::addr_of_mut!(TARGET).cast::<i16>(),
            MAX_SAMPLES,
        );
        let written = transform(input, output, source, target);
        let mut count = 0usize;
        let vector = Iovec {
            buffer: core::ptr::addr_of!(OUTPUT).cast::<u8>(),
            length: written,
        };
        let _ = fd_write(1, &vector, 1, &mut count);
    }
}

fn transform(input: &[u8], output: &mut [u8], source: &mut [i16], target: &mut [i16]) -> usize {
    if input.len() > MAX_INPUT_BYTES {
        return error(output, "input_limit_exceeded");
    }
    let input_rate = integer(input, b"\"input_sample_rate_hz\"").unwrap_or(0);
    let input_channels = integer(input, b"\"input_channel_count\"").unwrap_or(0);
    let target_rate = integer(input, b"\"target_sample_rate_hz\"").unwrap_or(0);
    let target_channels = integer(input, b"\"target_channel_count\"").unwrap_or(0);
    if input_rate < 8_000 || input_rate > 192_000 || target_rate < 8_000 || target_rate > 192_000 {
        return error(output, "unsupported_sample_rate");
    }
    if !(1..=2).contains(&input_channels) || !(1..=2).contains(&target_channels) {
        return error(output, "unsupported_channel_count");
    }
    let Some(sample_count) = sample_array(input, source) else {
        return error(output, "invalid_samples");
    };
    if sample_count == 0 || sample_count % input_channels as usize != 0 {
        return error(output, "invalid_samples");
    }
    let input_frames = sample_count / input_channels as usize;
    let target_frames =
        (input_frames as u64 * target_rate as u64 + input_rate as u64 / 2) / input_rate as u64;
    if target_frames == 0 || target_frames > (MAX_SAMPLES / target_channels as usize) as u64 {
        return error(output, "output_limit_exceeded");
    }
    let output_sample_count = target_frames as usize * target_channels as usize;
    for frame in 0..target_frames as usize {
        for channel in 0..target_channels as usize {
            let sample = if target_rate < input_rate {
                downsample_box_average(
                    source,
                    input_channels as usize,
                    input_frames,
                    frame,
                    channel,
                    target_channels as usize,
                    input_rate as u64,
                    target_rate as u64,
                )
            } else {
                let position = frame as u64 * input_rate as u64;
                let left = (position / target_rate as u64) as usize;
                let fraction = position % target_rate as u64;
                let right = core::cmp::min(left + 1, input_frames - 1);
                let a = source_frame(
                    source,
                    input_channels as usize,
                    input_frames,
                    left,
                    channel,
                    target_channels as usize,
                );
                let b = source_frame(
                    source,
                    input_channels as usize,
                    input_frames,
                    right,
                    channel,
                    target_channels as usize,
                );
                let weighted =
                    a as i64 * (target_rate as i64 - fraction as i64) + b as i64 * fraction as i64;
                weighted / target_rate as i64
            };
            target[frame * target_channels as usize + channel] =
                sample.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
        }
    }
    write_success(
        output,
        input_rate,
        input_channels,
        target_rate,
        target_channels,
        target_frames as usize,
        &target[..output_sample_count],
    )
}

fn downsample_box_average(
    samples: &[i16],
    input_channels: usize,
    input_frames: usize,
    output_frame: usize,
    output_channel: usize,
    output_channels: usize,
    input_rate: u64,
    target_rate: u64,
) -> i64 {
    // Integrate input-frame bins across one output-frame interval. This is a
    // bounded box low-pass before decimation, not a studio-grade polyphase FIR.
    let start = output_frame as u64 * input_rate;
    let source_end = input_frames as u64 * target_rate;
    let end = core::cmp::min(start.saturating_add(input_rate), source_end);
    let mut cursor = start;
    let mut weighted_sum = 0i64;
    let mut total_weight = 0u64;
    while cursor < end {
        let source_frame_index = (cursor / target_rate) as usize;
        if source_frame_index >= input_frames {
            break;
        }
        let source_bin_end = (source_frame_index as u64 + 1) * target_rate;
        let segment_end = core::cmp::min(end, source_bin_end);
        let weight = segment_end - cursor;
        let sample = source_frame(
            samples,
            input_channels,
            input_frames,
            source_frame_index,
            output_channel,
            output_channels,
        );
        weighted_sum += sample as i64 * weight as i64;
        total_weight += weight;
        cursor = segment_end;
    }
    if total_weight == 0 {
        0
    } else {
        weighted_sum / total_weight as i64
    }
}

fn source_frame(
    samples: &[i16],
    input_channels: usize,
    frames: usize,
    frame: usize,
    output_channel: usize,
    output_channels: usize,
) -> i16 {
    let frame = core::cmp::min(frame, frames - 1);
    if input_channels == 2 && output_channels == 1 {
        return ((samples[frame * 2] as i32 + samples[frame * 2 + 1] as i32) / 2) as i16;
    }
    let channel = if input_channels == 1 {
        0
    } else {
        output_channel
    };
    samples[frame * input_channels + channel]
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

fn write_success(
    output: &mut [u8],
    input_rate: i32,
    input_channels: i32,
    target_rate: i32,
    target_channels: i32,
    frames: usize,
    samples: &[i16],
) -> usize {
    let mut writer = Writer {
        bytes: output,
        position: 0,
    };
    writer.bytes(
        b"{\"result_class\":\"transformed\",\"sample_format\":\"s16le\",\"input_sample_rate_hz\":",
    );
    writer.number(input_rate as i64);
    writer.bytes(b",\"input_channel_count\":");
    writer.number(input_channels as i64);
    writer.bytes(b",\"sample_rate_hz\":");
    writer.number(target_rate as i64);
    writer.bytes(b",\"channel_count\":");
    writer.number(target_channels as i64);
    writer.bytes(b",\"frame_count\":");
    writer.number(frames as i64);
    writer.bytes(b",\"samples_s16\":[");
    for (index, sample) in samples.iter().enumerate() {
        if index > 0 {
            writer.bytes(b",");
        }
        writer.number(*sample as i64);
    }
    writer.bytes(b"]}");
    writer.position
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
    fn number(&mut self, value: i64) {
        if value < 0 {
            self.bytes(b"-");
            self.unsigned(value.unsigned_abs());
        } else {
            self.unsigned(value as u64);
        }
    }
    fn unsigned(&mut self, mut value: u64) {
        let mut digits = [0u8; 20];
        let mut count = 0usize;
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
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
