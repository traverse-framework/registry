//! Signal-quality metrics for `audio.signal-quality-evaluate`. The metric and
//! state logic lives here so `audio.recording-analyze` (registry#594,
//! decision-log entry 141) reuses it; `main.rs` is the WASI-command shim.
#![cfg_attr(not(test), no_std)]
pub const MAX_INPUT:usize=1_000_000; pub const MAX_OUTPUT:usize=2048; pub const MAX_SAMPLES:usize=48_000;
fn key(i:&[u8],k:&[u8])->Option<usize>{i.windows(k.len()).position(|w|w==k)}
fn int(i:&[u8],k:&[u8])->Option<i32>{let p=key(i,k)?;let c=i[p+k.len()..].iter().position(|b|*b==b':')?;let mut r=&i[p+k.len()+c+1..];while r.first().is_some_and(|b|b.is_ascii_whitespace()){r=&r[1..]}let mut n=0;let mut sign=1;if r.first()==Some(&b'-'){sign=-1;r=&r[1..]}let mut any=false;for b in r{if !b.is_ascii_digit(){break}n=n*10+(*b-b'0')as i32;any=true}if any{Some(n*sign)}else{None}}
fn samples(i:&[u8],s:&mut[i16])->Option<usize>{let p=key(i,b"\"samples\"")?;let mut r=&i[p..];r=&r[r.iter().position(|b|*b==b'[')?+1..];let mut n=0;loop{while r.first().is_some_and(|b|b.is_ascii_whitespace()||*b==b','){r=&r[1..]}if r.first()==Some(&b']'){return Some(n)}let mut sign=1;if r.first()==Some(&b'-'){sign=-1;r=&r[1..]}let mut v=0;let mut any=false;for b in r{if !b.is_ascii_digit(){break}v=v*10+(*b-b'0')as i32;any=true}if !any||n==s.len()||v*sign< -32768||v*sign>32767{return None} s[n]=(v*sign)as i16;n+=1;while r.first().is_some_and(|b|b.is_ascii_digit()){r=&r[1..]}if r.first()==Some(&b','){r=&r[1..]}}}
fn put(o:&mut[u8],at:&mut usize,b:&[u8]){if *at+b.len()<=o.len(){o[*at..*at+b.len()].copy_from_slice(b);*at+=b.len()}}
fn num(o:&mut[u8],at:&mut usize,mut n:i32){let mut b=[0;12];let mut c=0;if n==0{put(o,at,b"0");return}if n<0{put(o,at,b"-");n=-n}while n>0{b[c]=b'0'+(n%10)as u8;n/=10;c+=1}while c>0{c-=1;put(o,at,&b[c..c+1])}}
fn isqrt(n:i64)->i64{if n<=0{return 0}let(mut x,mut y)=(n,(n+1)/2);while y<x{x=y;y=(x+n/x)/2}x}
/// Whole-buffer signal metrics for interleaved PCM16 samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics { pub sample_count: usize, pub sum_squares: i64, pub peak: i32, pub clipping_samples: i32, pub zero_crossings: i32 }
impl Metrics {
    pub fn rms_milli(&self) -> i32 { if self.sample_count == 0 { return 0 } (isqrt(self.sum_squares / self.sample_count as i64) * 1000 / 32768) as i32 }
    pub fn peak_milli(&self) -> i32 { self.peak * 1000 / 32768 }
    pub fn clip_ratio_milli(&self) -> i32 { if self.sample_count == 0 { return 0 } self.clipping_samples * 1000 / self.sample_count as i32 }
}
/// Default thresholds of the published capability.
pub const DEFAULT_CLIP_THRESHOLD: i32 = 32760; pub const DEFAULT_QUIET_RMS_MILLI: i32 = 5; pub const DEFAULT_CLIP_RATIO_MILLI: i32 = 10;
pub fn evaluate(s: &[i16], clip: i32) -> Metrics { let mut m = Metrics { sample_count: s.len(), sum_squares: 0, peak: 0, clipping_samples: 0, zero_crossings: 0 }; for x in 0..s.len() { let v = s[x] as i32; let a = if v < 0 { -v } else { v }; if a > m.peak { m.peak = a } m.sum_squares += (v as i64) * (v as i64); if a >= clip { m.clipping_samples += 1 } if x > 0 && ((s[x-1] < 0 && s[x] >= 0) || (s[x-1] >= 0 && s[x] < 0)) { m.zero_crossings += 1 } } m }
/// `clipped`, `quiet` or `active`, in that order of precedence.
pub fn state(m: &Metrics, quiet: i32, ratio: i32) -> &'static [u8] { if m.clip_ratio_milli() >= ratio { b"clipped" } else if m.rms_milli() < quiet { b"quiet" } else { b"active" } }
pub fn run(i:&[u8],o:&mut[u8],s:&mut[i16])->usize{let Some(n)=samples(i,s)else{return err(o,b"invalid_samples")};let rate=int(i,b"\"sample_rate_hz\"").unwrap_or(0);if n==0||rate<1{return err(o,b"invalid_request")}let clip=int(i,b"\"clip_threshold\"").unwrap_or(DEFAULT_CLIP_THRESHOLD);let quiet=int(i,b"\"quiet_rms_milli\"").unwrap_or(DEFAULT_QUIET_RMS_MILLI);let ratio=int(i,b"\"clip_ratio_milli\"").unwrap_or(DEFAULT_CLIP_RATIO_MILLI);let m=evaluate(&s[..n],clip);let st=state(&m,quiet,ratio);let mut a=0;put(o,&mut a,b"{\"sample_count\":");num(o,&mut a,n as i32);put(o,&mut a,b",\"sample_rate_hz\":");num(o,&mut a,rate);put(o,&mut a,b",\"rms_milli\":");num(o,&mut a,m.rms_milli());put(o,&mut a,b",\"peak_milli\":");num(o,&mut a,m.peak_milli());put(o,&mut a,b",\"clipping_samples\":");num(o,&mut a,m.clipping_samples);put(o,&mut a,b",\"clip_ratio_milli\":");num(o,&mut a,m.clip_ratio_milli());put(o,&mut a,b",\"zero_crossings\":");num(o,&mut a,m.zero_crossings);put(o,&mut a,b",\"state\":\"");put(o,&mut a,st);put(o,&mut a,b"\"}");a}
fn err(o:&mut[u8],c:&[u8])->usize{let mut a=0;put(o,&mut a,b"{\"error\":\"");put(o,&mut a,c);put(o,&mut a,b"\"}");a}
#[cfg(test)]
mod tests {
    use super::*;

