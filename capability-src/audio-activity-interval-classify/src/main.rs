//! WASI-command shim for `audio.activity-interval-classify`; the logic lives
//! in `lib.rs`, which `audio.recording-analyze` (registry#594) also depends on.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![cfg_attr(test, allow(dead_code))]

#[cfg(not(test))]
use activity_interval_classify::classify;
use activity_interval_classify::{INPUT_LIMIT, OUTPUT_LIMIT};

#[repr(C)] struct Iovec { buffer: *const u8, length: usize }
#[repr(C)] struct IovecMut { buffer: *mut u8, length: usize }
#[link(wasm_import_module = "wasi_snapshot_preview1")]
unsafe extern "C" {
    fn fd_read(fd: u32, vectors: *const IovecMut, count: usize, read: *mut usize) -> u32;
    fn fd_write(fd: u32, vectors: *const Iovec, count: usize, written: *mut usize) -> u32;
}
static mut INPUT: [u8; INPUT_LIMIT] = [0; INPUT_LIMIT];
static mut OUTPUT: [u8; OUTPUT_LIMIT] = [0; OUTPUT_LIMIT];

#[cfg(not(test))]
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

#[cfg(not(test))]
#[cfg(not(test))]
#[panic_handler] fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop {} }
