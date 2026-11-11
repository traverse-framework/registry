//! `audio.recording-analyze`: quality and activity analysis of one recording,
//! invoked from an app state machine's `analyzing` state (registry#594,
//! decision-log entry 141).
//!
//! Input is exactly `{"artifact_base64": "<RIFF/WAVE PCM16>"}` (Traverse spec
//! 139 FR-020). The recording is cut into fixed 250 ms windows, each window is
//! scored with `audio.signal-quality-evaluate`'s metrics, and the windows are
//! merged into intervals by `audio.activity-interval-classify`. Both crates and
//! `audio.wav-pcm16-decode`'s base64 + container validation are reused through
//! their lib/bin splits. Window size and every threshold are fixed here and
//! reported as `policy_version`.
//!
//! Failures exit non-zero so the state machine takes `capability_failed`.
//! Traverse's nested runtime drops stdout on failure, so the exit status is
//! the stable failure code (entry 141 / registry#594): 10
//! `unsupported_format`, 11 `input_limit_exceeded`, 12 `corrupt_input`.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

use activity_interval_classify::classify;
use signal_quality_evaluate::{
    evaluate, state, Metrics, DEFAULT_CLIP_RATIO_MILLI, DEFAULT_CLIP_THRESHOLD,
    DEFAULT_QUIET_RMS_MILLI,
};
use wav_pcm16_decode::{base64_field, parse_wav};

pub const POLICY_VERSION: &[u8] = b"recording-analyze-1";
pub const WINDOW_MS: u64 = 250;
/// Largest accepted recording: 8 MiB of WAV (entry 141 Q4).
pub const MAX_WAV: usize = 8 * 1024 * 1024;
/// Base64 of `MAX_WAV` bytes, plus room for the JSON wrapper.
pub const MAX_INPUT: usize = MAX_WAV.div_ceil(3) * 4 + 4096;
/// Decode buffer: anything the input bound allows, so oversize is detected
/// as `input_limit_exceeded` rather than as a base64 failure.
pub const WAV_BUFFER: usize = MAX_WAV + 4096;
/// Most windows a capped recording can produce (8 kHz mono: 2098).
pub const MAX_WINDOWS: usize = 2100;
/// Interleaved samples in one 250 ms window at 192 kHz stereo.
pub const MAX_WINDOW_SAMPLES: usize = 192_000 / 4 * 2;
pub const WINDOWS_JSON: usize = MAX_WINDOWS * 80 + 64;
pub const INTERVALS_JSON: usize = MAX_WINDOWS * 96 + 128;
pub const MAX_OUTPUT: usize = INTERVALS_JSON + 1024;

/// Verdict thresholds (policy `recording-analyze-1`): a recording is
/// `clipped` when at least this share (per mille) of windows is clipped, and
/// `low_signal` when fewer than this share of windows is active.
pub const CLIPPED_WINDOWS_MILLI: usize = 500;
pub const ACTIVE_WINDOWS_MILLI: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    UnsupportedFormat,
    InputLimitExceeded,
    CorruptInput,
}

impl Failure {
    pub fn exit_status(self) -> u32 {
        match self {
            Failure::UnsupportedFormat => 10,
            Failure::InputLimitExceeded => 11,
            Failure::CorruptInput => 12,
        }
    }
    pub fn code(self) -> &'static [u8] {
        match self {
            Failure::UnsupportedFormat => b"analysis.unsupported_format",
            Failure::InputLimitExceeded => b"analysis.input_limit_exceeded",
            Failure::CorruptInput => b"analysis.corrupt_input",
        }
    }
}

/// Working buffers, sized by the constants above.
pub struct Buffers<'a> {
    pub wav: &'a mut [u8],
    pub samples: &'a mut [i16],
    pub windows_json: &'a mut [u8],
    pub intervals_json: &'a mut [u8],
    pub output: &'a mut [u8],
}

