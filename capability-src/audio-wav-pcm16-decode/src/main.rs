//! WASI-command shim for `audio.wav-pcm16-decode`. All decoding logic lives in
//! `lib.rs`, which `audio.recording-analyze` (registry#594) also depends on.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[cfg(target_arch = "wasm32")]
use wav_pcm16_decode::{decode, MAX_INPUT, MAX_OUTPUT, MAX_WAV};

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
#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