    fn run_json(input: &[u8]) -> String {
        let mut output = [0u8; MAX_OUTPUT];
        let mut samples_buf = [0i16; MAX_SAMPLES];
        let length = run(input, &mut output, &mut samples_buf);
        String::from_utf8(output[..length].to_vec()).unwrap()
    }

    #[test]
    fn classifies_active_clipped_and_quiet_audio() {
        let active = run_json(br#"{"sample_rate_hz":16000,"samples":[1000,-1000,500]}"#);
        assert!(active.contains("\"state\":\"active\""));
        assert!(active.contains("\"zero_crossings\":2"));

        let clipped = run_json(br#"{"sample_rate_hz":16000,"clip_threshold":100,"clip_ratio_milli":500,"samples":[100,100,0,0]}"#);
        assert!(clipped.contains("\"state\":\"clipped\""));

        let quiet = run_json(br#"{"sample_rate_hz":16000,"quiet_rms_milli":100,"samples":[1,0,-1]}"#);
        assert!(quiet.contains("\"state\":\"quiet\""));
    }

    #[test]
    fn rejects_invalid_requests_and_sample_values() {
        assert!(run_json(br#"{"sample_rate_hz":0,"samples":[1]}"#).contains("invalid_request"));
        assert!(run_json(br#"{"sample_rate_hz":1,"samples":[]}"#).contains("invalid_request"));
        assert!(run_json(br#"{"sample_rate_hz":1}"#).contains("invalid_samples"));
        assert!(run_json(br#"{"sample_rate_hz":1,"samples":[40000]}"#).contains("invalid_samples"));
        assert!(run_json(br#"{"sample_rate_hz":1,"samples":[-40000]}"#).contains("invalid_samples"));
    }

    #[test]
    fn parses_numbers_and_square_roots() {
        assert_eq!(int(br#"{"x":-12}"#, b"\"x\""), Some(-12));
        assert_eq!(int(br#"{"x":"bad"}"#, b"\"x\""), None);
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(16), 4);
    }
}