pub fn analyze(input: &[u8], buffers: &mut Buffers<'_>) -> Result<usize, Failure> {
    if input.len() > MAX_INPUT {
        return Err(Failure::InputLimitExceeded);
    }
    let wav_len =
        base64_field(input, b"\"artifact_base64\"", buffers.wav).ok_or(Failure::CorruptInput)?;
    if wav_len > MAX_WAV {
        return Err(Failure::InputLimitExceeded);
    }
    let bytes = &buffers.wav[..wav_len];
    let format = parse_wav(bytes).map_err(|class| match class {
        "unsupported_format" | "unsupported_sample_rate" | "unsupported_channel_count" => {
            Failure::UnsupportedFormat
        }
        _ => Failure::CorruptInput,
    })?;

    let rate = format.sample_rate_hz as u64;
    let channels = format.channel_count as usize;
    let frames = (format.data_size / format.block_align as usize) as u64;
    let duration_ms = (frames * 1000).div_ceil(rate);

    let mut total = Metrics {
        sample_count: 0,
        sum_squares: 0,
        peak: 0,
        clipping_samples: 0,
        zero_crossings: 0,
    };
    let (mut quiet, mut clipped, mut active) = (0usize, 0usize, 0usize);
    let mut windows = Writer::new(buffers.windows_json);
    windows.put(b"{\"windows\":[");
    let mut window = 0u64;
    loop {
        let first = window * rate / 4;
        if first >= frames {
            break;
        }
        let last = ((window + 1) * rate / 4).min(frames);
        let count = ((last - first) as usize) * channels;
        let start = format.data_start + first as usize * format.block_align as usize;
        for (index, sample) in buffers.samples[..count].iter_mut().enumerate() {
            let at = start + index * 2;
            *sample = i16::from_le_bytes([bytes[at], bytes[at + 1]]);
        }
        let metrics = evaluate(&buffers.samples[..count], DEFAULT_CLIP_THRESHOLD);
        let quality = state(&metrics, DEFAULT_QUIET_RMS_MILLI, DEFAULT_CLIP_RATIO_MILLI);
        match quality {
            b"quiet" => quiet += 1,
            b"clipped" => clipped += 1,
            _ => active += 1,
        }
        total.sample_count += metrics.sample_count;
        total.sum_squares += metrics.sum_squares;
        total.peak = total.peak.max(metrics.peak);
        total.clipping_samples += metrics.clipping_samples;

        let start_ms = window * WINDOW_MS;
        let end_ms = if last == frames {
            duration_ms
        } else {
            (window + 1) * WINDOW_MS
        };
        if window > 0 {
            windows.put(b",");
        }
        windows.put(b"{\"start_ms\":");
        windows.number(start_ms);
        windows.put(b",\"end_ms\":");
        windows.number(end_ms);
        windows.put(b",\"quality\":\"");
        windows.put(quality);
        windows.put(b"\"}");
        window += 1;
    }
    windows.put(b"]}");
    if windows.overflowed {
        return Err(Failure::InputLimitExceeded);
    }
    let window_count = window as usize;
    let windows_len = windows.pos;

    let intervals_len = classify(&buffers.windows_json[..windows_len], buffers.intervals_json);
    let classified = &buffers.intervals_json[..intervals_len];
    let intervals = interval_array(classified).ok_or(
        if find(classified, b"window_limit_exceeded").is_some() {
            Failure::InputLimitExceeded
        } else {
            Failure::CorruptInput
        },
    )?;

    let verdict: &[u8] = if quiet == window_count {
        b"silent"
    } else if clipped * 1000 >= window_count * CLIPPED_WINDOWS_MILLI {
        b"clipped"
    } else if active * 1000 < window_count * ACTIVE_WINDOWS_MILLI {
        b"low_signal"
    } else {
        b"usable"
    };

    let mut out = Writer::new(buffers.output);
    out.put(b"{\"policy_version\":\"");
    out.put(POLICY_VERSION);
    out.put(b"\",\"duration_ms\":");
    out.number(duration_ms);
    out.put(b",\"sample_rate_hz\":");
    out.number(rate);
    out.put(b",\"channel_count\":");
    out.number(channels as u64);
    out.put(b",\"rms_milli\":");
    out.number(total.rms_milli() as u64);
    out.put(b",\"peak_milli\":");
    out.number(total.peak_milli() as u64);
    out.put(b",\"clipping_ratio_milli\":");
    out.number(total.clip_ratio_milli() as u64);
    out.put(b",\"quality_verdict\":\"");
    out.put(verdict);
    out.put(b"\",\"window_ms\":");
    out.number(WINDOW_MS);
    out.put(b",\"window_count\":");
    out.number(window_count as u64);
    out.put(b",\"intervals\":");
    out.put(intervals);
    out.put(b"}");
    if out.overflowed {
        return Err(Failure::InputLimitExceeded);
    }
    Ok(out.pos)
}

/// The `[...]` intervals array from `classify`'s output, or `None` when it
/// reported an error.
fn interval_array(classified: &[u8]) -> Option<&[u8]> {
    let key = b"\"intervals\":";
    let start = find(classified, key)? + key.len();
    let end = find(classified, b",\"represented_window_count\"")?;
    classified.get(start..end)
}

fn find(input: &[u8], key: &[u8]) -> Option<usize> {
    input.windows(key.len()).position(|part| part == key)
}

pub fn write_failure(output: &mut [u8], failure: Failure) -> usize {
    let mut out = Writer::new(output);
    out.put(b"{\"error\":\"");
    out.put(failure.code());
    out.put(b"\"}");
    out.pos
}

struct Writer<'a> {
    bytes: &'a mut [u8],
    pos: usize,
    overflowed: bool,
}

impl<'a> Writer<'a> {
    fn new(bytes: &'a mut [u8]) -> Self {
        Writer {
            bytes,
            pos: 0,
            overflowed: false,
        }
    }
    fn put(&mut self, b: &[u8]) {
        let end = self.pos.saturating_add(b.len());
        if end <= self.bytes.len() {
            self.bytes[self.pos..end].copy_from_slice(b);
            self.pos = end;
        } else {
            self.overflowed = true;
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
            self.put(&digits[n..=n]);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasi {
    use super::*;

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
        fn proc_exit(code: u32) -> !;
    }

    static mut INPUT: [u8; MAX_INPUT + 1] = [0; MAX_INPUT + 1];
    static mut WAV: [u8; WAV_BUFFER] = [0; WAV_BUFFER];
    static mut SAMPLES: [i16; MAX_WINDOW_SAMPLES] = [0; MAX_WINDOW_SAMPLES];
    static mut WINDOWS: [u8; WINDOWS_JSON] = [0; WINDOWS_JSON];
    static mut INTERVALS: [u8; INTERVALS_JSON] = [0; INTERVALS_JSON];
    static mut OUTPUT: [u8; MAX_OUTPUT] = [0; MAX_OUTPUT];

    #[unsafe(no_mangle)]
    pub extern "C" fn _start() {
        unsafe {
            let input_ptr = core::ptr::addr_of_mut!(INPUT).cast::<u8>();
            let mut total = 0usize;
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
            let output_ptr = core::ptr::addr_of_mut!(OUTPUT).cast::<u8>();
            let mut buffers = Buffers {
                wav: core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(WAV).cast::<u8>(),
                    WAV_BUFFER,
                ),
                samples: core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(SAMPLES).cast::<i16>(),
                    MAX_WINDOW_SAMPLES,
                ),
                windows_json: core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(WINDOWS).cast::<u8>(),
                    WINDOWS_JSON,
                ),
                intervals_json: core::slice::from_raw_parts_mut(
                    core::ptr::addr_of_mut!(INTERVALS).cast::<u8>(),
                    INTERVALS_JSON,
                ),
                output: core::slice::from_raw_parts_mut(output_ptr, MAX_OUTPUT),
            };
            let (length, status) = match analyze(input, &mut buffers) {
                Ok(length) => (length, 0),
                Err(failure) => (
                    write_failure(buffers.output, failure),
                    failure.exit_status(),
                ),
            };
            let mut written = 0usize;
            let vector = Iovec {
                buffer: output_ptr,
                length,
            };
            let _ = fd_write(1, &vector, 1, &mut written);
            if status != 0 {
                proc_exit(status);
            }
        }
    }

    #[panic_handler]
    fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
        loop {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let align = channels * 2;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1u16.to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&rate.to_le_bytes());
        fmt.extend_from_slice(&(rate * align as u32).to_le_bytes());
        fmt.extend_from_slice(&align.to_le_bytes());
        fmt.extend_from_slice(&16u16.to_le_bytes());
        let mut body = b"WAVE".to_vec();
        body.extend_from_slice(b"fmt ");
        body.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        body.extend_from_slice(&fmt);
        body.extend_from_slice(b"data");
        body.extend_from_slice(&(data.len() as u32).to_le_bytes());
        body.extend_from_slice(&data);
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn base64(bytes: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(TABLE[(v >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    fn request(wav: &[u8]) -> Vec<u8> {
        format!("{{\"artifact_base64\":\"{}\"}}", base64(wav)).into_bytes()
    }

    fn run(input: &[u8]) -> Result<String, Failure> {
        let mut wav = vec![0u8; WAV_BUFFER];
        let mut samples = vec![0i16; MAX_WINDOW_SAMPLES];
        let mut windows_json = vec![0u8; WINDOWS_JSON];
        let mut intervals_json = vec![0u8; INTERVALS_JSON];
        let mut output = vec![0u8; MAX_OUTPUT];
        let mut buffers = Buffers {
            wav: &mut wav,
            samples: &mut samples,
            windows_json: &mut windows_json,
            intervals_json: &mut intervals_json,
            output: &mut output,
        };
        let length = analyze(input, &mut buffers)?;
        Ok(String::from_utf8(output[..length].to_vec()).expect("utf8"))
    }

    fn tone(frames: usize, amplitude: i16) -> Vec<i16> {
        (0..frames)
            .map(|i| if i % 2 == 0 { amplitude } else { -amplitude })
            .collect()
    }

    #[test]
    fn usable_recording_reports_summary_and_one_interval() {
        let out = run(&request(&wav(16_000, 1, &tone(16_000, 8_000)))).expect("ok");
        assert!(out.starts_with("{\"policy_version\":\"recording-analyze-1\",\"duration_ms\":1000,\"sample_rate_hz\":16000,\"channel_count\":1,"), "{out}");
        assert!(out.contains("\"rms_milli\":244"), "{out}");
        assert!(out.contains("\"peak_milli\":244"), "{out}");
        assert!(out.contains("\"clipping_ratio_milli\":0"), "{out}");
        assert!(out.contains("\"quality_verdict\":\"usable\""), "{out}");
        assert!(
            out.contains("\"window_ms\":250,\"window_count\":4"),
            "{out}"
        );
        assert!(out.ends_with("\"intervals\":[{\"start_ms\":0,\"end_ms\":1000,\"activity\":\"possible-event\",\"window_count\":4}]}"), "{out}");
    }

    #[test]
    fn silence_is_a_successful_silent_verdict() {
        let out = run(&request(&wav(8_000, 1, &[0; 8_000]))).expect("ok");
        assert!(out.contains("\"quality_verdict\":\"silent\""), "{out}");
        assert!(out.contains("\"activity\":\"silence\""), "{out}");
    }

    #[test]
    fn clipped_and_low_signal_verdicts() {
        let clipped = run(&request(&wav(8_000, 1, &tone(8_000, i16::MAX)))).expect("ok");
        assert!(
            clipped.contains("\"quality_verdict\":\"clipped\""),
            "{clipped}"
        );

        // One active window out of eleven: under 10% of windows active.
        let mut samples = vec![0i16; 2_000 * 10];
        samples.extend(tone(2_000, 8_000));
        let low = run(&request(&wav(8_000, 1, &samples))).expect("ok");
        assert!(low.contains("\"quality_verdict\":\"low_signal\""), "{low}");
        assert!(
            low.contains(
                "{\"start_ms\":0,\"end_ms\":2500,\"activity\":\"silence\",\"window_count\":10}"
            ),
            "{low}"
        );
        assert!(low.contains("{\"start_ms\":2500,\"end_ms\":2750,\"activity\":\"possible-event\",\"window_count\":1}"), "{low}");
    }

    #[test]
    fn partial_final_window_and_stereo_and_odd_rates() {
        // 44.1 kHz stereo, 1.1 s: windows end at the true duration.
        let out = run(&request(&wav(
            44_100,
            2,
            &tone(44_100 * 11 / 10 * 2, 4_000),
        )))
        .expect("ok");
        assert!(
            out.contains("\"duration_ms\":1100,\"sample_rate_hz\":44100,\"channel_count\":2"),
            "{out}"
        );
        assert!(out.contains("\"window_count\":5"), "{out}");
        assert!(out.contains("\"end_ms\":1100"), "{out}");
        // 22.05 kHz: a window is 5512.5 frames, so boundaries floor per window.
        let out = run(&request(&wav(22_050, 1, &tone(22_050, 4_000)))).expect("ok");
        assert!(out.contains("\"duration_ms\":1000"), "{out}");
        assert!(out.contains("\"window_count\":4"), "{out}");
        // One frame: a single 1 ms window, never an empty interval.
        let out = run(&request(&wav(48_000, 1, &[100]))).expect("ok");
        assert!(out.contains("\"duration_ms\":1,"), "{out}");
        assert!(out.contains("{\"start_ms\":0,\"end_ms\":1,"), "{out}");
    }

    #[test]
    fn largest_recording_fits_and_one_more_byte_does_not() {
        // 8 kHz mono maximises the window count for the 8 MiB cap.
        let header = wav(8_000, 1, &[]).len();
        let frames = (MAX_WAV - header) / 2;
        let max = wav(8_000, 1, &vec![0; frames]);
        assert_eq!(max.len(), MAX_WAV);
        let out = run(&request(&max)).expect("ok");
        assert!(out.contains("\"window_count\":2098"), "{}", &out[..200]);
        assert!(out.contains("\"quality_verdict\":\"silent\""));

        let mut over = max.clone();
        over.push(0);
        assert_eq!(run(&request(&over)), Err(Failure::InputLimitExceeded));
        let huge = vec![b'A'; MAX_INPUT + 1];
        assert_eq!(run(&huge), Err(Failure::InputLimitExceeded));
    }

    #[test]
    fn unsupported_and_corrupt_inputs_fail_with_stable_codes() {
        let mut float = wav(16_000, 1, &[1, 2]);
        float[20] = 3; // format tag 3: IEEE float
        assert_eq!(run(&request(&float)), Err(Failure::UnsupportedFormat));
        assert_eq!(
            run(&request(&wav(4_000, 1, &[1]))),
            Err(Failure::UnsupportedFormat)
        );
        assert_eq!(
            run(&request(&wav(16_000, 3, &[1, 2, 3]))),
            Err(Failure::UnsupportedFormat)
        );
        let mut bits = wav(16_000, 1, &[1]);
        bits[34] = 8; // 8-bit PCM
        assert_eq!(run(&request(&bits)), Err(Failure::UnsupportedFormat));

        assert_eq!(
            run(br#"{"wav_bytes_base64":"UklGRg=="}"#),
            Err(Failure::CorruptInput)
        );
        assert_eq!(
            run(br#"{"artifact_base64":"not base64!"}"#),
            Err(Failure::CorruptInput)
        );
        assert_eq!(
            run(&request(b"RIFF\x04\x00\x00\x00WAVE")),
            Err(Failure::CorruptInput)
        );
        assert_eq!(
            run(&request(&wav(16_000, 1, &[]))),
            Err(Failure::CorruptInput)
        );
    }

    #[test]
    fn failure_codes_and_exit_statuses_are_stable() {
        let all = [
            (
                Failure::UnsupportedFormat,
                10,
                "analysis.unsupported_format",
            ),
            (
                Failure::InputLimitExceeded,
                11,
                "analysis.input_limit_exceeded",
            ),
            (Failure::CorruptInput, 12, "analysis.corrupt_input"),
        ];
        for (failure, status, code) in all {
            assert_eq!(failure.exit_status(), status);
            let mut out = [0u8; 64];
            let n = write_failure(&mut out, failure);
            assert_eq!(&out[..n], format!("{{\"error\":\"{code}\"}}").as_bytes());
        }
    }

    #[test]
    fn writer_flags_overflow_and_interval_array_rejects_errors() {
        let mut small = [0u8; 4];
        let mut w = Writer::new(&mut small);
        w.put(b"abcde");
        assert!(w.overflowed);
        assert_eq!(
            interval_array(br#"{"error":"invalid_window_interval"}"#),
            None
        );
        assert_eq!(
            interval_array(
                br#"{"policy_version":"activity-1","intervals":[],"represented_window_count":0}"#
            ),
            Some(&b"[]"[..])
        );
    }
}
